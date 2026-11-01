//! High-level orchestration tying an APO install, a preset library, and the
//! atomic writer into the operations the UI invokes.
//!
//! This is the API the Tauri command layer calls; keeping it here (rather than
//! in `src-tauri`) means the whole switch/capture/bypass flow is unit-testable
//! without a running app.

use crate::apo::env::ApoInstall;
use crate::apo::model::{Config, Line};
use crate::apo::writer;
use crate::category::Category;
use crate::history::{self, PresetHistory, Revision, RevisionOp};
use crate::offload;
use crate::parse;
use crate::provenance;
use crate::store::PresetStore;
use crate::tone::{self, Tone};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Outcome of a bulk import from the APO config folder.
#[derive(Debug, Default, Serialize)]
pub struct ImportReport {
    /// Names of presets newly imported.
    pub imported: Vec<String>,
    /// Names skipped because a preset with that name already existed.
    pub skipped: Vec<String>,
    /// Files ignored (no EQ content, or an unusable name).
    pub ignored: u32,
}

/// The name the pre-existing config gets when a preset grows its first
/// variant ("Default pads"). Only special at that moment — afterwards it is
/// an ordinary variant that can be renamed or deleted.
pub const DEFAULT_VARIANT: &str = "Default";

/// One preset's entry in the `.variants.json` sidecar: which variant the
/// preset file currently mirrors. A struct (not a bare string) so it can grow
/// without a migration.
#[derive(Debug, Default, Serialize, Deserialize)]
struct VariantMeta {
    #[serde(default)]
    selected: Option<String>,
}

/// A preset's variants (sub-presets), as listed to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VariantSet {
    /// Every variant, sorted case-insensitively.
    pub variants: Vec<String>,
    /// The one `<name>.txt` mirrors — `None` only if the sidecar names nothing
    /// (or something that no longer exists); the UI then highlights nothing
    /// until the next pick.
    pub selected: Option<String>,
}

/// What [`Manager::delete_preset`] set aside, so the UI's Undo can bring all
/// of it back: the preset's own `delete` revision, and its variants directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeleteReceipt {
    /// The undo handle for the preset file, or `None` when nothing could be
    /// snapshotted (the UI offers no Undo then).
    pub revision: Option<String>,
    /// The parked variants, when the preset had any.
    pub variants: Option<ArchivedVariants>,
}

/// A deleted preset's variants, parked in the history archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArchivedVariants {
    /// The handle [`Manager::restore_variants`] takes.
    pub archive: String,
    /// Which variant was selected when the preset was deleted, so the undo can
    /// re-select it.
    pub selected: Option<String>,
}

#[derive(Clone)]
pub struct Manager {
    install: ApoInstall,
    store: PresetStore,
    backup_path: PathBuf,
}

impl Manager {
    pub fn new(install: ApoInstall, store: PresetStore, backup_path: PathBuf) -> Self {
        Manager {
            install,
            store,
            backup_path,
        }
    }

    pub fn install(&self) -> &ApoInstall {
        &self.install
    }

    pub fn store(&self) -> &PresetStore {
        &self.store
    }

    pub fn list_presets(&self) -> io::Result<Vec<String>> {
        self.store.list()
    }

    pub fn load_preset(&self, name: &str) -> io::Result<Config> {
        self.store.load(name)
    }

    pub fn save_preset(&self, name: &str, config: &Config) -> io::Result<()> {
        // Snapshot the content this save displaces — before the write, so a
        // crash mid-save can't lose it. Skipped when nothing audible changes
        // (the comparison is on the normalized form: preamp and no-op filters
        // don't count).
        let history = self.history();
        if let Ok(prior) = self.store.load(name)
            && history::normalize(&prior) != history::normalize(config)
        {
            log_history(history.record(name, &prior, RevisionOp::Save));
        }
        self.write_preset(name, config)?;
        // Invariant 2: the preset's new content must not linger as a revision
        // (e.g. the user hand-edited their way back to an old snapshot).
        log_history(history.remove_matching(name, config));
        Ok(())
    }

