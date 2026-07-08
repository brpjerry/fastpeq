//! Per-preset version history: normalized snapshots of a preset's prior EQ,
//! recorded before every destructive mutation so overwritten saves and deleted
//! presets are recoverable (see `docs/PRESET-HISTORY.md`).
//!
//! Snapshots live at `<preset store>/.history/<name>/<unix-ms>-<op>.txt` —
//! plain APO text, hand-recoverable by design, following the library when the
//! store is relocated. Each is the preset's EQ in **normal form** (see
//! [`normalize`]): the master preamp and no-op filters are stripped (both are
//! derived/inert, not history), while balance trims, disabled bands, and
//! unmodeled raw lines round-trip verbatim.
//!
//! Two invariants keep the history free of duplicates, both enforced at record
//! time on the normalized form:
//!
//! 1. **No two revisions with the same content** — recording a revision whose
//!    content already exists removes the older copy, so the content keeps its
//!    most recent position in the timeline.
//! 2. **No revision matching the live preset** ([`PresetHistory::remove_matching`],
//!    called by the manager after each preset write) — the canonical case being
//!    restore-then-save, where the restored snapshot would otherwise linger as
//!    a duplicate of the preset file itself.

use crate::apo::model::{Channel, Config, Filter, Line};
use crate::apo::{parse, serialize, writer};
use crate::provenance;
use crate::store::is_safe_name;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// How many revisions to keep per preset; the oldest beyond this are pruned at
/// record time. No age-based pruning — a delete snapshot's value *grows* with
/// age, and 30 × ~1 KB per edited preset is beneath caring about.
const KEEP_MAX: usize = 30;

const REV_EXT: &str = "txt";

/// What displaced a snapshot's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RevisionOp {
    /// Overwritten by a Save (or a capture over an existing name).
    Save,
    /// The preset was deleted; this snapshot *is* the undo-delete.
    Delete,
    /// Overwritten by restoring an older revision.
    Restore,
}

impl RevisionOp {
    fn as_str(self) -> &'static str {
        match self {
            RevisionOp::Save => "save",
            RevisionOp::Delete => "delete",
            RevisionOp::Restore => "restore",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "save" => RevisionOp::Save,
            "delete" => RevisionOp::Delete,
            "restore" => RevisionOp::Restore,
            _ => return None,
        })
    }
}

/// One recorded snapshot, as listed to the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    /// `<unix-ms>-<op>` — also the file stem, so it doubles as the load key.
    pub id: String,
    pub saved_at_ms: u64,
    pub op: RevisionOp,
    /// The user's name for this version, if any (see [`tag_of`]).
    pub tag: Option<String>,
}

/// The version-tag marker: a user-editable name for a revision, carried as a
/// comment (`# fastpeq:tag=<text>`) so it rides revision files, the preset
/// file, and `config.txt` the way the provenance stamp does. It is metadata,
/// not content — [`normalize`] strips it, so a tag never affects the dedupe
/// invariants; it simply travels with the content it describes.
const TAG_PREFIX: &str = "# fastpeq:tag=";

/// The version tag recorded in `config`, if any (empty tags read as none).
pub fn tag_of(config: &Config) -> Option<String> {
    config.lines.iter().find_map(|l| match l {
        Line::Raw(s) => s
            .trim()
            .strip_prefix(TAG_PREFIX)
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty()),
        _ => None,
    })
}

/// `config` with its version tag replaced by `tag` — or removed, when `tag`
/// trims to empty. The tag leads the file, like the provenance stamp.
pub fn with_tag(config: &Config, tag: &str) -> Config {
    let tag = tag.trim();
    let mut lines: Vec<Line> = Vec::with_capacity(config.lines.len() + 1);
    if !tag.is_empty() {
        lines.push(Line::Raw(format!("{TAG_PREFIX}{tag}")));
    }
    lines.extend(config.lines.iter().filter(|l| !is_tag(l)).cloned());
    Config { lines }
}

