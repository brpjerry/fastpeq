//! Shared application state: the core [`Manager`] plus the currently active
//! preset, behind a mutex. All operations funnel through here so the tray, the
//! hotkey, and the IPC commands stay consistent.

use crate::audio;
use crate::hardware::{self, DetectedDevice, HardwareSession};
use fastpeq_core::apo::env;
use fastpeq_core::{
    Category, Config, ImportReport, Manager as CoreManager, OffloadMode, PresetStore, Tone,
    offload, provenance,
};
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

/// Reported to the UI's hardware panel.
#[derive(Serialize, Clone, Default)]
pub struct HardwareStatus {
    /// The global toggle: offload to the active output when it supports it.
    pub enabled: bool,
    /// Whether EQ is *currently* being offloaded (the active output is a supported
    /// device and the toggle is on). `false` when the toggle is on but the active
    /// output isn't a device we can offload to.
    pub active: bool,
    /// The device currently being offloaded to (when `active`).
    pub device: Option<DetectedDevice>,
    /// Device firmware version, once read.
    pub version: Option<String>,
    /// The last worker error (e.g. the device was unplugged), if any.
    pub error: Option<String>,
    /// How many bands run on the device — the "first X" sent to hardware.
    pub max_filters: Option<usize>,
    /// Which strategy picks the bands sent to hardware (persisted preference).
    pub mode: OffloadMode,
}

/// Persisted backend settings, read at startup before the WebView exists (so it
/// can't live in the frontend's localStorage). Stored as `settings.json` in the
/// app data dir.
#[derive(Serialize, Deserialize, Default)]
struct Settings {
    /// Custom preset storage directory; `None` uses the default under app data.
    presets_dir: Option<String>,
    /// The EQ-routing mode (offload off / which bands go to hardware); `None` = the
    /// default (`ApoOnly`, offload off).
    #[serde(default)]
    offload_mode: Option<OffloadMode>,
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
                last_full: None,
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
            last_full: None,
        },
    }
}

