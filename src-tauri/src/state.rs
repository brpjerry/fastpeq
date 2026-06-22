//! Shared application state: the core [`Manager`] plus the currently active
//! preset, behind a mutex. All operations funnel through here so the tray, the
//! hotkey, and the IPC commands stay consistent.

use fastpeq_core::apo::env;
use fastpeq_core::{Category, Config, ImportReport, Manager as CoreManager, PresetStore, Tone};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

/// Reported to the UI so it can show APO status / errors.
#[derive(Serialize, Clone)]
pub struct ApoStatus {
    pub installed: bool,
    pub config_path: Option<String>,
    pub error: Option<String>,
}

/// Persisted backend settings, read at startup before the WebView exists (so it
/// can't live in the frontend's localStorage). Stored as `settings.json` in the
/// app data dir.
#[derive(Serialize, Deserialize, Default)]
struct Settings {
    /// Custom preset storage directory; `None` uses the default under app data.
    presets_dir: Option<String>,
}

fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

fn load_settings(data_dir: &Path) -> Settings {
    match std::fs::read_to_string(settings_path(data_dir)) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

fn save_settings(data_dir: &Path, settings: &Settings) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(settings).unwrap_or_else(|_| "{}".to_string());
    // Atomic write (temp file + rename), like every other file the app persists,
    // so a crash mid-write can't leave a truncated settings.json.
    fastpeq_core::apo::write_text_atomic(&settings_path(data_dir), &text)
}

/// Cached active preset. `Unknown` forces a disk re-derive on the next read; the
/// app sets it to `Known` whenever it applies or bypasses, so the common path
/// (clicking a preset, refreshing the tray) skips the scan over every preset.
enum Active {
    Known(Option<String>),
    Unknown,
}