fn is_tag(line: &Line) -> bool {
    matches!(line, Line::Raw(s) if s.trim().starts_with(TAG_PREFIX))
}

/// A preset's EQ in the **normal form** snapshots are stored and compared in:
/// the master (`Both`-channel) `Preamp:` lines, no-op filters, and any
/// provenance stamp removed. Everything else — balance trims, disabled bands
/// with real settings, unmodeled raw lines — is untouched.
///
/// The master preamp is *derived* state (Auto Preamp rewrites it; a restore
/// recomputes it), and a no-op filter — a gain-type filter sitting at 0 dB,
/// exactly what the editor's "Remove 0 dB" button calls "no effect" — changes
/// nothing audible. Stripping both means two contents that *sound* identical
/// also read as identical to the dedupe invariants.
pub fn normalize(config: &Config) -> Config {
    let stripped = provenance::strip(config);
    Config {
        lines: stripped
            .lines
            .into_iter()
            .filter(|l| match l {
                Line::Preamp {
                    channel: Channel::Both,
                    ..
                } => false,
                Line::Filter(f) => !is_noop_filter(f),
                Line::Raw(_) => !is_tag(l), // version tags are metadata, not EQ
                _ => true,
            })
            .collect(),
    }
}

/// A gain-type filter at 0 dB shapes nothing (a missing `Gain` acts as 0).
fn is_noop_filter(f: &Filter) -> bool {
    f.kind.has_gain() && f.gain.unwrap_or(0.0) == 0.0
}

/// The canonical stored/compared text of a config: normalized, serialized,
/// newline-terminated (matching the writer convention).
fn normal_text(config: &Config) -> String {
    let mut text = serialize(&normalize(config));
    text.push('\n');
    text
}

/// The file-backed revision store for one preset library.
#[derive(Debug, Clone)]
pub struct PresetHistory {
    dir: PathBuf, // <preset store>/.history
}

impl PresetHistory {
    /// The history alongside a preset store directory.
    pub fn new(store_dir: &Path) -> Self {
        PresetHistory {
            dir: store_dir.join(".history"),
        }
    }