    /// Write a preset file — and, when the preset has a selected variant, the
    /// variant file it mirrors, so the two never drift apart. Every path that
    /// writes `<name>.txt` for an existing preset goes through here.
    fn write_preset(&self, name: &str, config: &Config) -> io::Result<()> {
        self.store.save(name, config)?;
        if let Some(variant) = self.selected_variant(name) {
            self.store.save_variant(name, &variant, config)?;
        }
        Ok(())
    }

    /// Delete a preset (and its category) by moving its exact source file into
    /// the hidden history archive, and parking its variants directory there
    /// too. The receipt carries the undo handles: the normalized `delete`
    /// snapshot's id (`None` when there was nothing to snapshot or history
    /// recording was unavailable — the UI then offers no Undo instead of a dead
    /// button) and the parked variants. A delete that fails part-way leaves
    /// the preset whole: the variants are parked first (nothing has moved if
    /// that fails) and come back if the preset file then cannot be archived.
    pub fn delete_preset(&self, name: &str) -> io::Result<DeleteReceipt> {
        let source = self.store.path_for(name)?;
        let history = self.history();
        let revision = match self.store.load(name) {
            Ok(prior) => log_history(history.record(name, &prior, RevisionOp::Delete)),
            Err(_) => None,
        };
        let variants_dir = self.store.variants_dir(name)?;
        let variants = if variants_dir.is_dir() {
            let selected = self
                .variant_meta()?
                .get(name)
                .and_then(|meta| meta.selected.clone());
            let archive = history.archive_variants(name, &variants_dir)?;
            Some(ArchivedVariants { archive, selected })
        } else {
            None
        };
        if let Err(e) = history.archive_preset(name, &source) {
            if let Some(parked) = &variants {
                let _ = history.unarchive_variants(name, &parked.archive, &variants_dir);
            }
            return Err(e);
        }
        self.set_selected(name, None)?;
        let mut map = self.categories()?;
        if map.remove(name).is_some() {
            self.write_categories(&map)?;
        }
        Ok(DeleteReceipt { revision, variants })
    }

    pub fn rename_preset(&self, from: &str, to: &str) -> io::Result<()> {
        self.store.rename(from, to)?; // moves the variants directory too
        log_history(self.history().rename(from, to));
        let mut map = self.categories()?;
        if let Some(category) = map.remove(from) {
            map.insert(to.to_string(), category);
            self.write_categories(&map)?;
        }
        let mut meta = self.variant_meta()?;
        if let Some(entry) = meta.remove(from) {
            meta.insert(to.to_string(), entry);
            self.write_variant_meta(&meta)?;
        }
        Ok(())
    }

    // --- Variants (sub-presets): `.variants/<name>/<variant>.txt` + `.variants.json`
    //
    // The invariant: `<name>.txt` always equals the *selected* variant's file.
    // Selecting copies the variant in; saving ([`write_preset`]) writes both.
    // Everything that merely reads a preset is unaware variants exist.

    fn variants_path(&self) -> PathBuf {
        self.store.dir().join(".variants.json")
    }