/// E2E test override. When `FASTPEQ_TEST_DATA_DIR` is set, the app runs fully
/// self-contained: that directory is both the app data dir *and* the APO config
/// dir, so a run reads/writes only its own throwaway `config.txt` and preset
/// library and never touches the machine's real Equalizer APO install. Empty is
/// treated as unset.
fn test_data_dir() -> Option<PathBuf> {
    std::env::var_os("FASTPEQ_TEST_DATA_DIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Build the inner state for a given presets directory, (re)detecting APO. Used
/// at startup and whenever the presets directory changes.
fn build_inner(data_dir: &Path, presets_dir: PathBuf) -> Inner {
    let backup_path = data_dir.join("config.backup.txt");
    // In E2E mode, skip the registry and treat the test data dir as the APO
    // config dir; otherwise detect the real install.
    let detected = if test_data_dir().is_some() {
        Ok(env::ApoInstall {
            config_path: data_dir.to_path_buf(),
        })
    } else {
        env::detect()
    };
    match detected {
        Ok(install) => {
            let store = PresetStore::new(&presets_dir);
            let _ = store.ensure_dir();
            let manager = CoreManager::new(install, store, backup_path);
            let tone = manager.tone().unwrap_or_default(); // cache; sidecar is persistence
            Inner {
                manager: Some(manager),
                apo_error: None,
                presets_dir,
                bypassed: false,
                restore: None,
                tone,
                active: Active::Unknown,
            }
        }
        Err(e) => Inner {
            manager: None,
            apo_error: Some(e.to_string()),
            presets_dir,
            bypassed: false,
            restore: None,
            tone: Tone::default(),
            active: Active::Unknown,
        },
    }
}

pub struct AppState {
    data_dir: PathBuf,
    inner: Mutex<Inner>,
}

struct Inner {
    /// `None` if Equalizer APO wasn't detected — the UI shows `apo_error`.
    manager: Option<CoreManager>,
    apo_error: Option<String>,
    /// The directory presets are currently stored in.
    presets_dir: PathBuf,
    /// Whether the EQ is currently bypassed (so the toggle is a true toggle and
    /// the UI can reflect a bypass triggered from the tray/hotkey).
    bypassed: bool,
    /// The base config that was live when bypass began, re-applied on un-bypass
    /// so the exact prior state returns (including unsaved edits).
    restore: Option<Config>,
    /// Cached global tone overlay, so live-drag writes don't re-read the sidecar.
    tone: Tone,
    /// Cached active preset (see [`Active`]).
    active: Active,
}

impl AppState {
    pub fn initialize(app: &AppHandle) -> tauri::Result<Self> {
        let data_dir = match test_data_dir() {
            // The test dir may not exist on first launch; the real one does.
            Some(dir) => {
                let _ = std::fs::create_dir_all(&dir);
                dir
            }
            None => app.path().app_data_dir()?,
        };
        let presets_dir = load_settings(&data_dir)
            .presets_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("presets"));
        let inner = build_inner(&data_dir, presets_dir);
        Ok(AppState {
            data_dir,
            inner: Mutex::new(inner),
        })
    }

    fn default_presets_dir(&self) -> PathBuf {
        self.data_dir.join("presets")
    }

    /// The directory presets are currently stored in.
    pub fn presets_dir(&self) -> String {
        self.inner.lock().unwrap().presets_dir.display().to_string()
    }

    /// Switch to a new preset storage directory (created if needed), persist the
    /// choice, and rebuild the manager so reads/writes use it.
    pub fn set_presets_dir(&self, path: &str) -> Result<(), String> {
        let dir = PathBuf::from(path);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut settings = load_settings(&self.data_dir);
        settings.presets_dir = Some(dir.to_string_lossy().into_owned());
        save_settings(&self.data_dir, &settings).map_err(|e| e.to_string())?;
        let mut inner = self.inner.lock().unwrap();
        *inner = build_inner(&self.data_dir, dir);
        Ok(())
    }

    /// Reset back to the default directory under the app data folder.
    pub fn reset_presets_dir(&self) -> Result<(), String> {
        let mut settings = load_settings(&self.data_dir);
        settings.presets_dir = None;
        save_settings(&self.data_dir, &settings).map_err(|e| e.to_string())?;
        let dir = self.default_presets_dir();
        let mut inner = self.inner.lock().unwrap();
        *inner = build_inner(&self.data_dir, dir);
        Ok(())
    }

    /// Open the current presets directory in the system file explorer.
    pub fn open_presets_dir(&self) -> Result<(), String> {
        let dir = self.inner.lock().unwrap().presets_dir.clone();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// A clone of the core manager, or an error message if APO isn't available.
    fn manager(&self) -> Result<CoreManager, String> {
        let inner = self.inner.lock().unwrap();
        inner.manager.clone().ok_or_else(|| {
            inner
                .apo_error
                .clone()
                .unwrap_or_else(|| "Equalizer APO is not detected".to_string())
        })
    }

    pub fn status(&self) -> ApoStatus {
        let inner = self.inner.lock().unwrap();
        match &inner.manager {
            Some(m) => ApoStatus {
                installed: true,
                config_path: Some(m.install().config_file().display().to_string()),
                error: None,
            },
            None => ApoStatus {
                installed: false,
                config_path: None,
                error: inner.apo_error.clone(),
            },
        }
    }

    pub fn list_presets(&self) -> Result<Vec<String>, String> {
        self.manager()?.list_presets().map_err(|e| e.to_string())
    }

    /// The active preset, served from the cache when known. Used by the tray
    /// (rebuilt after every command), so it avoids re-scanning the library each
    /// time. A miss falls back to a disk re-derive.
    pub fn active(&self) -> Option<String> {
        {
            let inner = self.inner.lock().unwrap();
            if let Active::Known(a) = &inner.active {
                return a.clone();
            }
        }
        self.redetect_active()
    }

    /// Re-derive the active preset from the live `config.txt` (catching changes
    /// made by another tool) and refresh the cache. The frontend calls this on
    /// reload/focus, so external edits are still picked up.
    pub fn redetect_active(&self) -> Option<String> {
        let derived = self
            .manager()
            .ok()
            .and_then(|m| m.active_preset().ok())
            .flatten();
        self.inner.lock().unwrap().active = Active::Known(derived.clone());
        derived
    }

    /// The cached tone overlay (kept in sync by `set_tone`).
    fn tone_cache(&self) -> Tone {
        self.inner.lock().unwrap().tone
    }

    /// Mark the cached active preset stale, so the next read re-derives it from
    /// the live config. Used after any edit that may change what's active.
    fn invalidate_active(&self) {
        self.inner.lock().unwrap().active = Active::Unknown;
    }

    pub fn apply(&self, name: &str) -> Result<(), String> {
        let tone = self.tone_cache();
        self.manager()?
            .apply_preset(name, &tone)
            .map_err(|e| e.to_string())?;
        let mut inner = self.inner.lock().unwrap();
        inner.bypassed = false;
        inner.restore = None;
        inner.active = Active::Known(Some(name.to_string()));
        Ok(())
    }

    /// Whether the EQ is currently bypassed (for the UI and tray checkmark).
    pub fn is_bypassed(&self) -> bool {
        self.inner.lock().unwrap().bypassed
    }

    /// Toggle bypass: drop the filters (keeping the preamp), or restore the
    /// preset that was active when bypass began. Shared by the Bypass button,
    /// the tray item, and the global hotkey so all three stay consistent.
    pub fn toggle_bypass(&self) -> Result<(), String> {
        let manager = self.manager()?;
        let (bypassed, restore) = {
            let inner = self.inner.lock().unwrap();
            (inner.bypassed, inner.restore.clone())
        };
        if bypassed {
            // Un-bypass: re-apply the exact config that was live before bypassing.
            if let Some(base) = &restore {
                let tone = self.tone_cache();
                manager
                    .apply_config(base, &tone)
                    .map_err(|e| e.to_string())?;
            }
            let mut inner = self.inner.lock().unwrap();
            inner.bypassed = false;
            inner.restore = None;
            inner.active = Active::Unknown; // the restored config may or may not match a preset
        } else {
            // Capture the current base EQ (tone stripped) so un-bypass restores it
            // exactly — including unsaved edits — then drop the filters.
            let base = manager.base_config().map_err(|e| e.to_string())?;
            manager.bypass().map_err(|e| e.to_string())?;
            let mut inner = self.inner.lock().unwrap();
            inner.bypassed = true;
            inner.restore = Some(base);
            inner.active = Active::Known(None);
        }
        Ok(())
    }

    pub fn capture(&self, name: &str) -> Result<(), String> {
        self.manager()?
            .capture_current(name)
            .map_err(|e| e.to_string())?;
        self.invalidate_active(); // the captured preset may now match the live config
        Ok(())
    }

    pub fn delete(&self, name: &str) -> Result<(), String> {
        self.manager()?
            .delete_preset(name)
            .map_err(|e| e.to_string())?;
        self.invalidate_active(); // a deleted preset can no longer be "active"
        Ok(())
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        self.manager()?
            .rename_preset(from, to)
            .map_err(|e| e.to_string())?;
        self.invalidate_active(); // the active preset's name may have changed
        Ok(())
    }

    /// Load a preset as a structured config (for the parametric editor).
    pub fn load_config(&self, name: &str) -> Result<Config, String> {
        self.manager()?.load_preset(name).map_err(|e| e.to_string())
    }

    /// Save a structured config back to a preset.
    pub fn save_config(&self, name: &str, config: &Config) -> Result<(), String> {
        self.manager()?
            .save_preset(name, config)
            .map_err(|e| e.to_string())?;
        self.invalidate_active(); // the saved preset may now match the live config
        Ok(())
    }

    /// Live preview: write the given config straight to the live `config.txt`
    /// (with the one-time backup) WITHOUT touching any preset file. Saving the
    /// preset is a separate, explicit action. No tray refresh — it's called
    /// rapidly while dragging, so the tone is taken from the cache (no sidecar read).
    pub fn apply_config(&self, config: &Config) -> Result<(), String> {
        let tone = self.tone_cache();
        self.manager()?
            .apply_config(config, &tone)
            .map_err(|e| e.to_string())?;
        let mut inner = self.inner.lock().unwrap();
        inner.bypassed = false; // a live edit writes real filters
        inner.active = Active::Unknown; // unsaved edits may match no preset
        Ok(())
    }

    pub fn categories(&self) -> Result<BTreeMap<String, Category>, String> {
        self.manager()?.categories().map_err(|e| e.to_string())
    }

    pub fn set_category(&self, name: &str, category: Option<Category>) -> Result<(), String> {
        self.manager()?
            .set_category(name, category)
            .map_err(|e| e.to_string())
    }

    /// The cached tone overlay (the sidecar is its persistence layer).
    pub fn tone(&self) -> Result<Tone, String> {
        Ok(self.tone_cache())
    }

    /// Update the global tone overlay, persist it, re-apply it to the live
    /// config, and refresh the cache.
    pub fn set_tone(&self, tone: &Tone) -> Result<(), String> {
        self.manager()?.set_tone(tone).map_err(|e| e.to_string())?;
        self.inner.lock().unwrap().tone = *tone;
        Ok(())
    }

    pub fn import_peace_presets(&self) -> Result<ImportReport, String> {
        let report = self
            .manager()?
            .import_presets_from_config_dir()
            .map_err(|e| e.to_string())?;
        self.invalidate_active();
        Ok(report)
    }

    pub fn import_peace_files(&self, paths: Vec<String>) -> Result<ImportReport, String> {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let report = self
            .manager()?
            .import_peace_files(&paths)
            .map_err(|e| e.to_string())?;
        self.invalidate_active();
        Ok(report)
    }
}
