//! Integration tests for the preset store, atomic writer, and manager flow.

use fastpeq_core::apo::env::ApoInstall;
use fastpeq_core::{
    Channel, Config, Filter, FilterKind, Line, Manager, PresetStore, Tone, VariantSet, parse,
    provenance, serialize,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A throwaway directory under the OS temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "fastpeq-test-{tag}-{}-{n}-{nanos}",
            std::process::id()
        ));
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

fn sample_config() -> Config {
    Config {
        lines: vec![
            Line::Preamp {
                gain: -3.0,
                channel: Channel::Both,
            },
            Line::Filter(Filter::peak(1000.0, -2.0, 1.0)),
        ],
    }
}

#[test]
fn store_save_load_list_delete() {
    let tmp = TempDir::new("store");
    let store = PresetStore::new(tmp.path());

    assert!(store.list().unwrap().is_empty());

    store.save("Bass Boost", &sample_config()).unwrap();
    store.save("Vocal", &sample_config()).unwrap();

    // Sorted case-insensitively.
    assert_eq!(store.list().unwrap(), vec!["Bass Boost", "Vocal"]);
    assert!(store.exists("Vocal"));

    // Loads back to an identical model.
    assert_eq!(store.load("Bass Boost").unwrap(), sample_config());

    store.delete("Bass Boost").unwrap();
    assert_eq!(store.list().unwrap(), vec!["Vocal"]);
    // Deleting a missing preset is a no-op.
    store.delete("Bass Boost").unwrap();
}

#[test]
fn save_tone_persists_without_touching_live_config() {
    let apo_dir = TempDir::new("savetone-apo");
    let presets_dir = TempDir::new("savetone-presets");
    let config_file = apo_dir.path().join("config.txt");
    let install = ApoInstall {
        config_path: apo_dir.path().to_path_buf(),
    };
    let manager = Manager::new(
        install,
        PresetStore::new(presets_dir.path()),
        apo_dir.path().join("backup.txt"),
    );

    manager.save_preset("HP", &sample_config()).unwrap();
    manager.apply_preset("HP", &Tone::default()).unwrap();
    let before = fs::read_to_string(&config_file).unwrap();

    // save_tone (used while hardware-only offload keeps APO flat) records the
    // knobs for later but must not lay the overlay into the live config.
    let tone = Tone {
        bass: 6.0,
        ..Tone::default()
    };
    manager.save_tone(&tone).unwrap();
    assert_eq!(fs::read_to_string(&config_file).unwrap(), before);
    assert_eq!(manager.tone().unwrap(), tone); // ...but the sidecar has it

    // set_tone does both: persists and re-lays the overlay.
    manager.set_tone(&tone).unwrap();
    let after = fs::read_to_string(&config_file).unwrap();
    assert!(after.contains("fastpeq tone overlay"), "{after}");
}

#[test]
fn bypass_keeps_preamp_but_drops_filters() {
    let apo_dir = TempDir::new("bypass-apo");
    let presets_dir = TempDir::new("bypass-presets");
    let config_file = apo_dir.path().join("config.txt");
    let install = ApoInstall {
        config_path: apo_dir.path().to_path_buf(),
    };
    let manager = Manager::new(
        install,
        PresetStore::new(presets_dir.path()),
        apo_dir.path().join("backup.txt"),
    );

    manager.save_preset("HP", &sample_config()).unwrap(); // Preamp -3 + one filter
    manager.apply_preset("HP", &Tone::default()).unwrap();
    manager.bypass().unwrap();

    let after = fs::read_to_string(&config_file).unwrap();
    assert!(after.contains("Preamp: -3 dB"), "{after}");
    assert!(!after.contains("Filter"), "{after}");
    // No filters means it no longer matches the HP preset.
    assert_eq!(manager.active_preset().unwrap(), None);
}

#[test]
fn captured_base_restores_an_unsaved_edit_after_bypass() {
    let apo_dir = TempDir::new("restore-apo");
    let presets_dir = TempDir::new("restore-presets");
    let install = ApoInstall {
        config_path: apo_dir.path().to_path_buf(),
    };
    let manager = Manager::new(
        install,
        PresetStore::new(presets_dir.path()),
        apo_dir.path().join("backup.txt"),
    );

    // A live edit that was never saved as a preset, so a name-based restore would
    // lose it; capturing the base config must bring it back exactly.
    let edited = Config {
        lines: vec![
            Line::Preamp {
                gain: -5.0,
                channel: Channel::Both,
            },
            Line::Filter(Filter::peak(500.0, 3.0, 1.5)),
        ],
    };
    manager.apply_config(&edited, &Tone::default()).unwrap();

    let base = manager.base_config().unwrap(); // captured before bypass
    manager.bypass().unwrap();
    assert!(manager.current_config().unwrap().filters().next().is_none());

    manager.apply_config(&base, &Tone::default()).unwrap(); // un-bypass restore
    assert_eq!(manager.base_config().unwrap(), edited);
}

#[test]
fn store_rename_moves_and_guards() {
    let tmp = TempDir::new("rename");
    let store = PresetStore::new(tmp.path());
    store.save("Old Name", &sample_config()).unwrap();
    store.save("Other", &Config::new()).unwrap();

    store.rename("Old Name", "New Name").unwrap();
    assert_eq!(store.list().unwrap(), vec!["New Name", "Other"]);
    assert_eq!(store.load("New Name").unwrap(), sample_config());
    assert!(!store.exists("Old Name"));

    // Renaming a missing preset fails.
    assert!(store.rename("Ghost", "Whatever").is_err());
    // Renaming onto an existing name fails (never clobbers).
    assert!(store.rename("New Name", "Other").is_err());
}

#[test]
fn store_rename_allows_case_only_change() {
    let tmp = TempDir::new("rename-case");
    let store = PresetStore::new(tmp.path());
    store.save("HD600", &sample_config()).unwrap();

    // Fixing only the capitalisation is the same preset, not a clobber — allowed.
    store.rename("HD600", "hd600").unwrap();
    assert!(store.load("hd600").is_ok());
    assert_eq!(store.list().unwrap().len(), 1); // still one preset, not a duplicate
}

#[test]
fn manager_active_preset_tracks_live_config() {
    let apo_dir = TempDir::new("active-apo");
    let presets_dir = TempDir::new("active-presets");
    let install = ApoInstall {
        config_path: apo_dir.path().to_path_buf(),
    };
    let backup = apo_dir.path().join("config.backup.txt");
    let manager = Manager::new(install, PresetStore::new(presets_dir.path()), backup);

    manager.save_preset("Bass", &sample_config()).unwrap();
    let mut other = sample_config();
    other.lines.push(Line::Filter(Filter::peak(60.0, 4.0, 0.7)));
    manager.save_preset("Other", &other).unwrap();

    // Nothing applied yet => bypass.
    assert_eq!(manager.active_preset().unwrap(), None);

    manager.apply_preset("Bass", &Tone::default()).unwrap();
    assert_eq!(manager.active_preset().unwrap(), Some("Bass".to_string()));

    // Switching is reflected immediately.
    manager.apply_preset("Other", &Tone::default()).unwrap();
    assert_eq!(manager.active_preset().unwrap(), Some("Other".to_string()));

    // Bypass clears it.
    manager.bypass().unwrap();
    assert_eq!(manager.active_preset().unwrap(), None);
}

