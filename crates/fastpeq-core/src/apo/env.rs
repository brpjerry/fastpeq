//! Locating the Equalizer APO installation.
//!
//! APO records its config directory in the registry under
//! `HKLM\SOFTWARE\EqualizerAPO\ConfigPath`. We read that rather than guessing
//! `C:\Program Files\EqualizerAPO\config`, since the install location is
//! user-selectable.

use std::path::PathBuf;

/// A detected Equalizer APO installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApoInstall {
    /// The directory containing `config.txt` and included preset files.
    pub config_path: PathBuf,
}

impl ApoInstall {
    /// The path to the live `config.txt` that APO watches.
    pub fn config_file(&self) -> PathBuf {
        self.config_path.join("config.txt")
    }
}

/// Why detection failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectError {
    /// The APO registry key is absent — APO is (probably) not installed.
    NotInstalled,
    /// Detection isn't supported on this platform (APO is Windows-only).
    UnsupportedPlatform,
    /// The registry key exists but couldn't be read as expected.
    Registry(String),
}

impl std::fmt::Display for DetectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DetectError::NotInstalled => write!(f, "Equalizer APO is not installed"),
            DetectError::UnsupportedPlatform => {
                write!(f, "Equalizer APO detection is only supported on Windows")
            }
            DetectError::Registry(msg) => write!(f, "could not read APO registry: {msg}"),
        }
    }
}

impl std::error::Error for DetectError {}

/// Detect the Equalizer APO installation from the registry.
#[cfg(windows)]
pub fn detect() -> Result<ApoInstall, DetectError> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey(r"SOFTWARE\EqualizerAPO")
        .map_err(|_| DetectError::NotInstalled)?;
    let config_path: String = key
        .get_value("ConfigPath")
        .map_err(|e| DetectError::Registry(e.to_string()))?;

    Ok(ApoInstall {
        config_path: PathBuf::from(config_path),
    })
}

/// Detect the Equalizer APO installation (non-Windows stub).
#[cfg(not(windows))]
pub fn detect() -> Result<ApoInstall, DetectError> {
    Err(DetectError::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_file_appends_config_txt() {
        let install = ApoInstall {
            config_path: PathBuf::from(r"C:\Program Files\EqualizerAPO\config"),
        };
        assert_eq!(
            install.config_file(),
            PathBuf::from(r"C:\Program Files\EqualizerAPO\config\config.txt")
        );
    }

    /// Smoke test against the real machine; ignored by default because it
    /// requires Equalizer APO to actually be installed.
    /// Run with: `cargo test -- --ignored detects_real_install`
    #[test]
    #[ignore]
    fn detects_real_install() {
        let install = detect().expect("APO should be installed on this machine");
        println!(
            "Detected APO config path: {}",
            install.config_path.display()
        );
        assert!(install.config_path.is_dir(), "config path should exist");
    }
}