    fn variant_meta(&self) -> io::Result<BTreeMap<String, VariantMeta>> {
        match fs::read_to_string(self.variants_path()) {
            Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e),
        }
    }

    fn write_variant_meta(&self, map: &BTreeMap<String, VariantMeta>) -> io::Result<()> {
        self.store.ensure_dir()?;
        let text = serde_json::to_string_pretty(map).unwrap_or_else(|_| "{}".to_string());
        writer::write_text_atomic(&self.variants_path(), &text)
    }

    /// Record which variant `name`'s preset file mirrors (`None` forgets the
    /// preset — it has no variants any more).
    fn set_selected(&self, name: &str, variant: Option<&str>) -> io::Result<()> {
        let mut map = self.variant_meta()?;
        match variant {
            Some(v) => {
                map.insert(
                    name.to_string(),
                    VariantMeta {
                        selected: Some(v.to_string()),
                    },
                );
            }
            None if map.remove(name).is_none() => return Ok(()), // nothing to forget
            None => {}
        }
        self.write_variant_meta(&map)
    }

    /// The variant `name`'s preset file mirrors, if the sidecar names one that
    /// still exists.
    pub fn selected_variant(&self, name: &str) -> Option<String> {
        let selected = self.variant_meta().ok()?.remove(name)?.selected?;
        self.store
            .variant_exists(name, &selected)
            .then_some(selected)
    }

    /// Every preset that has variants, with its selection. Presets without a
    /// variants directory (or with an empty one) are absent.
    pub fn variant_sets(&self) -> io::Result<BTreeMap<String, VariantSet>> {
        let entries = match fs::read_dir(self.store.variants_root()) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(e),
        };
        let meta = self.variant_meta()?;
        let mut out = BTreeMap::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(variants) = self.store.list_variants(name) else {
                continue; // an unsafe directory name is not a preset of ours
            };
            if variants.is_empty() {
                continue;
            }
            let selected = meta
                .get(name)
                .and_then(|m| m.selected.clone())
                .filter(|s| variants.iter().any(|v| v == s));
            out.insert(name.to_string(), VariantSet { variants, selected });
        }
        Ok(out)
    }

    /// Add a variant to `name`, copying the preset's current content (the
    /// selected variant), and select it. The first variant also seeds
    /// [`DEFAULT_VARIANT`] from that same content, so the original config
    /// stays available as "Default". Rejects a name already in use (case-
    /// insensitively — the files live on a Windows filesystem).
    pub fn create_variant(&self, name: &str, variant: &str) -> io::Result<()> {
        let current = self.store.load(name)?; // NotFound when the preset is missing
        let variant = variant.trim();
        self.store.variant_path(name, variant)?; // validate before writing anything
        let mut taken = self.store.list_variants(name)?;
        let seeding = taken.is_empty();
        if seeding {
            taken.push(DEFAULT_VARIANT.to_string());
        }
        let wanted = variant.to_lowercase();
        if taken.iter().any(|v| v.to_lowercase() == wanted) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("variant already exists: {variant}"),
            ));
        }
        if seeding {
            self.store.save_variant(name, DEFAULT_VARIANT, &current)?;
        }
        self.store.save_variant(name, variant, &current)?;
        self.set_selected(name, Some(variant))
    }

    /// Make `variant` the one `name`'s preset file mirrors: copy its content
    /// over `<name>.txt`, then record it. Normally no history is recorded —
    /// the content being replaced is the previously selected variant's, which
    /// its own file already holds; content no variant holds is snapshotted
    /// first (see [`snapshot_drift`](Self::snapshot_drift)).
    pub fn select_variant(&self, name: &str, variant: &str) -> io::Result<()> {
        let config = self.store.load_variant(name, variant)?; // NotFound when missing
        self.snapshot_drift(name, &config);
        self.mirror_variant(name, variant, &config)
    }

    /// Copy `config` (`variant`'s content) over `<name>.txt`, then record
    /// `variant` as selected — the sidecar is the last write of every variant
    /// mutation, so it says what the preset file already is.
    fn mirror_variant(&self, name: &str, variant: &str, config: &Config) -> io::Result<()> {
        self.store.save(name, config)?;
        self.set_selected(name, Some(variant.trim()))
    }

    /// The mirror invariant can be broken from outside: `<name>.txt` is plain
    /// text and may be hand-edited, or a save may have landed while the
    /// sidecar named a variant whose file was gone. Before such content is
    /// overwritten with `incoming`, record it as a `save` revision so the
    /// switch cannot silently lose it. In the normal case — the file mirrors
    /// the selected variant — or when it already holds `incoming`, nothing is
    /// recorded.
    fn snapshot_drift(&self, name: &str, incoming: &Config) {
        let Ok(current) = self.store.load(name) else {
            return;
        };
        let normal = history::normalize(&current);
        if normal == history::normalize(incoming) {
            return;
        }
        let mirrored = self
            .selected_variant(name)
            .and_then(|v| self.store.load_variant(name, &v).ok());
        if mirrored.is_some_and(|m| history::normalize(&m) == normal) {
            return;
        }
        log_history(self.history().record(name, &current, RevisionOp::Save));
    }

    /// Delete a variant: its exact file is archived — the handle returned is
    /// what [`restore_variant`](Self::restore_variant) takes, and unlike a
    /// revision id it cannot be invalidated by later saves — and its content
    /// recorded as a `delete` revision of the preset for the history browser.
    /// Deleting the selected variant selects [`DEFAULT_VARIANT`] if present,
    /// else the first remaining; deleting the last one leaves a plain preset
    /// holding its current content. `None` when there was no file to archive.
    pub fn delete_variant(&self, name: &str, variant: &str) -> io::Result<Option<String>> {
        let variant = variant.trim();
        let path = self.store.variant_path(name, variant)?;
        let history = self.history();
        let was_selected = self
            .variant_meta()?
            .get(name)
            .is_some_and(|m| m.selected.as_deref() == Some(variant));
        // The fallback is resolved up front and mirrored directly, not through
        // `select_variant`: once the file is archived the preset file holds
        // content no variant has (the deleted one's), which that path would
        // snapshot again — displacing the delete revision recorded below.
        // Snapshotting any real drift happens here, while the selection still
        // resolves.
        let next = if was_selected {
            let remaining: Vec<String> = self
                .store
                .list_variants(name)?
                .into_iter()
                .filter(|v| v.to_lowercase() != variant.to_lowercase())
                .collect();
            remaining
                .iter()
                .find(|v| *v == DEFAULT_VARIANT)
                .or(remaining.first())
                .map(|v| self.store.load_variant(name, v).map(|c| (c, v.clone())))
                .transpose()?
        } else {
            None
        };
        if let Some((config, _)) = &next {
            self.snapshot_drift(name, config);
        }
        if let Ok(prior) = self.store.load_variant(name, variant) {
            log_history(history.record(name, &prior, RevisionOp::Delete));
        }
        let handle = history.archive_preset(name, &path)?;
        if let Some((config, v)) = next {
            self.mirror_variant(name, &v, &config)?;
        } else if self.store.list_variants(name)?.is_empty() {
            self.set_selected(name, None)?;
            self.store.remove_variants_dir(name);
        }
        Ok(handle)
    }

    /// Undo a variant delete: move the archived file (`handle`, as
    /// [`delete_variant`](Self::delete_variant) returned it) back as
    /// `variant`'s — the exact file, master preamp included. A variant
    /// recreated under that name meanwhile is not clobbered. The `delete`
    /// revision recorded for the content is removed: it is a variant again.
    /// The caller re-selects it if it had been selected.
    pub fn restore_variant(&self, name: &str, variant: &str, handle: &str) -> io::Result<()> {
        let variant = variant.trim();
        if self.store.variant_exists(name, variant) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("variant already exists: {variant}"),
            ));
        }
        let target = self.store.variant_path(name, variant)?;
        let history = self.history();
        history.unarchive_file(name, handle, &target)?;
        if let Ok(restored) = self.store.load_variant(name, variant) {
            log_history(history.remove_matching(name, &restored));
        }
        Ok(())
    }

    /// Rename a variant; the selection follows it.
    pub fn rename_variant(&self, name: &str, from: &str, to: &str) -> io::Result<()> {
        self.store.rename_variant(name, from, to)?;
        let mut map = self.variant_meta()?;
        if let Some(meta) = map.get_mut(name)
            && meta.selected.as_deref() == Some(from)
        {
            meta.selected = Some(to.trim().to_string());
            self.write_variant_meta(&map)?;
        }
        Ok(())
    }

    /// Undo-delete's second half: move a deleted preset's parked variants back
    /// under `name` (see [`DeleteReceipt`]). The caller then re-selects.
    pub fn restore_variants(&self, name: &str, archive: &str) -> io::Result<()> {
        let target = self.store.variants_dir(name)?;
        self.history().unarchive_variants(name, archive, &target)
    }

    // --- History: normalized snapshots of displaced preset content ------------

    fn history(&self) -> PresetHistory {
        PresetHistory::new(self.store.dir())
    }

    /// The revisions of a preset, newest first.
    pub fn preset_history(&self, name: &str) -> io::Result<Vec<Revision>> {
        self.history().list(name)
    }

    /// Revision counts per preset — the preset list's version badges.
    pub fn history_counts(&self) -> io::Result<BTreeMap<String, usize>> {
        self.history().counts()
    }

    /// Set (or clear, with a string that trims to empty) a revision's user tag.
    pub fn set_revision_tag(&self, name: &str, id: &str, tag: &str) -> io::Result<()> {
        self.history().set_tag(name, id, tag)
    }

    /// Remove a revision from visible history while retaining its source file
    /// in the hidden history archive for manual recovery.
    pub fn delete_revision(&self, name: &str, id: &str) -> io::Result<()> {
        self.history().archive(name, id)
    }

    /// One revision, parsed for the history browser's preview ghost.
    pub fn load_revision(&self, name: &str, id: &str) -> io::Result<Config> {
        self.history().load(name, id)
    }

    /// Restore a revision into the preset file. The current content (when it
    /// differs) is snapshotted first as a `restore` revision, so a restore is
    /// itself undoable. Snapshots carry no master preamp, so the restored
    /// preset gets the *recomputed* anti-clip value over its bands — a
    /// hand-set manual preamp is the one thing history doesn't preserve.
    pub fn restore_revision(&self, name: &str, id: &str) -> io::Result<()> {
        let history = self.history();
        let restored = with_recomputed_preamp(history.load(name, id)?);
        if let Ok(current) = self.store.load(name)
            && history::normalize(&current) != history::normalize(&restored)
        {
            log_history(history.record(name, &current, RevisionOp::Restore));
        }
        self.write_preset(name, &restored)?;
        // The restored revision now IS the preset — invariant 2 removes it.
        log_history(history.remove_matching(name, &restored));
        Ok(())
    }

    // --- Categories: sidecar metadata mapping preset name -> device type ---

    fn categories_path(&self) -> PathBuf {
        self.store.dir().join(".categories.json")
    }

    /// The category of every classified preset. Unclassified presets are absent.
    pub fn categories(&self) -> io::Result<BTreeMap<String, Category>> {
        match fs::read_to_string(self.categories_path()) {
            Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e),
        }
    }

    /// Set a preset's category, or clear it with `None`.
    pub fn set_category(&self, name: &str, category: Option<Category>) -> io::Result<()> {
        let mut map = self.categories()?;
        match category {
            Some(c) => {
                map.insert(name.to_string(), c);
            }
            None => {
                map.remove(name);
            }
        }
        self.write_categories(&map)
    }

    fn write_categories(&self, map: &BTreeMap<String, Category>) -> io::Result<()> {
        self.store.ensure_dir()?;
        let text = serde_json::to_string_pretty(map).unwrap_or_else(|_| "{}".to_string());
        writer::write_text_atomic(&self.categories_path(), &text)
    }

    // --- Tone: a global bass/mid/treble overlay layered over the active preset ---

    fn tone_path(&self) -> PathBuf {
        self.store.dir().join(".tone.json")
    }

    /// The persisted tone-knob values (flat if none have been set).
    pub fn tone(&self) -> io::Result<Tone> {
        match fs::read_to_string(self.tone_path()) {
            Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Tone::default()),
            Err(e) => Err(e),
        }
    }

    /// Persist new tone values WITHOUT touching the live config — for callers
    /// that keep the overlay out of `config.txt` (hardware-only offload leaves
    /// APO flat) but still want the knob values to survive for later.
    pub fn save_tone(&self, tone: &Tone) -> io::Result<()> {
        self.store.ensure_dir()?;
        let text = serde_json::to_string_pretty(tone).unwrap_or_else(|_| "{}".to_string());
        writer::write_text_atomic(&self.tone_path(), &text)
    }

    /// Persist new tone values and re-lay the overlay over the current base EQ
    /// (preset or live edit), so changing a knob doesn't disturb the rest.
    pub fn set_tone(&self, tone: &Tone) -> io::Result<()> {
        self.save_tone(tone)?;
        let composed = tone::compose(&self.current_config()?, tone);
        self.write_live(&composed)
    }

    /// The current live `config.txt`, parsed. A missing file is treated as an
    /// empty (passthrough) configuration.
    pub fn current_config(&self) -> io::Result<Config> {
        match fs::read_to_string(self.install.config_file()) {
            Ok(text) => Ok(parse(&text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::new()),
            Err(e) => Err(e),
        }
    }

    /// Save the current live config as a named preset (e.g. for migration). The
    /// global tone overlay is stripped first so it never gets baked into a preset.
    pub fn capture_current(&self, name: &str) -> io::Result<()> {
        self.save_preset(name, &self.base_config()?)
    }

    /// The live config with the tone overlay stripped — the "base EQ" a preset or
    /// live edit produced. Captured before a bypass so un-bypass can restore the
    /// exact prior state, including unsaved edits, not just a named preset.
    pub fn base_config(&self) -> io::Result<Config> {
        Ok(tone::strip(&self.current_config()?))
    }

    /// Apply a stored preset to the live `config.txt`, with the given tone
    /// overlay layered on top. Backs up the existing config once, then writes —
    /// APO live-reloads, no restart. The caller supplies the tone (the shell
    /// caches it) so this never has to read the sidecar.
    pub fn apply_preset(&self, name: &str, tone: &Tone) -> io::Result<()> {
        self.apply_loaded_preset(name, &self.store.load(name)?, tone)
    }

    /// [`apply_preset`](Self::apply_preset) for an already-parsed preset config.
    /// The shell loads the preset once anyway (the offload split needs it), so
    /// this spares a second read of the same file.
    pub fn apply_loaded_preset(&self, name: &str, config: &Config, tone: &Tone) -> io::Result<()> {
        let stamped = provenance::set(config, name);
        self.write_live(&tone::compose(&stamped, tone))
    }

    /// Write the given (base) config to the live `config.txt`, layering the
    /// given tone overlay on top. Called rapidly during a drag, so the tone is
    /// passed in rather than re-read from disk each time.
    ///
    /// Carries the provenance stamp forward — from the config itself if it has
    /// one (e.g. an un-bypass restore), otherwise from the current live config —
    /// so a live edit or Auto Preamp tweak that keeps the EQ equivalent still
    /// resolves to the same active preset, even across a restart.
    pub fn apply_config(&self, config: &Config, tone: &Tone) -> io::Result<()> {
        let carried = match provenance::name(config) {
            Some(name) => Some(name),
            None => provenance::name(&self.current_config()?),
        };
        let stamped = match carried {
            Some(name) => provenance::set(config, &name),
            None => provenance::strip(config),
        };
        self.write_live(&tone::compose(&stamped, tone))
    }

    /// Bypass the EQ: drop every filter but keep the rest of the live config —
    /// the preamp (and any `Device:`/`Include:`/comment lines). Keeping the
    /// preamp means an A/B against the active preset isn't skewed by a level
    /// difference.
    pub fn bypass(&self) -> io::Result<()> {
        // Strip the tone overlay (sentinels and all) and the provenance stamp
        // before dropping filters: a bypassed config has neither the preset's EQ,
        // the tone controls, nor an active preset.
        let current = provenance::strip(&tone::strip(&self.current_config()?));
        let config = Config {
            lines: current
                .lines
                .into_iter()
                .filter(|l| !matches!(l, Line::Filter(_)))
                .collect(),
        };
        self.write_live(&config)
    }

    /// The active preset, if any — identified purely by the live `config.txt`'s
    /// [provenance] stamp: active iff the stamp names a preset that still exists.
    ///
    /// Derived from disk rather than tracked in memory, so it stays correct across
    /// restarts, and O(1) — there is no content scan over the library. The stamp is
    /// written on apply and carried through live edits, so an unsaved edit keeps the
    /// config pinned to its source preset (the editor surfaces "modified" on its
    /// own). A [`bypass`](Self::bypass) strips the stamp, and a stamp naming a
    /// since-deleted preset resolves to `None`; an unstamped or foreign config is
    /// never detected. Hardware offload's software remainder (its offloaded bands
    /// gone from `config.txt`) still resolves, because the stamp — not the EQ
    /// content — is what's checked.
    ///
    /// [provenance]: crate::provenance
    pub fn active_preset(&self) -> io::Result<Option<String>> {
        let current = self.current_config()?;
        match provenance::name(&current) {
            Some(name) if self.store.exists(&name) => Ok(Some(name)),
            _ => Ok(None),
        }
    }

    /// Bulk-import PEACE presets (`*.peace`) from the Equalizer APO config folder,
    /// converting PEACE's INI format to APO filters. Only the folder itself is
    /// scanned (not subfolders like `OLD\`); templates with no EQ are skipped;
    /// existing presets are never overwritten.
    pub fn import_presets_from_config_dir(&self) -> io::Result<ImportReport> {
        let mut report = ImportReport::default();
        self.store.ensure_dir()?;

        let entries = match fs::read_dir(&self.install.config_path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(report),
            Err(e) => return Err(e),
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|s| s.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("peace"))
            })
            .collect();
        files.sort();

        for path in files {
            self.import_one(&path, &mut report)?;
        }

        report.imported.sort();
        report.skipped.sort();
        Ok(report)
    }

    /// Import the given `.peace` files (e.g. chosen via a file picker).
    pub fn import_peace_files(&self, paths: &[PathBuf]) -> io::Result<ImportReport> {
        let mut report = ImportReport::default();
        self.store.ensure_dir()?;
        for path in paths {
            self.import_one(path, &mut report)?;
        }
        report.imported.sort();
        report.skipped.sort();
        Ok(report)
    }

    /// Import a single `.peace` file. Shared by the folder scan and file picker:
    /// skips empties / unusable names, and never overwrites an existing preset.
    fn import_one(&self, path: &Path, report: &mut ImportReport) -> io::Result<()> {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            report.ignored += 1;
            return Ok(());
        };
        let Ok(text) = fs::read_to_string(path) else {
            report.ignored += 1;
            return Ok(());
        };
        let config = crate::peace::from_peace(&text);
        if config.preamp().is_none() && config.filters().next().is_none() {
            report.ignored += 1;
            return Ok(());
        }
        if self.store.path_for(stem).is_err() {
            report.ignored += 1;
            return Ok(());
        }
        if self.store.exists(stem) {
            report.skipped.push(stem.to_string());
            return Ok(());
        }
        self.store.save(stem, &config)?;
        report.imported.push(stem.to_string());
        Ok(())
    }

    fn write_live(&self, config: &Config) -> io::Result<()> {
        let config_file = self.install.config_file();
        writer::backup_once(&config_file, &self.backup_path)?;
        writer::write_config_atomic(&config_file, config)
    }
}

/// A history snapshot made whole again: snapshots carry no master preamp, so
/// a restored config gets the *recomputed* anti-clip value over its bands — a
/// hand-set manual preamp is the one thing history doesn't preserve.
fn with_recomputed_preamp(mut restored: Config) -> Config {
    let preamp = (offload::auto_preamp(&restored, &Tone::default()) * 10.0).round() / 10.0;
    if preamp != 0.0 {
        offload::set_master_preamp(&mut restored, preamp);
    }
    restored
}

/// History is the safety net, not the payload: a failed snapshot must never
/// fail the user's save/delete/rename — log and carry on without it.
fn log_history<T>(result: io::Result<T>) -> Option<T> {
    match result {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("fastpeq: preset history unavailable: {e}");
            None
        }
    }
}