#[test]
fn active_preset_resolves_an_offload_remainder() {
    // Reproduces the hardware-offload restart case: the live config holds only the
    // software *remainder* (offloaded bands removed) plus the provenance stamp. The
    // full preset's EQ isn't in `config.txt`, but the stamp still resolves it — the
    // stamp, not the content, is what detection checks.
    let apo_dir = TempDir::new("stamp-apo");
    let presets_dir = TempDir::new("stamp-presets");
    let install = ApoInstall {
        config_path: apo_dir.path().to_path_buf(),
    };
    let backup = apo_dir.path().join("config.backup.txt");
    let manager = Manager::new(install, PresetStore::new(presets_dir.path()), backup);

    // A preset with several bands.
    let mut preset = sample_config();
    preset
        .lines
        .push(Line::Filter(Filter::peak(60.0, 4.0, 0.7)));
    preset
        .lines
        .push(Line::Filter(Filter::peak(8000.0, 3.0, 1.0)));
    manager.save_preset("HD600", &preset).unwrap();

    // Write a stamped remainder straight to the live config (what's on disk after
    // offloading two bands to hardware and restarting).
    let remainder = provenance::set(
        &Config {
            lines: vec![Line::Filter(Filter::peak(8000.0, 3.0, 1.0))],
        },
        "HD600",
    );
    fs::write(manager.install().config_file(), serialize(&remainder)).unwrap();

    // The stamp resolves the remainder to its source preset.
    assert_eq!(manager.active_preset().unwrap(), Some("HD600".to_string()));

    // A stamp pointing at a deleted preset resolves to nothing.
    manager.delete_preset("HD600").unwrap();
    assert_eq!(manager.active_preset().unwrap(), None);
}

#[test]
fn provenance_stamp_disambiguates_equivalent_presets() {
    let apo_dir = TempDir::new("prov-apo");
    let presets_dir = TempDir::new("prov-presets");
    let install = ApoInstall {
        config_path: apo_dir.path().to_path_buf(),
    };
    let backup = apo_dir.path().join("config.backup.txt");
    let manager = Manager::new(install, PresetStore::new(presets_dir.path()), backup);

    // Two presets with identical filters that differ ONLY by master preamp, so
    // they are equivalent to each other — and to any auto-preamped live config.
    let band = Filter::peak(1000.0, 3.0, 1.0);
    let soft = Config {
        lines: vec![
            Line::Preamp {
                gain: -6.0,
                channel: Channel::Both,
            },
            Line::Filter(band.clone()),
        ],
    };
    let loud = Config {
        lines: vec![
            Line::Preamp {
                gain: -1.0,
                channel: Channel::Both,
            },
            Line::Filter(band),
        ],
    };
    // Identical filters — genuinely ambiguous by content alone (they differ
    // only in the master preamp, which Auto Preamp rewrites anyway).
    assert!(soft.filters().eq(loud.filters()));
    manager.save_preset("Loud", &loud).unwrap(); // sorts before "Soft"
    manager.save_preset("Soft", &soft).unwrap();

    // Apply Soft, then let Auto Preamp rewrite the master gain so it matches
    // NEITHER preset exactly. The stamp keeps it pinned to the applied preset.
    manager.apply_preset("Soft", &Tone::default()).unwrap();
    let mut retuned = soft.clone();
    for line in &mut retuned.lines {
        if let Line::Preamp {
            gain,
            channel: Channel::Both,
        } = line
        {
            *gain = -9.5;
        }
    }
    manager.apply_config(&retuned, &Tone::default()).unwrap();
    assert_eq!(manager.active_preset().unwrap(), Some("Soft".to_string()));

    // Contrast: the same EQ written *without* a stamp (as another tool would
    // leave it) is not detected at all. Provenance is required — we no longer
    // guess by content, so the "Loud" vs "Soft" ambiguity simply isn't entered.
    std::fs::write(manager.install().config_file(), serialize(&retuned)).unwrap();
    assert_eq!(manager.active_preset().unwrap(), None);
}

#[test]
fn saved_presets_never_carry_the_provenance_stamp() {
    let apo_dir = TempDir::new("prov-save-apo");
    let presets_dir = TempDir::new("prov-save-presets");
    let install = ApoInstall {
        config_path: apo_dir.path().to_path_buf(),
    };
    let backup = apo_dir.path().join("config.backup.txt");
    let manager = Manager::new(install, PresetStore::new(presets_dir.path()), backup);

    manager.save_preset("Bass", &sample_config()).unwrap();
    manager.apply_preset("Bass", &Tone::default()).unwrap();
    // The live config IS stamped...
    assert_eq!(
        provenance::name(&manager.current_config().unwrap()).as_deref(),
        Some("Bass")
    );

    // ...but capturing it back into a preset must strip the stamp, even though
    // the captured EQ stays equivalent to its source.
    manager.capture_current("Copy").unwrap();
    let copy = manager.load_preset("Copy").unwrap();
    assert_eq!(provenance::name(&copy), None);
    // With the stamp gone (and the tone flat), the capture round-trip is
    // lossless — the copy equals its source exactly.
    assert_eq!(copy, sample_config());
    // Belt-and-suspenders: no marker comment survives in the file text.
    let text = std::fs::read_to_string(presets_dir.path().join("Copy.txt")).unwrap();
    assert!(!text.contains("fastpeq:preset"), "{text}");
}

#[test]
fn stale_stamp_for_deleted_preset_is_not_active() {
    let apo_dir = TempDir::new("prov-stale-apo");
    let presets_dir = TempDir::new("prov-stale-presets");
    let install = ApoInstall {
        config_path: apo_dir.path().to_path_buf(),
    };
    let backup = apo_dir.path().join("config.backup.txt");
    let manager = Manager::new(install, PresetStore::new(presets_dir.path()), backup);

    manager.save_preset("Bass", &sample_config()).unwrap();
    manager.apply_preset("Bass", &Tone::default()).unwrap();

    // Delete the stamped preset: the live config still names "Bass", but its
    // file is gone. The dangling stamp must resolve to nothing (and not panic on
    // the missing-file load path).
    manager.delete_preset("Bass").unwrap();
    assert_eq!(manager.active_preset().unwrap(), None);

    // Even an equivalent preset saved under a new name stays undetected: there
    // is no content scan, so only a matching *stamp* makes a preset active.
    manager.save_preset("Rescue", &sample_config()).unwrap();
    assert_eq!(manager.active_preset().unwrap(), None);
}

