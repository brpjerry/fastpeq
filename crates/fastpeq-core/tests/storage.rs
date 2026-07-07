//! Integration tests for the preset store, atomic writer, and manager flow.

use fastpeq_core::apo::env::ApoInstall;
use fastpeq_core::{
    Channel, Config, Filter, FilterKind, Line, Manager, PresetStore, Tone, provenance, serialize,
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
