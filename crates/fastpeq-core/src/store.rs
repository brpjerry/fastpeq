//! A file-backed library of named presets.
//!
//! Each preset is just a native APO config file (`<name>.txt`) living in
//! fastpeq's own directory — deliberately *not* APO's config folder, so we
//! don't clutter it the way PEACE does. Because presets are native APO text,
//! they're trivially shareable and importable.

use crate::apo::model::Config;
use crate::apo::writer::write_config_atomic;
use crate::parse;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const PRESET_EXT: &str = "txt";

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
    /// yields an empty list rather than an error.
    pub fn list(&self) -> io::Result<Vec<String>> {
        let entries = match fs::read_dir(&self.dir) {
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
        names.sort_by_key(|n| n.to_lowercase());
        Ok(names)
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

    /// Rename a preset. Fails if `from` is missing or `to` already exists
    /// (so a rename never silently clobbers another preset).
    pub fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let from_path = self.path_for(from)?;
        let to_path = self.path_for(to)?;
        if !from_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("preset not found: {from}"),
            ));
        }
        // `to` already existing is a clobber — unless it resolves to the *same*
        // file as `from` (a case-only rename on a case-insensitive filesystem,
        // e.g. "HD600" -> "hd600" on Windows), which is a legitimate fix.
        if to_path.exists() {
            let same = matches!(
                (fs::canonicalize(&from_path), fs::canonicalize(&to_path)),
                (Ok(a), Ok(b)) if a == b
            );
            if !same {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("preset already exists: {to}"),
                ));
            }
        }
        fs::rename(from_path, to_path)
    }

    /// Resolve a preset name to its file path, rejecting names that could escape
    /// the preset directory or are illegal as Windows filenames.
    pub fn path_for(&self, name: &str) -> io::Result<PathBuf> {
        let name = name.trim();
        if name.is_empty() || !is_safe_name(name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid preset name: {name:?}"),
            ));
        }
        Ok(self.dir.join(format!("{name}.{PRESET_EXT}")))
    }
}

fn is_safe_name(name: &str) -> bool {
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
/// CON, PRN, AUX, NUL, COM1–COM9, LPT1–LPT9.
fn is_reserved_device(stem: &str) -> bool {
    let s = stem.trim().to_ascii_uppercase();
    if matches!(s.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let b = s.as_bytes();
    (s.starts_with("COM") || s.starts_with("LPT"))
        && b.len() == 4
        && b[3].is_ascii_digit()
        && b[3] != b'0'
}