#[test]
fn categories_set_get_and_track_rename_delete() {
    let apo_dir = TempDir::new("cat-apo");
    let presets_dir = TempDir::new("cat-presets");
    let install = ApoInstall {
        config_path: apo_dir.path().to_path_buf(),
    };
    let manager = Manager::new(
        install,
        PresetStore::new(presets_dir.path()),
        apo_dir.path().join("backup.txt"),
    );

    manager.save_preset("HD600", &sample_config()).unwrap();
    manager.save_preset("KEF", &sample_config()).unwrap();
    assert!(manager.categories().unwrap().is_empty());

    manager
        .set_category("HD600", Some("headphone".to_string()))
        .unwrap();
    manager
        .set_category("KEF", Some("speaker".to_string()))
        .unwrap();
    let cats = manager.categories().unwrap();
    assert_eq!(cats.get("HD600"), Some(&"headphone".to_string()));
    assert_eq!(cats.get("KEF"), Some(&"speaker".to_string()));

    // Rename carries the category across.
    manager.rename_preset("HD600", "Sennheiser HD600").unwrap();
    let cats = manager.categories().unwrap();
    assert_eq!(cats.get("Sennheiser HD600"), Some(&"headphone".to_string()));
    assert!(!cats.contains_key("HD600"));

    // Delete drops it.
    manager.delete_preset("KEF").unwrap();
    assert!(!manager.categories().unwrap().contains_key("KEF"));

    // Clearing with None removes it.
    manager.set_category("Sennheiser HD600", None).unwrap();
    assert!(manager.categories().unwrap().is_empty());
}

#[test]
fn imports_peace_presets_from_config_dir() {
    let apo_dir = TempDir::new("peace-apo");
    let presets_dir = TempDir::new("peace-presets");

    // A real PEACE preset (sparse gains: only band 1 is active).
    fs::write(
        apo_dir.path().join("HD600.peace"),
        "[General]\nPreAmp=-3\n[Frequencies]\nFrequency1=100\nFrequency2=1000\n\
         [Gains]\nGain1=4\n[Qualities]\nQuality1=1.5\nQuality2=2\n\
         [Speakers]\nSpeakerId0=0\nSpeakerTargets0=all\n",
    )
    .unwrap();
    // A template with no gains -> ignored.
    fs::write(
        apo_dir.path().join("BASE.peace"),
        "[Frequencies]\nFrequency1=100\n[Qualities]\nQuality1=1\n",
    )
    .unwrap();
    // A preset in a subfolder must NOT be imported (top-level only).
    fs::create_dir_all(apo_dir.path().join("OLD")).unwrap();
    fs::write(
        apo_dir.path().join("OLD").join("KEF.peace"),
        "[Frequencies]\nFrequency1=60\n[Gains]\nGain1=3\n[Qualities]\nQuality1=0.7\n",
    )
    .unwrap();
    // Non-.peace files are ignored entirely.
    fs::write(
        apo_dir.path().join("config.txt"),
        "Filter: ON PK Fc 50 Hz Gain 2 dB Q 1\n",
    )
    .unwrap();

    let install = ApoInstall {
        config_path: apo_dir.path().to_path_buf(),
    };
    let manager = Manager::new(
        install,
        PresetStore::new(presets_dir.path()),
        apo_dir.path().join("backup.txt"),
    );

    let report = manager.import_presets_from_config_dir().unwrap();
    assert_eq!(report.imported, vec!["HD600"]); // top-level only
    assert!(report.skipped.is_empty());
    assert_eq!(report.ignored, 1); // BASE has no EQ
    assert!(!manager.store().exists("KEF")); // OLD/ subfolder not scanned

    let cfg = manager.load_preset("HD600").unwrap();
    assert_eq!(cfg.preamp(), Some(-3.0));
    let filters: Vec<_> = cfg.filters().collect();
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].freq, 100.0);
    assert_eq!(filters[0].gain, Some(4.0));
    assert_eq!(filters[0].q, Some(1.5));
    assert_eq!(filters[0].kind, FilterKind::Peak);
    assert_eq!(filters[0].channel, Channel::Both);
}

#[test]
fn store_rejects_unsafe_names() {
    let tmp = TempDir::new("unsafe");
    let store = PresetStore::new(tmp.path());

    assert!(store.path_for("../evil").is_err());
    assert!(store.path_for("sub/dir").is_err());
    assert!(store.path_for("a:b").is_err());
    assert!(store.path_for("").is_err());
    assert!(store.path_for("  ").is_err());
    // Windows reserved device names (with or without an extension).
    assert!(store.path_for("CON").is_err());
    assert!(store.path_for("nul").is_err());
    assert!(store.path_for("COM1").is_err());
    assert!(store.path_for("LPT9.txt").is_err());
    assert!(store.path_for("CONIN$").is_err());
    assert!(store.path_for("conout$").is_err());
    assert!(store.path_for("COM¹").is_err()); // superscript digits count too
    // Trailing dot / space (Windows strips these) and control chars.
    assert!(store.path_for("name.").is_err());
    assert!(store.path_for("name\u{7}").is_err());
    // Not reserved: COM0, and names that merely start with a device prefix.
    assert!(store.path_for("COM0").is_ok());
    assert!(store.path_for("Console").is_ok());
    assert!(store.path_for("Good Name 01").is_ok());
}

#[test]
fn atomic_write_replaces_and_leaves_no_temp() {
    let tmp = TempDir::new("atomic");
    let target = tmp.path().join("config.txt");
    fs::write(&target, "stale contents").unwrap();

    fastpeq_core::apo::write_config_atomic(&target, &sample_config()).unwrap();

    let written = fs::read_to_string(&target).unwrap();
    assert!(written.contains("Preamp: -3 dB"));
    assert!(written.contains("Filter: ON PK Fc 1000 Hz Gain -2 dB Q 1"));
    assert!(written.ends_with('\n'));

    let leftovers: Vec<_> = fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("fastpeq-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "temp files left behind: {leftovers:?}"
    );
}

#[test]
fn manager_apply_backs_up_once_then_switches() {
    let apo_dir = TempDir::new("apo");
    let presets_dir = TempDir::new("presets");
    let config_file = apo_dir.path().join("config.txt");
    fs::write(&config_file, "# user's original config\nPreamp: 0 dB\n").unwrap();

    let install = ApoInstall {
        config_path: apo_dir.path().to_path_buf(),
    };
    let backup = apo_dir.path().join("config.txt.fastpeq-backup");
    let manager = Manager::new(
        install,
        PresetStore::new(presets_dir.path()),
        backup.clone(),
    );

    manager.save_preset("Flat", &Config::new()).unwrap();
    manager.save_preset("BassBoost", &sample_config()).unwrap();

    // Capture the live config before touching it (migration path).
    manager.capture_current("Original").unwrap();
    assert_eq!(manager.load_preset("Original").unwrap().preamp(), Some(0.0));

    // Apply a preset: backup created, config.txt replaced live.
    manager.apply_preset("BassBoost", &Tone::default()).unwrap();
    assert!(backup.exists(), "backup should be created on first write");
    assert!(
        fs::read_to_string(&config_file)
            .unwrap()
            .contains("Preamp: -3 dB")
    );

    let backup_text = fs::read_to_string(&backup).unwrap();
    assert!(backup_text.contains("user's original config"));

    // A second switch must not clobber the original backup.
    manager.apply_preset("Flat", &Tone::default()).unwrap();
    assert_eq!(fs::read_to_string(&backup).unwrap(), backup_text);

    // Bypass writes a passthrough (effectively empty) config.
    manager.bypass().unwrap();
    assert!(fs::read_to_string(&config_file).unwrap().trim().is_empty());
}

// --- Preset history: normalized snapshots + undo flows ----------------------

/// A manager over throwaway dirs, for the history flows.
fn history_manager(tag: &str) -> (TempDir, TempDir, Manager) {
    let apo_dir = TempDir::new(&format!("{tag}-apo"));
    let presets_dir = TempDir::new(&format!("{tag}-presets"));
    let install = ApoInstall {
        config_path: apo_dir.path().to_path_buf(),
    };
    let backup = apo_dir.path().join("config.backup.txt");
    let manager = Manager::new(install, PresetStore::new(presets_dir.path()), backup);
    (apo_dir, presets_dir, manager)
}

