//! A file-backed library of named presets.
//!
//! Each preset is just a native APO config file (`<name>.txt`) living in
//! fastpeq's own directory — deliberately *not* APO's config folder, so we
//! don't clutter it the way PEACE does. Because presets are native APO text,
//! they're trivially shareable and importable.
//!
//! A preset may also hold **variants** (sub-presets — one per set of ear pads
//! or IEM tips): `.variants/<name>/<variant>.txt`, one plain APO file each.
//! The preset file itself always mirrors the *selected* variant, so every
//! reader of `<name>.txt` sees the sub-preset in use without knowing variants
//! exist; which one that is lives in the manager's `.variants.json` sidecar.

use crate::apo::model::Config;
use crate::apo::writer::write_config_atomic;
use crate::parse;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const PRESET_EXT: &str = "txt";
/// Hidden directory holding each preset's variants (`.variants/<name>/`).
const VARIANTS_DIR: &str = ".variants";

/// A directory of preset `.txt` files.
#[derive(Debug, Clone)]
pub struct PresetStore {
    dir: PathBuf,
}

impl PresetStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        PresetStore { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Create the preset directory if it doesn't exist.
    pub fn ensure_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.dir)
    }

    /// Preset names (file stems), sorted case-insensitively. Missing directory
    /// yields an empty list rather than an error. Only top-level files count,
    /// so the hidden sidecars and the `.variants/` tree never show up here.
    pub fn list(&self) -> io::Result<Vec<String>> {
        txt_stems(&self.dir)
    }

    pub fn exists(&self, name: &str) -> bool {
        matches!(self.path_for(name), Ok(path) if path.exists())
    }

    pub fn load(&self, name: &str) -> io::Result<Config> {
        let text = fs::read_to_string(self.path_for(name)?)?;
        Ok(parse(&text))
    }

    pub fn save(&self, name: &str, config: &Config) -> io::Result<()> {
        self.ensure_dir()?;
        // A preset file is pure EQ: never carry the live config's provenance
        // stamp into one (a captured/edited config may still have it).
        let config = crate::provenance::strip(config);
        write_config_atomic(&self.path_for(name)?, &config)
    }

    /// Delete a preset. Deleting a non-existent preset is a no-op.
    pub fn delete(&self, name: &str) -> io::Result<()> {
        match fs::remove_file(self.path_for(name)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Rename a preset, taking its variants directory along. Fails if `from` is
    /// missing or `to` already exists (so a rename never silently clobbers
    /// another preset); a case-only rename of the same files is allowed.
    pub fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let from_path = self.path_for(from)?;
        let to_path = self.path_for(to)?;
        let from_variants = self.variants_dir(from)?;
        let to_variants = self.variants_dir(to)?;
        // Pre-flight the variants move, so the preset file is never renamed
        // and then left with its variants stranded under the old name.
        if from_variants.exists()
            && to_variants.exists()
            && !same_entry(&from_variants, &to_variants)
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("preset already exists: {to}"),
            ));
        }
        rename_file(&from_path, &to_path, "preset")?;
        if from_variants.exists() {
            fs::rename(&from_variants, &to_variants)?;
        }
        Ok(())
    }

    /// Resolve a preset name to its file path, rejecting names that could escape
    /// the preset directory or are illegal as Windows filenames.
    pub fn path_for(&self, name: &str) -> io::Result<PathBuf> {
        let name = checked_name(name)?;
        Ok(self.dir.join(format!("{name}.{PRESET_EXT}")))
    }

    // --- Variants: `.variants/<name>/<variant>.txt`, one plain file each -------

    /// The root of the variants tree (`<store>/.variants`); may not exist.
    pub fn variants_root(&self) -> PathBuf {
        self.dir.join(VARIANTS_DIR)
    }

    /// The directory holding `name`'s variant files; may not exist.
    pub fn variants_dir(&self, name: &str) -> io::Result<PathBuf> {
        let name = checked_name(name)?;
        Ok(self.variants_root().join(name))
    }

    /// A variant's file path. Both names are validated like [`path_for`]
    /// (a variant name becomes a file name too).
    ///
    /// [`path_for`]: Self::path_for
    pub fn variant_path(&self, name: &str, variant: &str) -> io::Result<PathBuf> {
        let variant = checked_name(variant)?;
        Ok(self
            .variants_dir(name)?
            .join(format!("{variant}.{PRESET_EXT}")))
    }

    /// `name`'s variant names, sorted case-insensitively; empty when the preset
    /// has none (no directory).
    pub fn list_variants(&self, name: &str) -> io::Result<Vec<String>> {
        txt_stems(&self.variants_dir(name)?)
    }

    pub fn variant_exists(&self, name: &str, variant: &str) -> bool {
        matches!(self.variant_path(name, variant), Ok(path) if path.exists())
    }

    pub fn load_variant(&self, name: &str, variant: &str) -> io::Result<Config> {
        let text = fs::read_to_string(self.variant_path(name, variant)?)?;
        Ok(parse(&text))
    }

    pub fn save_variant(&self, name: &str, variant: &str, config: &Config) -> io::Result<()> {
        let path = self.variant_path(name, variant)?;
        fs::create_dir_all(self.variants_dir(name)?)?;
        write_config_atomic(&path, &crate::provenance::strip(config))
    }

    /// Delete a variant file. A missing one is a no-op.
    pub fn delete_variant(&self, name: &str, variant: &str) -> io::Result<()> {
        match fs::remove_file(self.variant_path(name, variant)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Drop `name`'s variants directory once it is empty. Best-effort: a
    /// non-empty or missing directory is simply left alone.
    pub fn remove_variants_dir(&self, name: &str) {
        if let Ok(dir) = self.variants_dir(name) {
            let _ = fs::remove_dir(dir);
        }
    }

    /// Rename one of `name`'s variants, with the same no-clobber rule as
    /// [`rename`](Self::rename).
    pub fn rename_variant(&self, name: &str, from: &str, to: &str) -> io::Result<()> {
        rename_file(
            &self.variant_path(name, from)?,
            &self.variant_path(name, to)?,
            "variant",
        )
    }
}

/// The `.txt` file stems directly inside `dir`, sorted case-insensitively. A
/// missing directory yields an empty list rather than an error.
fn txt_stems(dir: &Path) -> io::Result<Vec<String>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut names = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let is_preset = path.extension().and_then(|s| s.to_str()) == Some(PRESET_EXT);
        if is_preset && let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            names.push(stem.to_string());
        }
    }
    // Cached keys: `sort_by_key` would re-allocate the lowercase string on
    // every comparison.
    names.sort_by_cached_key(|n| n.to_lowercase());
    Ok(names)
}