    /// The directory holding `name`'s revisions, name-validated like
    /// [`PresetStore::path_for`](crate::store::PresetStore::path_for) — the
    /// name becomes a directory name, so nothing unsafe may pass.
    fn preset_dir(&self, name: &str) -> io::Result<PathBuf> {
        let name = name.trim();
        if name.is_empty() || !is_safe_name(name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid preset name: {name:?}"),
            ));
        }
        Ok(self.dir.join(name))
    }

    /// Record `prior` (the content a mutation is about to displace) as a
    /// revision of `name`. Normalizes first, removes any older revision with
    /// the same content (invariant 1), and prunes to the newest [`KEEP_MAX`].
    /// Returns the new revision id.
    pub fn record(&self, name: &str, prior: &Config, op: RevisionOp) -> io::Result<String> {
        let text = normal_text(prior);
        let dir = self.preset_dir(name)?;
        fs::create_dir_all(&dir)?;

        // Invariant 1: the same content never appears twice — drop older copies
        // so this recording becomes the content's (single) timeline position.
        for rev in self.list(name)? {
            if self.revision_text(&dir, &rev.id)?.as_deref() == Some(text.as_str()) {
                let _ = fs::remove_file(rev_path(&dir, &rev.id));
            }
        }

        let id = unique_id(&dir, op);
        // The stored file re-attaches the displaced content's tag — metadata
        // rides with its content, even though the comparisons above ignore it.
        let stored = match tag_of(prior) {
            Some(tag) => {
                let mut s = serialize(&with_tag(&normalize(prior), &tag));
                s.push('\n');
                s
            }
            None => text,
        };
        writer::write_text_atomic(&rev_path(&dir, &id), &stored)?;
        self.prune(name)?;
        Ok(id)
    }

    /// Set a revision's tag — or clear it, with a string that trims to empty.
    pub fn set_tag(&self, name: &str, id: &str, tag: &str) -> io::Result<()> {
        let dir = self.preset_dir(name)?;
        if parse_id(id).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid revision id: {id:?}"),
            ));
        }
        let path = rev_path(&dir, id);
        let text = fs::read_to_string(&path)?;
        let mut out = serialize(&with_tag(&parse(&text), tag));
        out.push('\n');
        writer::write_text_atomic(&path, &out)
    }

    /// Invariant 2: remove every revision whose content matches `current` (the
    /// preset's live content after a write) — a snapshot of what the preset
    /// *is* isn't history. Called by the manager after save and restore.
    pub fn remove_matching(&self, name: &str, current: &Config) -> io::Result<()> {
        let text = normal_text(current);
        let dir = self.preset_dir(name)?;
        for rev in self.list(name)? {
            if self.revision_text(&dir, &rev.id)?.as_deref() == Some(text.as_str()) {
                let _ = fs::remove_file(rev_path(&dir, &rev.id));
            }
        }
        Ok(())
    }

    /// The revisions of `name`, newest first. A preset with no history (or no
    /// history directory at all) yields an empty list.
    pub fn list(&self, name: &str) -> io::Result<Vec<Revision>> {
        let dir = self.preset_dir(name)?;
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut revisions: Vec<Revision> = entries
            .flatten()
            .filter_map(|e| {
                let mut rev = parse_revision(&e.path())?;
                rev.tag = fs::read_to_string(e.path())
                    .ok()
                    .and_then(|text| tag_of(&parse(&text)));
                Some(rev)
            })
            .collect();
        revisions.sort_by(|a, b| b.saved_at_ms.cmp(&a.saved_at_ms).then(b.id.cmp(&a.id)));
        Ok(revisions)
    }

    /// Load one revision as a parsed (normalized) config. The id is validated
    /// against the revision-name grammar — it becomes a file name, so an
    /// arbitrary string from IPC must not reach the filesystem.
    pub fn load(&self, name: &str, id: &str) -> io::Result<Config> {
        let dir = self.preset_dir(name)?;
        if parse_id(id).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid revision id: {id:?}"),
            ));
        }
        let text = fs::read_to_string(rev_path(&dir, id))?;
        Ok(parse(&text))
    }

    /// Revision counts per preset name (presets without history are absent) —
    /// the preset list's version-badge data: a preset's *current* content is
    /// version `count + 1`, its oldest snapshot v1.
    pub fn counts(&self) -> io::Result<BTreeMap<String, usize>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(e),
        };
        let mut out = BTreeMap::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let n = fs::read_dir(&path)?
                .flatten()
                .filter(|e| parse_revision(&e.path()).is_some())
                .count();
            if n > 0 {
                out.insert(name.to_string(), n);
            }
        }
        Ok(out)
    }

    /// Move `from`'s history to `to` (a preset rename). Merges file-by-file
    /// when the target already has history (renaming onto a previously-deleted
    /// name); id collisions bump the millisecond like [`record`](Self::record).
    pub fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let from_dir = self.preset_dir(from)?;
        let to_dir = self.preset_dir(to)?;
        if !from_dir.exists() || from_dir == to_dir {
            return Ok(());
        }
        if !to_dir.exists() {
            // The parent (.history) exists iff from_dir does.
            return fs::rename(&from_dir, &to_dir);
        }
        for rev in self.list(from)? {
            let Some((mut ms, op)) = parse_id(&rev.id) else {
                continue;
            };
            let mut target = rev_path(&to_dir, &rev.id);
            while target.exists() {
                ms += 1;
                target = rev_path(&to_dir, &format!("{ms}-{}", op.as_str()));
            }
            fs::rename(rev_path(&from_dir, &rev.id), target)?;
        }
        let _ = fs::remove_dir(&from_dir); // best-effort: empty now
        Ok(())
    }

    /// Drop the oldest revisions beyond [`KEEP_MAX`].
    fn prune(&self, name: &str) -> io::Result<()> {
        let dir = self.preset_dir(name)?;
        for rev in self.list(name)?.into_iter().skip(KEEP_MAX) {
            let _ = fs::remove_file(rev_path(&dir, &rev.id));
        }
        Ok(())
    }

    /// A stored revision's canonical text (re-normalized, so even a hand-edited
    /// revision file compares correctly), or `None` if it vanished meanwhile.
    fn revision_text(&self, dir: &Path, id: &str) -> io::Result<Option<String>> {
        match fs::read_to_string(rev_path(dir, id)) {
            Ok(text) => Ok(Some(normal_text(&parse(&text)))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}

fn rev_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.{REV_EXT}"))
}

/// `<unix-ms>-<op>` from a revision file path, or `None` for foreign files.
fn parse_revision(path: &Path) -> Option<Revision> {
    if path.extension().and_then(|s| s.to_str()) != Some(REV_EXT) {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    let (saved_at_ms, op) = parse_id(stem)?;
    Some(Revision {
        id: stem.to_string(),
        saved_at_ms,
        op,
        tag: None, // filled by list(), which reads the file anyway
    })
}

/// Split a revision id into its timestamp and op, or `None` if malformed.
fn parse_id(id: &str) -> Option<(u64, RevisionOp)> {
    let (ms, op) = id.split_once('-')?;
    Some((ms.parse().ok()?, RevisionOp::from_str(op)?))
}

/// A fresh id at (or just after) now — bumping the millisecond past any
/// existing file, so two records in the same millisecond stay distinct.
fn unique_id(dir: &Path, op: RevisionOp) -> String {
    let mut ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    loop {
        let id = format!("{ms}-{}", op.as_str());
        if !rev_path(dir, &id).exists() {
            return id;
        }
        ms += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apo::model::FilterKind;

    fn peak(freq: f64, gain: f64) -> Line {
        Line::Filter(Filter::peak(freq, gain, 1.0))
    }

    fn preamp(gain: f64) -> Line {
        Line::Preamp {
            gain,
            channel: Channel::Both,
        }
    }

    #[test]
    fn normalize_strips_master_preamp_and_noop_filters_only() {
        let disabled = Line::Filter(Filter {
            enabled: false,
            ..Filter::peak(3000.0, -3.0, 2.0)
        });
        let noop = peak(2000.0, 0.0);
        let gainless_pk = Line::Filter(Filter {
            gain: None, // acts as 0 dB — a no-op too
            ..Filter::peak(4000.0, 0.0, 1.0)
        });
        let trim = Line::Preamp {
            gain: -2.0,
            channel: Channel::Left,
        };
        let lowpass = Line::Filter(Filter {
            kind: FilterKind::LowPass,
            gain: None,
            q: None,
            ..Filter::peak(5000.0, 0.0, 1.0)
        });
        let config = Config {
            lines: vec![
                Line::Raw("# fastpeq:preset=Old".into()),
                Line::Raw("# a comment".into()),
                preamp(-6.0),
                trim.clone(),
                peak(1000.0, 3.0),
                noop,
                gainless_pk,
                disabled.clone(),
                lowpass.clone(),
            ],
        };

        let normalized = normalize(&config);
        assert_eq!(
            normalized.lines,
            vec![
                Line::Raw("# a comment".into()),
                trim,
                peak(1000.0, 3.0),
                disabled,
                lowpass,
            ],
            "keep raw lines, trims, real bands, disabled bands, gainless kinds; \
             strip stamp, master preamp, 0 dB gain-type filters"
        );
    }

    #[test]
    fn record_list_load_round_trips() {
        let tmp = tempdir("record");
        let history = PresetHistory::new(tmp.path());
        let config = Config {
            lines: vec![Line::Raw("# 中文注释".into()), peak(1000.0, 3.0)],
        };

        let id = history.record("HD600", &config, RevisionOp::Save).unwrap();
        let listed = history.list("HD600").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);
        assert_eq!(listed[0].op, RevisionOp::Save);

        // The snapshot round-trips: raw lines verbatim, already-normal content
        // unchanged.
        assert_eq!(history.load("HD600", &id).unwrap(), normalize(&config));
    }

    #[test]
    fn same_millisecond_records_get_distinct_ids() {
        let tmp = tempdir("collide");
        let history = PresetHistory::new(tmp.path());
        let a = history
            .record(
                "P",
                &Config {
                    lines: vec![peak(100.0, 1.0)],
                },
                RevisionOp::Save,
            )
            .unwrap();
        let b = history
            .record(
                "P",
                &Config {
                    lines: vec![peak(200.0, 2.0)],
                },
                RevisionOp::Save,
            )
            .unwrap();
        assert_ne!(a, b);
        assert_eq!(history.list("P").unwrap().len(), 2);
    }

    #[test]
    fn recording_duplicate_content_keeps_one_copy_at_the_newest_position() {
        let tmp = tempdir("dupe");
        let history = PresetHistory::new(tmp.path());
        let content = Config {
            lines: vec![peak(1000.0, 3.0)],
        };
        let other = Config {
            lines: vec![peak(2000.0, -2.0)],
        };

        let first = history.record("P", &content, RevisionOp::Save).unwrap();
        history.record("P", &other, RevisionOp::Save).unwrap();
        // The same content again — the preamp difference must not defeat the
        // dedupe (both normalize identically).
        let mut with_preamp = content.clone();
        with_preamp.lines.insert(0, preamp(-9.0));
        let again = history.record("P", &with_preamp, RevisionOp::Save).unwrap();

        let ids: Vec<String> = history
            .list("P")
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert!(!ids.contains(&first), "older duplicate must be removed");
        assert_eq!(ids.len(), 2, "{ids:?}");
        assert_eq!(ids[0], again, "the content sits at its newest position");
    }

    #[test]
    fn remove_matching_evicts_revisions_equal_to_the_live_preset() {
        let tmp = tempdir("evict");
        let history = PresetHistory::new(tmp.path());
        let content = Config {
            lines: vec![peak(1000.0, 3.0)],
        };
        let other = Config {
            lines: vec![peak(2000.0, -2.0)],
        };
        history.record("P", &content, RevisionOp::Save).unwrap();
        history.record("P", &other, RevisionOp::Delete).unwrap();

        // The live preset now equals `content` (e.g. it was just restored).
        history.remove_matching("P", &content).unwrap();
        let listed = history.list("P").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].op, RevisionOp::Delete);
    }

    #[test]
    fn prune_keeps_the_newest_30() {
        let tmp = tempdir("prune");
        let history = PresetHistory::new(tmp.path());
        for i in 0..33 {
            history
                .record(
                    "P",
                    &Config {
                        lines: vec![peak(100.0 + i as f64, 1.0)],
                    },
                    RevisionOp::Save,
                )
                .unwrap();
        }
        let listed = history.list("P").unwrap();
        assert_eq!(listed.len(), 30);
        // Newest first; the oldest three (100, 101, 102 Hz) are gone.
        let oldest = history.load("P", &listed.last().unwrap().id).unwrap();
        assert!(matches!(
            &oldest.lines[0],
            Line::Filter(f) if f.freq == 103.0
        ));
    }

    #[test]
    fn tags_ride_with_content_but_are_not_identity() {
        // tag_of / with_tag round-trip; empty clears.
        let base = Config {
            lines: vec![peak(1000.0, 3.0)],
        };
        let tagged = with_tag(&base, "Warm bass");
        assert_eq!(tag_of(&tagged).as_deref(), Some("Warm bass"));
        assert_eq!(with_tag(&tagged, ""), base);
        // Metadata, not content: normalize strips it.
        assert_eq!(normalize(&tagged), normalize(&base));

        // A recorded snapshot keeps the displaced content's tag…
        let tmp = tempdir("tags");
        let history = PresetHistory::new(tmp.path());
        let id = history.record("P", &tagged, RevisionOp::Save).unwrap();
        assert_eq!(
            history.list("P").unwrap()[0].tag.as_deref(),
            Some("Warm bass")
        );
        assert_eq!(
            tag_of(&history.load("P", &id).unwrap()).as_deref(),
            Some("Warm bass")
        );

        // …and the tag doesn't defeat the dedupe: recording the same content
        // under a different tag replaces the old copy (newest tag wins).
        let retagged = with_tag(&base, "Renamed");
        let id2 = history.record("P", &retagged, RevisionOp::Save).unwrap();
        let listed = history.list("P").unwrap();
        assert_eq!(listed.len(), 1, "{listed:?}");
        assert_eq!(listed[0].id, id2);
        assert_eq!(listed[0].tag.as_deref(), Some("Renamed"));
    }

    #[test]
    fn set_tag_names_and_clears_a_revision() {
        let tmp = tempdir("settag");
        let history = PresetHistory::new(tmp.path());
        let id = history
            .record(
                "P",
                &Config {
                    lines: vec![peak(1000.0, 3.0)],
                },
                RevisionOp::Save,
            )
            .unwrap();
        assert_eq!(history.list("P").unwrap()[0].tag, None); // default: untagged

        history.set_tag("P", &id, "  V-shaped  ").unwrap();
        assert_eq!(
            history.list("P").unwrap()[0].tag.as_deref(),
            Some("V-shaped") // trimmed
        );
        // The EQ content is untouched by tagging.
        assert_eq!(
            normalize(&history.load("P", &id).unwrap()),
            Config {
                lines: vec![peak(1000.0, 3.0)],
            }
        );

        history.set_tag("P", &id, "").unwrap();
        assert_eq!(history.list("P").unwrap()[0].tag, None);
        // Ids are validated like load's.
        assert!(history.set_tag("P", "../../evil", "x").is_err());
    }

    #[test]
    fn counts_reports_per_preset_revision_totals() {
        let tmp = tempdir("counts");
        let history = PresetHistory::new(tmp.path());
        assert!(history.counts().unwrap().is_empty()); // no .history dir yet

        let a = Config {
            lines: vec![peak(100.0, 1.0)],
        };
        let b = Config {
            lines: vec![peak(200.0, 2.0)],
        };
        history.record("P", &a, RevisionOp::Save).unwrap();
        history.record("P", &b, RevisionOp::Save).unwrap();
        history.record("Q", &a, RevisionOp::Delete).unwrap();

        let counts = history.counts().unwrap();
        assert_eq!(counts.get("P"), Some(&2));
        assert_eq!(counts.get("Q"), Some(&1));
        assert_eq!(counts.get("R"), None);
    }

    #[test]
    fn rename_moves_history_and_merges_into_an_existing_dir() {
        let tmp = tempdir("rename");
        let history = PresetHistory::new(tmp.path());
        let a = Config {
            lines: vec![peak(100.0, 1.0)],
        };
        let b = Config {
            lines: vec![peak(200.0, 2.0)],
        };
        history.record("Old", &a, RevisionOp::Save).unwrap();
        history.record("New", &b, RevisionOp::Delete).unwrap();

        history.rename("Old", "New").unwrap();
        assert!(history.list("Old").unwrap().is_empty());
        let merged = history.list("New").unwrap();
        assert_eq!(merged.len(), 2, "both sets survive the merge");

        // A rename with no history is a no-op, not an error.
        history.rename("Missing", "Elsewhere").unwrap();
    }

    #[test]
    fn unsafe_names_and_malformed_ids_are_rejected() {
        let tmp = tempdir("safe");
        let history = PresetHistory::new(tmp.path());
        assert!(history.list("../evil").is_err());
        assert!(
            history
                .record("CON", &Config::new(), RevisionOp::Save)
                .is_err()
        );
        // An id is a file name; arbitrary strings must not reach the fs.
        assert!(history.load("P", "../../secret").is_err());
        assert!(history.load("P", "123-nonsense").is_err());
    }

    fn tempdir(tag: &str) -> TempDir {
        TempDir::new(tag)
    }

    /// A throwaway directory under the OS temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir()
                .join(format!("fastpeq-hist-{tag}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