#[test]
fn save_records_the_displaced_content_and_delete_then_restore_round_trips() {
    let (_apo, presets, manager) = history_manager("hist-roundtrip");

    // v1 carries a raw line, a balance trim, and a master preamp.
    let v1 = Config {
        lines: vec![
            Line::Raw("# tuned by ear".into()),
            Line::Preamp {
                gain: -6.0,
                channel: Channel::Both,
            },
            Line::Preamp {
                gain: -1.5,
                channel: Channel::Left,
            },
            Line::Filter(Filter::peak(1000.0, 4.0, 1.0)),
        ],
    };
    let v2 = Config {
        lines: vec![Line::Filter(Filter::peak(2000.0, 5.0, 2.0))],
    };

    manager.save_preset("HD600", &v1).unwrap();
    assert!(
        manager.preset_history("HD600").unwrap().is_empty(),
        "a first save displaces nothing"
    );

    manager.save_preset("HD600", &v2).unwrap();
    let after_save = manager.preset_history("HD600").unwrap();
    assert_eq!(after_save.len(), 1);
    assert_eq!(after_save[0].op, fastpeq_core::RevisionOp::Save);

    // The snapshot is v1 normalized: raw line + trim + band kept verbatim,
    // master preamp gone.
    let snap = manager.load_revision("HD600", &after_save[0].id).unwrap();
    assert_eq!(
        snap.lines,
        vec![
            Line::Raw("# tuned by ear".into()),
            Line::Preamp {
                gain: -1.5,
                channel: Channel::Left,
            },
            Line::Filter(Filter::peak(1000.0, 4.0, 1.0)),
        ]
    );

    // Delete records the undo handle and moves the exact source preset into
    // the hidden archive. Restoring brings the file back with a RECOMPUTED
    // anti-clip preamp (≈ −5 dB for v2's +5 dB band — snapshots carry no
    // preamp of their own).
    let source_text = fs::read_to_string(presets.path().join("HD600.txt")).unwrap();
    let undo = manager
        .delete_preset("HD600")
        .unwrap()
        .revision
        .expect("undo handle");
    assert!(!presets.path().join("HD600.txt").exists());
    let archived_preset = presets
        .path()
        .join(".history")
        .join(".deleted")
        .join("HD600")
        .join("HD600.txt");
    assert_eq!(fs::read_to_string(&archived_preset).unwrap(), source_text);

    manager.restore_revision("HD600", &undo).unwrap();
    let back = manager.load_preset("HD600").unwrap();
    let preamp = back.preamp().expect("recomputed anti-clip preamp");
    assert!(
        (-5.6..=-4.4).contains(&preamp),
        "≈ −5 dB for a +5 dB peak, got {preamp}"
    );
    // Content (normalized view) equals the deleted v2.
    assert_eq!(
        fastpeq_core::history::normalize(&back),
        fastpeq_core::history::normalize(&v2)
    );
    // ...and the consumed delete revision is gone (invariant 2), while the
    // pre-delete timeline (v1's save snapshot) survives.
    let ids: Vec<_> = manager.preset_history("HD600").unwrap();
    assert!(ids.iter().all(|r| r.id != undo), "{ids:?}");
}

#[test]
fn restore_then_save_leaves_no_duplicate_revision() {
    let (_apo, _presets, manager) = history_manager("hist-unique");
    let v1 = Config {
        lines: vec![Line::Filter(Filter::peak(1000.0, 3.0, 1.0))],
    };
    let v2 = Config {
        lines: vec![Line::Filter(Filter::peak(2000.0, -2.0, 1.0))],
    };
    manager.save_preset("P", &v1).unwrap();
    manager.save_preset("P", &v2).unwrap(); // history: [v1]

    let v1_rev = manager.preset_history("P").unwrap()[0].id.clone();
    manager.restore_revision("P", &v1_rev).unwrap();
    // The restore snapshotted v2 and consumed v1's revision.
    let ops: Vec<_> = manager
        .preset_history("P")
        .unwrap()
        .into_iter()
        .map(|r| r.op)
        .collect();
    assert_eq!(ops, vec![fastpeq_core::RevisionOp::Restore]);

    // Saving the (unchanged) restored content records nothing new and the
    // uniqueness property holds: no revision equals the live preset, no two
    // revisions are equal.
    manager
        .save_preset("P", &manager.load_preset("P").unwrap())
        .unwrap();
    assert_unique_history(&manager, "P");
    assert_eq!(manager.preset_history("P").unwrap().len(), 1);

    // A save that only changes the preamp / adds a 0 dB band is a no-op to
    // history too.
    let mut cosmetic = manager.load_preset("P").unwrap();
    cosmetic
        .lines
        .push(Line::Filter(Filter::peak(500.0, 0.0, 1.0)));
    manager.save_preset("P", &cosmetic).unwrap();
    assert_eq!(manager.preset_history("P").unwrap().len(), 1);
}

/// After any flow: no two revisions share content, and none equals the live
/// preset (both compared normalized).
fn assert_unique_history(manager: &Manager, name: &str) {
    let live = fastpeq_core::history::normalize(&manager.load_preset(name).unwrap());
    let revs = manager.preset_history(name).unwrap();
    let mut contents: Vec<String> = Vec::new();
    for r in &revs {
        let c = serialize(&fastpeq_core::history::normalize(
            &manager.load_revision(name, &r.id).unwrap(),
        ));
        assert_ne!(
            c,
            serialize(&live),
            "revision {} duplicates the live preset",
            r.id
        );
        assert!(!contents.contains(&c), "duplicate revisions: {revs:?}");
        contents.push(c);
    }
}

#[test]
fn history_recording_failure_is_non_fatal_to_saves() {
    let (_apo, presets, manager) = history_manager("hist-nonfatal");
    // Occupy the .history path with a FILE so the history dir can't exist.
    fs::write(presets.path().join(".history"), "in the way").unwrap();

    let v1 = Config {
        lines: vec![Line::Filter(Filter::peak(1000.0, 3.0, 1.0))],
    };
    let v2 = Config {
        lines: vec![Line::Filter(Filter::peak(2000.0, -2.0, 1.0))],
    };
    manager.save_preset("P", &v1).unwrap();
    manager.save_preset("P", &v2).unwrap(); // snapshot fails, save succeeds
    assert_eq!(manager.load_preset("P").unwrap(), v2);

    // Archiving is now the deletion itself, so a blocked archive must fail
    // safely and leave the live preset untouched.
    assert!(manager.delete_preset("P").is_err());
    assert!(presets.path().join("P.txt").exists());
}

#[test]
fn repeated_preset_deletes_keep_every_archived_source_file() {
    let (_apo, presets, manager) = history_manager("hist-delete-collide");
    let first = Config {
        lines: vec![Line::Filter(Filter::peak(1000.0, 3.0, 1.0))],
    };
    let second = Config {
        lines: vec![Line::Filter(Filter::peak(2000.0, -2.0, 1.0))],
    };

    manager.save_preset("P", &first).unwrap();
    manager.delete_preset("P").unwrap();
    manager.save_preset("P", &second).unwrap();
    manager.delete_preset("P").unwrap();

    let archive = presets.path().join(".history").join(".deleted").join("P");
    assert_eq!(
        parse(&fs::read_to_string(archive.join("P.txt")).unwrap()),
        first
    );
    assert_eq!(
        parse(&fs::read_to_string(archive.join("P-1.txt")).unwrap()),
        second
    );
}

