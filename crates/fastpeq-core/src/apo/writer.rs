//! Writing the live `config.txt` that Equalizer APO watches.
//!
//! APO reloads its config the instant the file changes, so a non-atomic write
//! risks the engine reading a half-written file mid-save. Every write here goes
//! to a sibling temp file first and is then renamed over the target, which is
//! atomic on a single volume.

use crate::apo::model::Config;
use crate::apo::serialize::serialize;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Serialize `config` and atomically replace the file at `path`.
pub fn write_config_atomic(path: &Path, config: &Config) -> io::Result<()> {
    let mut text = serialize(config);
    text.push('\n'); // APO config files conventionally end with a newline
    write_atomic(path, text.as_bytes())
}

/// Write a neutral (empty) configuration — i.e. Equalizer APO passthrough.
pub fn write_bypass(path: &Path) -> io::Result<()> {
    write_config_atomic(path, &Config::new())
}

/// Atomically write arbitrary UTF-8 text (used for sidecar metadata files).
pub fn write_text_atomic(path: &Path, text: &str) -> io::Result<()> {
    write_atomic(path, text.as_bytes())
}

/// Copy `config_file` to `backup_path`, but only if no backup exists yet.
///
/// Returns `Ok(true)` if a backup was just created, `Ok(false)` if one already
/// existed (or there was nothing to back up). This lets the app preserve the
/// user's pre-fastpeq `config.txt` exactly once, before it ever takes over.
pub fn backup_once(config_file: &Path, backup_path: &Path) -> io::Result<bool> {
    if backup_path.exists() {
        return Ok(false);
    }
    if !config_file.exists() {
        return Ok(false);
    }
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(config_file, backup_path)?;
    Ok(true)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "target path has no parent directory",
        )
    })?;
    fs::create_dir_all(dir)?;

    let tmp = unique_temp_path(path);
    fs::write(&tmp, bytes)?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp); // best-effort cleanup
        return Err(e);
    }
    Ok(())
}

/// A unique sibling temp path so concurrent writers never collide.
fn unique_temp_path(target: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("config.txt");
    target.with_file_name(format!(".{name}.fastpeq-{}-{n}.tmp", std::process::id()))
}