pub struct AppState {
    data_dir: PathBuf,
    inner: Mutex<Inner>,
    /// The active hardware-EQ offload connection, if any. Kept separate from
    /// [`Inner`] (the core EQ/preset state) so it survives a presets-dir rebuild
    /// and so its lock never nests with the manager. `None` means offload is off.
    hardware: Mutex<Option<HardwareSession>>,
    /// Which strategy picks the bands sent to hardware. Persisted in settings.json;
    /// applies to every offload write (tray/hotkey/UI), so it lives here.
    offload_mode: Mutex<OffloadMode>,
    /// Cache for [`sync_offload`]: the `(offload_enabled, default-output-name)` the
    /// session was last reconciled against. When unchanged, sync skips the expensive
    /// HID enumeration, so an on-demand reconcile is near-free in steady state.
    last_sync: Mutex<Option<(bool, Option<String>)>>,
    /// Serializes [`sync_offload`]: reconciles run off the UI thread and can be
    /// requested from several places at once (focus, mode change, …); this makes an
    /// overlapping request a no-op instead of racing the session open/close.
    sync_guard: Mutex<()>,
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
    /// While hardware offload is active, the most recent *full* (un-split) base EQ
    /// applied. The live `config.txt` only holds the software remainder, so this is
    /// what bypass captures and un-bypass restores (across both stages). `None`
    /// when offload is off.
    last_full: Option<Config>,
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
        let settings = load_settings(&data_dir);
        let presets_dir = settings
            .presets_dir
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("presets"));
        let offload_mode = settings.offload_mode.unwrap_or_default();
        let inner = build_inner(&data_dir, presets_dir);
        let state = AppState {
            data_dir,
            inner: Mutex::new(inner),
            hardware: Mutex::new(None),
            offload_mode: Mutex::new(offload_mode),
            last_sync: Mutex::new(None),
            sync_guard: Mutex::new(()),
        };
        Ok(state)
    }

    /// Reconcile the live offload session with the active output: open a session
    /// (and offload the current EQ) when the active output becomes a supported
    /// device, or close it (restoring the full EQ to software) when it stops being
    /// one.
    ///
    /// Called on demand — startup, focus, a mode change, an output switch — never on
    /// a timer, and always off the UI thread (the HID enumeration takes ~1 s). It
    /// first checks a cached `(enabled, default-output-name)` key via the cheap
    /// `default_output_name()` and returns early when nothing changed, so most calls
    /// cost ~a few ms.
    pub fn sync_offload(&self) {
        // One reconcile at a time — overlapping requests (e.g. focus + mode change)
        // just no-op rather than racing the session open/close.
        let Ok(_guard) = self.sync_guard.try_lock() else {
            return;
        };
        let enabled = self.offload_enabled();
        let output_name = if enabled {
            audio::default_output_name()
        } else {
            None
        };
        // Skip the costly work when neither the on/off state nor the active output
        // has changed since we last reconciled.
        {
            let mut last = self.last_sync.lock().unwrap();
            let key = (enabled, output_name.clone());
            if last.as_ref() == Some(&key) {
                return;
            }
            *last = Some(key);
        }

        // The output (or on/off state) changed — resolve the supported device for it
        // (this is the part that enumerates HID) and reconcile the session.
        let target = output_name.as_deref().and_then(hardware::device_for_output);
        let current = self
            .hardware
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.descriptor.id.clone());
        match (&target, &current) {
            (Some(t), Some(c)) if &t.id == c => return, // already on the right device
            (None, None) => return,                     // already off
            _ => {}
        }

        // The full (un-split) EQ to re-apply to whichever stage(s) now apply.
        let full = self.full_eq();

        // Clear and stop the outgoing session (if any) so a device we're leaving
        // doesn't keep applying EQ.
        let prev = self.hardware.lock().unwrap().take();
        if let Some(prev) = prev {
            let _ = prev.push_live(Vec::new(), 0.0);
            prev.stop();
        }

        match target {
            Some(dev) => match hardware::profile(&dev.id) {
                Ok(profile) => {
                    *self.hardware.lock().unwrap() = Some(HardwareSession::start(dev, profile));
                    // Push the current EQ to the new device (RAM only — following the
                    // output shouldn't wear the device's flash) and write the remainder.
                    if let Some(full) = &full
                        && self.offload_apply(full, None, false, None).unwrap_or(false)
                    {
                        self.inner.lock().unwrap().last_full = Some(full.clone());
                    }
                }
                Err(_) => {
                    // The device vanished (or HID glitched) between detection and
                    // open. Clear the sync cache so the next reconcile retries,
                    // instead of treating this output as already handled.
                    *self.last_sync.lock().unwrap() = None;
                }
            },
            None => {
                // No longer offloading: put the full EQ back into software.
                if let Some(full) = &full {
                    let tone = self.tone_cache();
                    if let Ok(m) = self.manager() {
                        let _ = m.apply_config(full, &tone);
                    }
                }
                self.inner.lock().unwrap().last_full = None;
            }
        }
        self.invalidate_active();
    }

    /// The current full (un-split) EQ: the cached `last_full` while offloading,
    /// otherwise reconstructed from the active preset's stamp, otherwise the live
    /// `config.txt` as-is.
    fn full_eq(&self) -> Option<Config> {
        if let Some(full) = self.inner.lock().unwrap().last_full.clone() {
            return Some(full);
        }
        let manager = self.manager().ok()?;
        if let Ok(Some(name)) = manager.active_preset_by_stamp()
            && let Ok(cfg) = manager.load_preset(&name)
        {
            return Some(provenance::set(&cfg, &name));
        }
        manager.current_config().ok()
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
        let derived = self.derive_active();
        self.inner.lock().unwrap().active = Active::Known(derived.clone());
        derived
    }

    /// Compute the active preset from the live config — by provenance stamp alone
    /// while a session is offloading (the offloaded bands are gone from `config.txt`,
    /// so the stricter equivalence check can't match the full preset), otherwise the
    /// normal check. `sync_offload` keeps the session in step with the active output,
    /// so `hardware_active` accurately means "the live config is a remainder".
    fn derive_active(&self) -> Option<String> {
        let manager = self.manager().ok()?;
        if self.hardware_active() {
            manager.active_preset_by_stamp().ok().flatten()
        } else {
            manager.active_preset().ok().flatten()
        }
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
        let manager = self.manager()?;
        let config = manager.load_preset(name).map_err(|e| e.to_string())?;
        // Offload the first X bands to hardware if active; otherwise full software.
        // Preset apply has no sliders → automatic pregain.
        let offloaded = self.offload_apply(&config, Some(name), true, None)?;
        if !offloaded {
            let tone = self.tone_cache();
            manager
                .apply_preset(name, &tone)
                .map_err(|e| e.to_string())?;
        }
        let mut inner = self.inner.lock().unwrap();
        // Remember the stamped full base so a later bypass/un-bypass restores it.
        inner.last_full = offloaded.then(|| provenance::set(&config, name));
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
        let offloaded = self.hardware_active();
        if bypassed {
            // Un-bypass: re-apply the exact base that was live before bypassing,
            // restoring both the software and (if offloaded) the hardware bands.
            if let Some(base) = &restore
                && (!offloaded || !self.offload_apply(base, None, true, None)?)
            {
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
            // Capture the full base EQ (tone stripped) so un-bypass restores it
            // exactly — including unsaved edits — then drop the filters from both
            // the software config and (if offloaded) the device.
            let full = if offloaded {
                let captured = self.inner.lock().unwrap().last_full.clone();
                self.clear_hardware_eq(); // flatten the device's bands
                manager.bypass().map_err(|e| e.to_string())?; // drop software filters
                captured.unwrap_or_else(|| manager.base_config().unwrap_or_default())
            } else {
                let base = manager.base_config().map_err(|e| e.to_string())?;
                manager.bypass().map_err(|e| e.to_string())?;
                base
            };
            let mut inner = self.inner.lock().unwrap();
            inner.bypassed = true;
            inner.restore = Some(full);
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
    pub fn apply_config(&self, config: &Config, device_pregain: Option<f64>) -> Result<(), String> {
        let offloaded = self.offload_apply(config, None, false, device_pregain)?;
        if !offloaded {
            let tone = self.tone_cache();
            self.manager()?
                .apply_config(config, &tone)
                .map_err(|e| e.to_string())?;
        }
        let mut inner = self.inner.lock().unwrap();
        if offloaded {
            inner.last_full = Some(config.clone());
        }
        inner.bypassed = false; // a live edit writes real filters
        inner.active = Active::Unknown; // unsaved edits may match no preset
        Ok(())
    }

    // --- Hardware EQ offload ---------------------------------------------------

    /// The current offload status for the UI's hardware panel. A cheap read — the
    /// session is reconciled with the active output by the background reconciler, not
    /// here (this is polled from the UI thread).
    pub fn hardware_status(&self) -> HardwareStatus {
        let enabled = self.offload_enabled();
        let mode = self.offload_mode();
        let hw = self.hardware.lock().unwrap();
        match hw.as_ref() {
            Some(session) => {
                let rt = session.status();
                HardwareStatus {
                    enabled,
                    // A session can briefly outlive a turn-off until the reconciler
                    // closes it — it's only "active" while offload is still enabled.
                    active: enabled,
                    device: Some(session.descriptor.clone()),
                    version: rt.version,
                    error: rt.error,
                    max_filters: Some(session.profile.max_filters),
                    mode,
                }
            }
            None => HardwareStatus {
                enabled,
                active: false,
                mode,
                ..Default::default()
            },
        }
    }

    /// The current band-selection mode for hardware offload.
    pub fn offload_mode(&self) -> OffloadMode {
        *self.offload_mode.lock().unwrap()
    }

    /// Set the EQ-routing mode (the panel's 5-way control), persist it, and apply.
    /// `ApoOnly` turns offload off; the others turn it on.
    ///
    /// Opening/closing the device session needs HID enumeration (~1 s), so that's
    /// left to the background reconciler (off the UI thread) — we just invalidate its
    /// cache. A change *between* offloading modes only re-splits, which is cheap, so
    /// we do it inline for an immediate result.
    pub fn set_offload_mode(&self, mode: OffloadMode) -> Result<(), String> {
        // Persist first: if the settings write fails, the in-memory mode stays
        // untouched, so the UI never shows a mode that won't survive a restart.
        let mut settings = load_settings(&self.data_dir);
        settings.offload_mode = Some(mode);
        save_settings(&self.data_dir, &settings).map_err(|e| e.to_string())?;
        *self.offload_mode.lock().unwrap() = mode;

        // Force the background reconciler to re-evaluate on/off + device next tick.
        *self.last_sync.lock().unwrap() = None;
        if self.hardware_active() {
            // Re-split the current EQ with the new mode (no HID enumeration).
            let cached = self.inner.lock().unwrap().last_full.clone();
            let full = match cached {
                Some(f) => f,
                None => self.manager()?.base_config().map_err(|e| e.to_string())?,
            };
            if self.offload_apply(&full, None, true, None)? {
                self.inner.lock().unwrap().last_full = Some(full);
            }
            self.invalidate_active();
        }
        Ok(())
    }

    /// Whether offload is on at all (any mode other than `ApoOnly`).
    fn offload_enabled(&self) -> bool {
        self.offload_mode() != OffloadMode::ApoOnly
    }

    fn hardware_active(&self) -> bool {
        self.hardware.lock().unwrap().is_some()
    }

    /// The positions (among `config`'s filters, in document order) that would be
    /// offloaded to the device right now — for the editor's per-band indicator.
    /// Empty when offload is off.
    pub fn offload_selection(&self, config: &Config) -> Vec<usize> {
        let mode = self.offload_mode();
        let hw = self.hardware.lock().unwrap();
        match hw.as_ref() {
            Some(session) => offload::selected_filter_positions(config, &session.profile, mode),
            None => Vec::new(),
        }
    }

    /// When hardware offload is active, split `full` (a base EQ), push the hardware
    /// bands to the worker, and write the software remainder to APO. Returns whether
    /// offload was active (and thus handled the write). `stamp` forces the
    /// provenance preset name (preset apply); `None` carries it like a live edit;
    /// `commit` saves the device's flash.
    ///
    /// If the worker has died (the channel send fails), the dead session is dropped
    /// and this returns `false`, so the caller falls back to writing the full EQ to
    /// software — the offloaded bands aren't lost.
    /// `device_pregain` (dB, `≤ 0`), when `Some`, overrides the auto device pregain
    /// and signals that the caller (the editor's two preamp sliders) owns both
    /// stages: the software master preamp in `full` is used as-is and the mode-3
    /// auto-recompute is skipped. `None` keeps the automatic behavior (used by the
    /// tray/preset/hotkey paths, which have no sliders).
    fn offload_apply(
        &self,
        full: &Config,
        stamp: Option<&str>,
        commit: bool,
        device_pregain: Option<f64>,
    ) -> Result<bool, String> {
        let mode = self.offload_mode();
        let software = {
            let hw = self.hardware.lock().unwrap();
            let Some(session) = hw.as_ref() else {
                return Ok(false);
            };
            let split = offload::split(full, &session.profile, mode);
            let pregain = device_pregain.unwrap_or(split.hw_pregain);
            let sent = if commit {
                session.push_commit(split.hw, pregain)
            } else {
                session.push_live(split.hw, pregain)
            };
            match sent {
                Ok(()) => Some(match stamp {
                    Some(name) => provenance::set(&split.software, name),
                    None => split.software,
                }),
                Err(_) => None, // worker gone — fall back below
            }
        };
        match software {
            Some(mut sw) => {
                let tone = self.tone_cache();
                // Minimize-preamp mode: set APO's master preamp to the auto-preamp
                // value over the remaining software bands *and* the tone overlay, so
                // what's left to APO can't clip while its preamp stays near 0 (the
                // boosts' headroom is on the device). Skipped when the editor supplies
                // an explicit pregain — then its APO slider already set the preamp.
                if mode == OffloadMode::MinimizePreamp && device_pregain.is_none() {
                    let preamp = offload::auto_preamp(&sw, &tone);
                    offload::set_master_preamp(&mut sw, preamp);
                }
                self.manager()?
                    .apply_config(&sw, &tone)
                    .map_err(|e| e.to_string())?;
                Ok(true)
            }
            None => {
                self.clear_hardware_session();
                Ok(false)
            }
        }
    }

    /// Push a flat band set to the device (clears its EQ) without stopping the
    /// worker. Used by bypass; a dead worker is ignored (status surfaces it).
    fn clear_hardware_eq(&self) {
        let hw = self.hardware.lock().unwrap();
        if let Some(session) = hw.as_ref() {
            let _ = session.push_commit(Vec::new(), 0.0);
        }
    }

    /// Stop and drop the worker (e.g. after a disconnect). Does not touch APO.
    fn clear_hardware_session(&self) {
        let prev = self.hardware.lock().unwrap().take();
        if let Some(prev) = prev {
            prev.stop();
        }
        // Force the next reconcile to re-evaluate (the session no longer matches the
        // cached target — e.g. the worker died but the output is unchanged).
        *self.last_sync.lock().unwrap() = None;
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