#[test]
fn rename_carries_history_along() {
    let (_apo, _presets, manager) = history_manager("hist-rename");
    let v1 = Config {
        lines: vec![Line::Filter(Filter::peak(1000.0, 3.0, 1.0))],
    };
    let v2 = Config {
        lines: vec![Line::Filter(Filter::peak(2000.0, -2.0, 1.0))],
    };
    manager.save_preset("Old", &v1).unwrap();
    manager.save_preset("Old", &v2).unwrap();

    manager.rename_preset("Old", "New").unwrap();
    assert!(manager.preset_history("Old").unwrap().is_empty());
    let moved = manager.preset_history("New").unwrap();
    assert_eq!(moved.len(), 1);
    // The moved revision still loads.
    manager.load_revision("New", &moved[0].id).unwrap();
}

#[test]
fn a_save_keeps_the_displaced_contents_tag_on_its_snapshot() {
    let (_apo, _presets, manager) = history_manager("hist-tag");

    // The preset file holds tagged content (the state after restoring a
    // tagged version and saving it unchanged: the tag rides with the content).
    let tagged = Config {
        lines: vec![
            Line::Raw("# fastpeq:tag=Warm".into()),
            Line::Filter(Filter::peak(1000.0, 3.0, 1.0)),
        ],
    };
    manager.save_preset("P", &tagged).unwrap();

    // Saving a real change displaces it; the snapshot keeps the tag, the new
    // file doesn't have one.
    let changed = Config {
        lines: vec![Line::Filter(Filter::peak(2000.0, -2.0, 1.0))],
    };
    manager.save_preset("P", &changed).unwrap();

    let revs = manager.preset_history("P").unwrap();
    assert_eq!(revs.len(), 1);
    assert_eq!(revs[0].tag.as_deref(), Some("Warm"));
    assert_eq!(
        fastpeq_core::history::tag_of(&manager.load_preset("P").unwrap()),
        None
    );

    // A tag-only difference is not a content change: re-saving the same EQ
    // without its tag records nothing new.
    let untagged_same = Config {
        lines: vec![Line::Filter(Filter::peak(2000.0, -2.0, 1.0))],
    };
    manager.save_preset("P", &untagged_same).unwrap();
    assert_eq!(manager.preset_history("P").unwrap().len(), 1);
}

// --- Variants (sub-presets): the mirror model ------------------------------
//
// `<name>.txt` always equals the selected variant's file; selecting copies a
// variant in, saving writes both. See `docs/PERSISTENCE.md`.

fn band(freq: f64, gain: f64) -> Config {
    Config {
        lines: vec![Line::Filter(Filter::peak(freq, gain, 1.0))],
    }
}

fn set(variants: &[&str], selected: &str) -> VariantSet {
    VariantSet {
        variants: variants.iter().map(|v| v.to_string()).collect(),
        selected: Some(selected.to_string()),
    }
}

#[test]
fn create_variant_seeds_default_copies_current_and_selects() {
    let (_apo, presets, manager) = history_manager("var-create");
    let stock = band(1000.0, 3.0);
    manager.save_preset("HD600", &stock).unwrap();
    assert!(manager.variant_sets().unwrap().is_empty());

    manager.create_variant("HD600", "Dekoni").unwrap();

    // The seeded Default and the new variant both hold the current content...
    let dir = presets.path().join(".variants").join("HD600");
    assert_eq!(
        parse(&fs::read_to_string(dir.join("Default.txt")).unwrap()),
        stock
    );
    assert_eq!(
        parse(&fs::read_to_string(dir.join("Dekoni.txt")).unwrap()),
        stock
    );
    // ...the preset file is untouched, and the new variant is the selection.
    assert_eq!(manager.load_preset("HD600").unwrap(), stock);
    assert_eq!(
        manager.variant_sets().unwrap()["HD600"],
        set(&["Default", "Dekoni"], "Dekoni")
    );
    assert_eq!(manager.selected_variant("HD600").as_deref(), Some("Dekoni"));
    // Variants never surface as presets.
    assert_eq!(manager.list_presets().unwrap(), vec!["HD600"]);

    // A second variant copies the selected content without re-seeding.
    manager.create_variant("HD600", "Brainwavz").unwrap();
    assert_eq!(
        manager.variant_sets().unwrap()["HD600"],
        set(&["Brainwavz", "Default", "Dekoni"], "Brainwavz")
    );
}