/// Trim a preset or variant name and reject anything unsafe as a file name.
fn checked_name(name: &str) -> io::Result<&str> {
    let name = name.trim();
    if name.is_empty() || !is_safe_name(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid preset name: {name:?}"),
        ));
    }
    Ok(name)
}

/// Rename `from` to `to`. Fails if `from` is missing or `to` already exists —
/// unless `to` resolves to the *same* entry as `from` (a case-only rename on a
/// case-insensitive filesystem, e.g. "HD600" -> "hd600" on Windows), which is
/// a legitimate fix rather than a clobber.
fn rename_file(from: &Path, to: &Path, what: &str) -> io::Result<()> {
    let stem = |p: &Path| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string()
    };
    if !from.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{what} not found: {}", stem(from)),
        ));
    }
    if to.exists() && !same_entry(from, to) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{what} already exists: {}", stem(to)),
        ));
    }
    fs::rename(from, to)
}

/// Whether two paths resolve to the same file or directory.
fn same_entry(a: &Path, b: &Path) -> bool {
    matches!(
        (fs::canonicalize(a), fs::canonicalize(b)),
        (Ok(a), Ok(b)) if a == b
    )
}

/// Shared with [`crate::history`], whose revision dirs are keyed by preset name.
pub(crate) fn is_safe_name(name: &str) -> bool {
    // Reject path traversal, separators / drive specifiers / wildcards / illegal
    // filename chars and control chars, and a trailing dot or space (Windows
    // silently strips those, which would change the file we read/write).
    if name.contains("..") || name.ends_with('.') || name.ends_with(' ') {
        return false;
    }
    if name.chars().any(|c| {
        matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
    }) {
        return false;
    }
    // Windows reserved device names are illegal even with an extension
    // (e.g. "CON", "nul", "COM1.txt"), so test the part before the first dot.
    let stem = name.split('.').next().unwrap_or(name);
    !is_reserved_device(stem)
}

/// Whether `stem` is a Windows reserved device name (case-insensitive):
/// CON, PRN, AUX, NUL, the console devices CONIN$/CONOUT$, and COM/LPT
/// followed by one digit 1–9 — including the superscript digits ¹ ² ³, which
/// newer Windows also reserves (COM0/LPT0 are not reserved).
fn is_reserved_device(stem: &str) -> bool {
    let s = stem.trim().to_ascii_uppercase();
    if matches!(
        s.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let Some(unit) = s.strip_prefix("COM").or_else(|| s.strip_prefix("LPT")) else {
        return false;
    };
    matches!(
        unit,
        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
    )
}
