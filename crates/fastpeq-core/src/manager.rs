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
use crate::parse;
use crate::provenance;
use crate::store::PresetStore;
use crate::tone::{self, Tone};
use serde::Serialize;
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
        self.store.save(name, config)
    }

    pub fn delete_preset(&self, name: &str) -> io::Result<()> {
        self.store.delete(name)?;
        let mut map = self.categories()?;
        if map.remove(name).is_some() {
            self.write_categories(&map)?;
        }
        Ok(())
    }

    pub fn rename_preset(&self, from: &str, to: &str) -> io::Result<()> {
        self.store.rename(from, to)?;
        let mut map = self.categories()?;
        if let Some(category) = map.remove(from) {
            map.insert(to.to_string(), category);
            self.write_categories(&map)?;
        }
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

    /// Persist new tone values and re-lay the overlay over the current base EQ
    /// (preset or live edit), so changing a knob doesn't disturb the rest.
    pub fn set_tone(&self, tone: &Tone) -> io::Result<()> {
        self.store.ensure_dir()?;
        let text = serde_json::to_string_pretty(tone).unwrap_or_else(|_| "{}".to_string());
        writer::write_text_atomic(&self.tone_path(), &text)?;
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
        let config = provenance::set(&self.store.load(name)?, name);
        self.write_live(&tone::compose(&config, tone))
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
    /// [provenance] stamp.
    ///
    /// Derived from disk rather than tracked in memory, so it stays correct
    /// across restarts. The stamp is written on apply, so this is O(1): the live
    /// config is "active" iff it carries a stamp whose preset still exists and
    /// still matches the live EQ — exactly, or equivalently after Auto Preamp has
    /// rewritten the master gain (see [`Config::is_equivalent`]). There is no
    /// content scan: a config produced *outside* fastpeq has no stamp and is, by
    /// design, not detected. Returns `None` for a bypassed config (no preamp or
    /// filters), a missing/stale stamp, or an EQ that has diverged from its preset.
    ///
    /// [provenance]: crate::provenance
    pub fn active_preset(&self) -> io::Result<Option<String>> {
        let current = self.current_config()?;
        // The base EQ: tone overlay and provenance stamp removed, so neither the
        // global tone controls nor the stamp itself can break the equivalence check.
        let live = provenance::strip(&tone::strip(&current));
        if live.preamp().is_none() && live.filters().next().is_none() {
            return Ok(None);
        }

        // Trust the stamp, but only while the live EQ still matches the named
        // preset (`is_equivalent` also covers exact equality, and tolerates the
        // master gain Auto Preamp rewrites). A divergent edit or a since-deleted
        // preset therefore reads as "not active" rather than guessing by content.
        if let Some(name) = provenance::name(&current)
            && let Ok(preset) = self.store.load(&name)
            && preset.is_equivalent(&live)
        {
            return Ok(Some(name));
        }

        Ok(None)
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