#[test]
fn select_variant_mirrors_into_the_preset_file_without_history() {
    let (_apo, _presets, manager) = history_manager("var-select");
    let stock = band(1000.0, 3.0);
    let pads = band(2000.0, -2.0);
    manager.save_preset("HD600", &stock).unwrap();
    manager.create_variant("HD600", "Dekoni").unwrap();
    manager.save_preset("HD600", &pads).unwrap(); // edits Dekoni; displaces stock

    manager.select_variant("HD600", "Default").unwrap();
    assert_eq!(manager.load_preset("HD600").unwrap(), stock);
    assert_eq!(
        manager.selected_variant("HD600").as_deref(),
        Some("Default")
    );
    // Dekoni kept its edit (the save mirrored it), and switching back finds it.
    manager.select_variant("HD600", "Dekoni").unwrap();
    assert_eq!(manager.load_preset("HD600").unwrap(), pads);
    // Only the save recorded history; neither switch did.
    assert_eq!(manager.preset_history("HD600").unwrap().len(), 1);

    // Selecting a variant that does not exist is an error and changes nothing.
    assert_eq!(
        manager.select_variant("HD600", "Nope").unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    assert_eq!(manager.selected_variant("HD600").as_deref(), Some("Dekoni"));
}

#[test]
fn save_and_restore_mirror_into_the_selected_variant() {
    let (_apo, _presets, manager) = history_manager("var-mirror");
    let v1 = band(1000.0, 3.0);
    let v2 = band(2000.0, 5.0);
    manager.save_preset("HD600", &v1).unwrap();
    manager.create_variant("HD600", "Dekoni").unwrap();

    manager.save_preset("HD600", &v2).unwrap();
    assert_eq!(manager.store().load_variant("HD600", "Dekoni").unwrap(), v2);
    assert_eq!(
        manager.store().load_variant("HD600", "Default").unwrap(),
        v1
    );

    // Restoring a revision writes the mirror too.
    let rev = manager.preset_history("HD600").unwrap()[0].id.clone();
    manager.restore_revision("HD600", &rev).unwrap();
    let restored = manager.load_preset("HD600").unwrap();
    assert_eq!(
        fastpeq_core::history::normalize(&restored),
        fastpeq_core::history::normalize(&v1)
    );
    assert_eq!(
        manager.store().load_variant("HD600", "Dekoni").unwrap(),
        restored
    );
}

#[test]
fn create_variant_rejects_duplicates_and_unsafe_names() {
    use std::io::ErrorKind;
    let (_apo, _presets, manager) = history_manager("var-reject");
    assert_eq!(
        manager.create_variant("Missing", "X").unwrap_err().kind(),
        ErrorKind::NotFound
    );
    manager.save_preset("HD600", &band(1000.0, 3.0)).unwrap();
    // The first variant seeds "Default", so that name is taken from the start.
    assert_eq!(
        manager
            .create_variant("HD600", "default")
            .unwrap_err()
            .kind(),
        ErrorKind::AlreadyExists
    );
    assert!(
        manager.variant_sets().unwrap().is_empty(),
        "a rejected create writes nothing"
    );
    manager.create_variant("HD600", "Dekoni").unwrap();
    assert_eq!(
        manager
            .create_variant("HD600", "DEKONI")
            .unwrap_err()
            .kind(),
        ErrorKind::AlreadyExists
    );
    assert_eq!(
        manager
            .create_variant("HD600", "../evil")
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        manager.create_variant("HD600", "  ").unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        manager.variant_sets().unwrap()["HD600"],
        set(&["Default", "Dekoni"], "Dekoni")
    );
}

#[test]
fn delete_variant_reselects_default_then_first_and_undo_restores() {
    let (_apo, presets, manager) = history_manager("var-delete");
    let stock = band(1000.0, 3.0);
    let pads = band(2000.0, -2.0);
    manager.save_preset("HD600", &stock).unwrap();
    manager.create_variant("HD600", "Dekoni").unwrap();
    manager.save_preset("HD600", &pads).unwrap(); // Dekoni = pads
    manager.create_variant("HD600", "Brainwavz").unwrap(); // = pads, selected

    // Deleting the selected variant falls back to Default...
    let undo = manager
        .delete_variant("HD600", "Brainwavz")
        .unwrap()
        .expect("undo handle");
    assert_eq!(
        manager.selected_variant("HD600").as_deref(),
        Some("Default")
    );
    assert_eq!(manager.load_preset("HD600").unwrap(), stock);
    assert!(
        presets
            .path()
            .join(".history/.deleted/HD600/Brainwavz.txt")
            .exists(),
        "the file is archived, not removed"
    );
    // ...and undo moves the exact file back, ready to be re-selected.
    assert_eq!(undo, "Brainwavz.txt");
    manager
        .restore_variant("HD600", "Brainwavz", &undo)
        .unwrap();
    assert!(
        !presets
            .path()
            .join(".history/.deleted/HD600/Brainwavz.txt")
            .exists()
    );
    let back = manager.store().load_variant("HD600", "Brainwavz").unwrap();
    assert_eq!(back, pads);
    manager.select_variant("HD600", "Brainwavz").unwrap();
    assert_eq!(manager.load_preset("HD600").unwrap(), back);

    // Deleting a non-selected variant leaves the selection alone.
    manager.delete_variant("HD600", "Default").unwrap();
    assert_eq!(
        manager.selected_variant("HD600").as_deref(),
        Some("Brainwavz")
    );
    // Without a Default, the first remaining variant takes over.
    manager.delete_variant("HD600", "Brainwavz").unwrap();
    assert_eq!(manager.selected_variant("HD600").as_deref(), Some("Dekoni"));
    assert_eq!(manager.load_preset("HD600").unwrap(), pads);
}

#[test]
fn deleting_the_last_variant_leaves_a_plain_preset() {
    let (_apo, presets, manager) = history_manager("var-last");
    let stock = band(1000.0, 3.0);
    manager.save_preset("HD600", &stock).unwrap();
    manager.create_variant("HD600", "Dekoni").unwrap();

    manager.delete_variant("HD600", "Default").unwrap();
    manager.delete_variant("HD600", "Dekoni").unwrap();
    assert!(manager.variant_sets().unwrap().is_empty());
    assert!(!presets.path().join(".variants").join("HD600").exists());
    assert_eq!(manager.selected_variant("HD600"), None);
    assert_eq!(manager.load_preset("HD600").unwrap(), stock); // keeps its content

    // The sidecar entry is gone too: a later variant starts over with a Default.
    manager.create_variant("HD600", "Again").unwrap();
    assert_eq!(
        manager.variant_sets().unwrap()["HD600"],
        set(&["Again", "Default"], "Again")
    );
}

#[test]
fn rename_variant_moves_file_and_selection_and_refuses_clobber() {
    use std::io::ErrorKind;
    let (_apo, _presets, manager) = history_manager("var-rename");
    manager.save_preset("HD600", &band(1000.0, 3.0)).unwrap();
    manager.create_variant("HD600", "Dekoni").unwrap();

    manager
        .rename_variant("HD600", "Dekoni", "Dekoni Elite")
        .unwrap();
    assert_eq!(
        manager.variant_sets().unwrap()["HD600"],
        set(&["Default", "Dekoni Elite"], "Dekoni Elite")
    );
    // Renaming a non-selected variant leaves the selection alone.
    manager.rename_variant("HD600", "Default", "Stock").unwrap();
    assert_eq!(
        manager.selected_variant("HD600").as_deref(),
        Some("Dekoni Elite")
    );
    // No clobbering, and nothing to rename that is not there.
    assert_eq!(
        manager
            .rename_variant("HD600", "Stock", "Dekoni Elite")
            .unwrap_err()
            .kind(),
        ErrorKind::AlreadyExists
    );
    assert_eq!(
        manager
            .rename_variant("HD600", "Missing", "X")
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
    // A case-only rename is a legitimate fix (Windows filesystem).
    manager.rename_variant("HD600", "Stock", "stock").unwrap();
    assert_eq!(
        manager.variant_sets().unwrap()["HD600"].variants,
        vec!["Dekoni Elite", "stock"]
    );
}

#[test]
fn parent_rename_moves_variants_dir_and_selection() {
    let (_apo, presets, manager) = history_manager("var-parent-rename");
    let stock = band(1000.0, 3.0);
    manager.save_preset("HD600", &stock).unwrap();
    manager.create_variant("HD600", "Dekoni").unwrap();

    manager.rename_preset("HD600", "HD 600").unwrap();
    assert!(!presets.path().join(".variants").join("HD600").exists());
    assert!(
        presets
            .path()
            .join(".variants")
            .join("HD 600")
            .join("Dekoni.txt")
            .exists()
    );
    let sets = manager.variant_sets().unwrap();
    assert!(!sets.contains_key("HD600"));
    assert_eq!(sets["HD 600"], set(&["Default", "Dekoni"], "Dekoni"));

    // Renaming onto a preset that has variants of its own is refused up front.
    manager.save_preset("Other", &stock).unwrap();
    manager.create_variant("Other", "Foam").unwrap();
    assert_eq!(
        manager.rename_preset("HD 600", "Other").unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert!(presets.path().join("HD 600.txt").exists(), "nothing moved");
}

#[test]
fn parent_delete_parks_variants_and_restore_variants_brings_them_back() {
    use std::io::ErrorKind;
    let (_apo, presets, manager) = history_manager("var-parent-delete");
    let stock = band(1000.0, 3.0);
    // A hand-set master preamp: snapshots drop it, the parked file keeps it.
    let pads = Config {
        lines: vec![
            Line::Preamp {
                gain: -6.0,
                channel: Channel::Both,
            },
            Line::Filter(Filter::peak(2000.0, -2.0, 1.0)),
        ],
    };
    manager.save_preset("HD600", &stock).unwrap();
    manager.create_variant("HD600", "Dekoni").unwrap();
    manager.save_preset("HD600", &pads).unwrap();

    let receipt = manager.delete_preset("HD600").unwrap();
    let revision = receipt.revision.expect("undo handle");
    let parked = receipt.variants.expect("variants were parked");
    assert_eq!(parked.selected.as_deref(), Some("Dekoni"));
    assert!(
        parked.archive.starts_with(".variants-"),
        "{}",
        parked.archive
    );
    assert!(!presets.path().join(".variants").join("HD600").exists());
    let archive = presets
        .path()
        .join(".history")
        .join(".deleted")
        .join("HD600")
        .join(&parked.archive);
    assert!(archive.join("Dekoni.txt").exists());
    assert!(archive.join("Default.txt").exists());
    assert!(manager.variant_sets().unwrap().is_empty());
    assert!(manager.list_presets().unwrap().is_empty());

    // Undo: the preset, then its variants, then the selection.
    manager.restore_revision("HD600", &revision).unwrap();
    manager.restore_variants("HD600", &parked.archive).unwrap();
    manager.select_variant("HD600", "Dekoni").unwrap();
    assert!(!archive.exists());
    assert_eq!(
        manager.variant_sets().unwrap()["HD600"],
        set(&["Default", "Dekoni"], "Dekoni")
    );
    // The exact parked file came back (preamp included), not the snapshot.
    assert_eq!(manager.load_preset("HD600").unwrap(), pads);
    assert_eq!(
        manager.store().load_variant("HD600", "Default").unwrap(),
        stock
    );

    // Bad handles never reach the filesystem, and a second undo cannot clobber
    // the variants the restored preset now has.
    assert_eq!(
        manager
            .restore_variants("HD600", "../x")
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        manager
            .restore_variants("HD600", &parked.archive)
            .unwrap_err()
            .kind(),
        ErrorKind::AlreadyExists
    );
}

#[test]
fn sidecar_naming_a_missing_variant_reads_as_unselected() {
    let (_apo, presets, manager) = history_manager("var-sidecar");
    manager.save_preset("HD600", &band(1000.0, 3.0)).unwrap();
    manager.create_variant("HD600", "Dekoni").unwrap();

    // The file vanished behind our back (or the sidecar was hand-edited).
    fs::remove_file(presets.path().join(".variants/HD600/Dekoni.txt")).unwrap();
    assert_eq!(manager.selected_variant("HD600"), None);
    assert_eq!(
        manager.variant_sets().unwrap()["HD600"],
        VariantSet {
            variants: vec!["Default".into()],
            selected: None,
        }
    );
    // A save has nothing to mirror into...
    manager.save_preset("HD600", &band(500.0, 1.0)).unwrap();
    assert!(!presets.path().join(".variants/HD600/Dekoni.txt").exists());
    // ...and the next pick repairs the selection, recording that save (which
    // no variant holds) instead of silently overwriting it.
    manager.select_variant("HD600", "Default").unwrap();
    assert_eq!(
        manager.selected_variant("HD600").as_deref(),
        Some("Default")
    );
    let kept = fastpeq_core::history::normalize(&band(500.0, 1.0));
    assert!(
        manager
            .preset_history("HD600")
            .unwrap()
            .iter()
            .any(|r| manager.load_revision("HD600", &r.id).unwrap() == kept)
    );

    // An unreadable sidecar reads as empty, never as an error.
    fs::write(presets.path().join(".variants.json"), "{corrupt").unwrap();
    assert_eq!(manager.variant_sets().unwrap()["HD600"].selected, None);
}

#[test]
fn select_variant_snapshots_content_no_variant_holds() {
    let (_apo, presets, manager) = history_manager("var-drift");
    let stock = band(1000.0, 3.0);
    manager.save_preset("HD600", &stock).unwrap();
    manager.create_variant("HD600", "Dekoni").unwrap(); // selected, == stock

    // Edited behind the app's back: the preset file no longer mirrors Dekoni.
    let edited = band(3000.0, -4.0);
    fs::write(presets.path().join("HD600.txt"), serialize(&edited)).unwrap();
    manager.select_variant("HD600", "Default").unwrap();
    assert_eq!(manager.load_preset("HD600").unwrap(), stock);
    // The edit is recorded rather than lost...
    let revs = manager.preset_history("HD600").unwrap();
    assert_eq!(revs.len(), 1);
    assert!(revs[0].id.ends_with("-save"), "{}", revs[0].id);
    assert_eq!(
        manager.load_revision("HD600", &revs[0].id).unwrap(),
        fastpeq_core::history::normalize(&edited)
    );
    // ...and Dekoni's own file is left as it was.
    assert_eq!(
        manager.store().load_variant("HD600", "Dekoni").unwrap(),
        stock
    );
    // A switch in the normal case still records nothing.
    manager.select_variant("HD600", "Dekoni").unwrap();
    manager.select_variant("HD600", "Default").unwrap();
    assert_eq!(manager.preset_history("HD600").unwrap().len(), 1);
}

#[test]
fn variant_undo_survives_history_dedupe_and_refuses_to_clobber() {
    use std::io::ErrorKind;
    let (_apo, presets, manager) = history_manager("var-undo-stable");
    let stock = band(1000.0, 3.0);
    manager.save_preset("HD600", &stock).unwrap();
    manager.create_variant("HD600", "Dekoni").unwrap(); // Default == Dekoni == stock

    let handle = manager
        .delete_variant("HD600", "Default")
        .unwrap()
        .expect("undo handle");
    assert_eq!(handle, "Default.txt");
    // Saving over the identical remaining variant displaces the same content:
    // the history dedupes the delete revision away into a fresh save one...
    manager.save_preset("HD600", &band(2000.0, -2.0)).unwrap();
    let revs = manager.preset_history("HD600").unwrap();
    assert_eq!(revs.len(), 1);
    assert!(revs[0].id.ends_with("-save"), "{}", revs[0].id);
    // ...which cannot invalidate the undo: its handle is the archived file.
    manager
        .restore_variant("HD600", "Default", &handle)
        .unwrap();
    assert_eq!(
        manager.store().load_variant("HD600", "Default").unwrap(),
        stock
    );
    assert!(
        !presets
            .path()
            .join(".history/.deleted/HD600/Default.txt")
            .exists()
    );
    // The content is a variant again, so no revision keeps it.
    assert!(manager.preset_history("HD600").unwrap().is_empty());

    // A second undo cannot clobber the restored (or a recreated) variant, a
    // bad handle never reaches the filesystem, and a stale one is NotFound.
    assert_eq!(
        manager
            .restore_variant("HD600", "Default", &handle)
            .unwrap_err()
            .kind(),
        ErrorKind::AlreadyExists
    );
    assert_eq!(
        manager
            .restore_variant("HD600", "Stock", "../x")
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        manager
            .restore_variant("HD600", "Stock", "Nope.txt")
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
}

#[test]
fn parent_rename_onto_a_deleted_name_carries_parked_variants_along() {
    let (_apo, presets, manager) = history_manager("var-rename-merge");
    // "HD600" once existed and was deleted, so its archive is there...
    manager.save_preset("HD600", &band(1000.0, 3.0)).unwrap();
    manager.delete_preset("HD600").unwrap();
    // ...and "HD650" had variants, was deleted (parking them) and recreated.
    manager.save_preset("HD650", &band(2000.0, -2.0)).unwrap();
    manager.create_variant("HD650", "Dekoni").unwrap();
    manager.delete_preset("HD650").unwrap();
    manager.save_preset("HD650", &band(3000.0, 1.0)).unwrap();

    // Renaming it onto the deleted name merges the two archives, the parked
    // directory included, so those variants stay recoverable under the new name.
    manager.rename_preset("HD650", "HD600").unwrap();
    let archive = presets.path().join(".history/.deleted/HD600");
    assert!(archive.join("HD600.txt").exists());
    assert!(archive.join("HD650.txt").exists());
    assert!(!presets.path().join(".history/.deleted/HD650").exists());
    let parked: Vec<String> = fs::read_dir(&archive)
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(parked.len(), 1, "{parked:?}");
    assert!(parked[0].starts_with(".variants-"), "{}", parked[0]);
    manager.restore_variants("HD600", &parked[0]).unwrap();
    assert_eq!(
        manager.variant_sets().unwrap()["HD600"],
        VariantSet {
            variants: vec!["Default".into(), "Dekoni".into()],
            selected: None,
        }
    );
}

/// A delete that fails part-way must leave the preset whole, not stripped of
/// its variants: they are parked first and come back when the preset file
/// itself cannot be archived. (A file open without sharing blocks the move —
/// a Windows-only way to provoke that failure.)
#[cfg(windows)]
#[test]
fn delete_preset_leaves_the_preset_whole_when_its_file_cannot_be_archived() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_apo, presets, manager) = history_manager("var-delete-locked");
    let stock = band(1000.0, 3.0);
    manager.save_preset("HD600", &stock).unwrap();
    manager.create_variant("HD600", "Dekoni").unwrap();

    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(presets.path().join("HD600.txt"))
        .unwrap();
    assert!(manager.delete_preset("HD600").is_err());
    drop(lock);

    assert_eq!(manager.list_presets().unwrap(), vec!["HD600"]);
    assert_eq!(manager.load_preset("HD600").unwrap(), stock);
    assert_eq!(
        manager.variant_sets().unwrap()["HD600"],
        set(&["Default", "Dekoni"], "Dekoni")
    );
    let archive = presets.path().join(".history/.deleted/HD600");
    assert!(
        !archive.exists() || fs::read_dir(&archive).unwrap().next().is_none(),
        "nothing is left parked"
    );
}

/// The counterpart for rename: when the variants directory cannot move, the
/// preset file goes back, so nothing is stranded under either name.
#[cfg(windows)]
#[test]
fn rename_preset_rolls_back_when_its_variants_cannot_move() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_apo, presets, manager) = history_manager("var-rename-locked");
    manager.save_preset("HD600", &band(1000.0, 3.0)).unwrap();
    manager.create_variant("HD600", "Dekoni").unwrap();

    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(presets.path().join(".variants/HD600/Dekoni.txt"))
        .unwrap();
    assert!(manager.rename_preset("HD600", "HD650").is_err());
    drop(lock);

    assert_eq!(manager.list_presets().unwrap(), vec!["HD600"]);
    assert!(!presets.path().join(".variants/HD650").exists());
    assert_eq!(
        manager.variant_sets().unwrap()["HD600"],
        set(&["Default", "Dekoni"], "Dekoni")
    );
}

#[test]
fn a_new_preset_never_inherits_variants_left_under_its_name() {
    let (_apo, presets, manager) = history_manager("var-orphan");
    let stock = band(1000.0, 3.0);
    manager.save_preset("HD600", &stock).unwrap();
    manager.create_variant("HD600", "Dekoni").unwrap();

    // The preset file went missing behind the app's back...
    fs::remove_file(presets.path().join("HD600.txt")).unwrap();
    assert!(manager.list_presets().unwrap().is_empty());
    // ...and a new preset is written under the name: it starts plain, and its
    // content never lands in the old selected variant.
    let fresh = band(500.0, 1.0);
    manager.save_preset("HD600", &fresh).unwrap();
    assert_eq!(manager.load_preset("HD600").unwrap(), fresh);
    assert!(manager.variant_sets().unwrap().is_empty());
    assert_eq!(manager.selected_variant("HD600"), None);
    assert!(!presets.path().join(".variants/HD600").exists());
    // The leftovers are parked in the archive, intact, for hand recovery.
    let archive = presets.path().join(".history/.deleted/HD600");
    let parked: Vec<PathBuf> = fs::read_dir(&archive)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    assert_eq!(parked.len(), 1, "{parked:?}");
    assert_eq!(
        parse(&fs::read_to_string(parked[0].join("Dekoni.txt")).unwrap()),
        stock
    );
    assert!(parked[0].join("Default.txt").exists());
}

/// A switch is both halves or neither: with the sidecar unwritable, the preset
/// file must not be left holding the other variant's content (the next save
/// would land it in the wrong variant).
#[cfg(windows)]
#[test]
fn select_variant_puts_the_preset_file_back_when_the_sidecar_cannot_be_written() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_apo, presets, manager) = history_manager("var-select-locked");
    let stock = band(1000.0, 3.0);
    let pads = band(2000.0, -2.0);
    manager.save_preset("HD600", &stock).unwrap();
    manager.create_variant("HD600", "Dekoni").unwrap();
    manager.save_preset("HD600", &pads).unwrap(); // Dekoni = pads, selected

    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(presets.path().join(".variants.json"))
        .unwrap();
    assert!(manager.select_variant("HD600", "Default").is_err());
    drop(lock);

    assert_eq!(manager.load_preset("HD600").unwrap(), pads);
    assert_eq!(manager.selected_variant("HD600").as_deref(), Some("Dekoni"));
    assert_eq!(
        manager.store().load_variant("HD600", "Default").unwrap(),
        stock
    );
}

#[test]
fn an_imported_preset_never_inherits_variants_left_under_its_name() {
    let (apo, presets, manager) = history_manager("var-orphan-import");
    manager.save_preset("HD600", &band(1000.0, 3.0)).unwrap();
    manager.create_variant("HD600", "Dekoni").unwrap();
    fs::remove_file(presets.path().join("HD600.txt")).unwrap();

    // A .peace import under the vanished name takes the same write path as a
    // save, so the leftovers are parked rather than inherited.
    let peace = apo.path().join("HD600.peace");
    fs::write(
        &peace,
        "[General]\nPreAmp=-3\n[Frequencies]\nFrequency1=100\n[Gains]\nGain1=4\n\
         [Qualities]\nQuality1=1.5\n[Speakers]\nSpeakerId0=0\nSpeakerTargets0=all\n",
    )
    .unwrap();
    let report = manager.import_peace_files(&[peace]).unwrap();
    assert_eq!(report.imported, vec!["HD600"]);
    assert!(manager.variant_sets().unwrap().is_empty());
    assert_eq!(manager.selected_variant("HD600"), None);
    assert!(!presets.path().join(".variants/HD600").exists());
    assert!(
        fs::read_dir(presets.path().join(".history/.deleted/HD600"))
            .unwrap()
            .flatten()
            .any(|e| e.path().is_dir()),
        "the old variants are parked"
    );
}
