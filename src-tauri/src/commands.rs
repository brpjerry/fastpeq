//! IPC commands invoked from the Svelte frontend. Each is a thin wrapper that
//! delegates to [`AppState`] and refreshes the tray when state changes.

use crate::state::{ApoStatus, AppState, HardwareStatus};
use crate::tray;
use fastpeq_core::{Category, Config, ImportReport, OffloadMode, Tone};
use std::collections::BTreeMap;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub fn apo_status(state: State<'_, AppState>) -> ApoStatus {
    state.status()
}

#[tauri::command]
pub fn list_presets(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state.list_presets()
}

#[tauri::command]
pub fn active_preset(state: State<'_, AppState>) -> Option<String> {
    // Re-derive from the live config so a change made outside the app (or the
    // tray/hotkey) is reflected; this also refreshes the cache the tray reads.
    state.redetect_active()
}

#[tauri::command]
pub fn apply_preset(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    state.apply(&name)?;
    let _ = tray::refresh(&app);
    Ok(())
}

#[tauri::command]
pub fn toggle_bypass(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.toggle_bypass()?;
    let _ = tray::refresh(&app);
    Ok(())
}

#[tauri::command]
pub fn bypassed(state: State<'_, AppState>) -> bool {
    state.is_bypassed()
}

#[tauri::command]
pub fn capture_current(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    state.capture(&name)?;
    let _ = tray::refresh(&app);
    Ok(())
}

/// Delete a preset. Resolves to the id of its `delete` history revision (the
/// undo handle), or `null` when nothing could be snapshotted.
#[tauri::command]
pub fn delete_preset(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<Option<String>, String> {
    let revision = state.delete(&name)?;
    let _ = tray::refresh(&app);
    Ok(revision)
}

#[tauri::command]
pub fn rename_preset(
    app: AppHandle,
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> Result<(), String> {
    state.rename(&from, &to)?;
    let _ = tray::refresh(&app);
    Ok(())
}

/// Restore a preset-history revision into the preset file — the undo-delete
/// path, and later the history browser's Restore.
#[tauri::command]
pub fn restore_revision(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    id: String,
) -> Result<(), String> {
    state.restore_revision(&name, &id)?;
    let _ = tray::refresh(&app);
    Ok(())
}

/// A preset's history revisions, newest first (the history browser's list).
#[tauri::command]
pub fn preset_history(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<fastpeq_core::Revision>, String> {
    state.preset_history(&name)
}

/// Revision counts per preset name — the preset list's version badges (a
/// preset's current content is version `count + 1`).
#[tauri::command]
pub fn preset_versions(state: State<'_, AppState>) -> Result<BTreeMap<String, usize>, String> {
    state.preset_versions()
}

/// Name (or clear, with an empty string) a history revision — the tag shown
/// after "vX" in the history menu, carried as a comment with the content.
#[tauri::command]
pub fn set_revision_tag(
    state: State<'_, AppState>,
    name: String,
    id: String,
    tag: String,
) -> Result<(), String> {
    state.set_revision_tag(&name, &id, &tag)
}

/// Hide a history revision without deleting its snapshot file. It moves into
/// the preset library's `.history/.deleted` archive instead.
#[tauri::command]
pub fn delete_revision(state: State<'_, AppState>, name: String, id: String) -> Result<(), String> {
    state.delete_revision(&name, &id)
}

/// One history revision, parsed for the browser's preview/audition.
#[tauri::command]
pub fn get_revision(
    state: State<'_, AppState>,
    name: String,
    id: String,
) -> Result<Config, String> {
    state.get_revision(&name, &id)
}

#[tauri::command]
pub fn get_preset(state: State<'_, AppState>, name: String) -> Result<Config, String> {
    state.load_config(&name)
}

#[tauri::command]
pub fn save_preset(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    config: Config,
) -> Result<(), String> {
    state.save_config(&name, &config)?;
    let _ = tray::refresh(&app);
    Ok(())
}

/// Live preview. `pregain` (dB, `≤ 0`), when present, is the hardware device's
/// pregain set by the editor's hardware preamp slider; `null` keeps the automatic
/// pregain (and lets Min. APO preamp mode recompute the APO preamp).
#[tauri::command]
pub fn apply_live(
    state: State<'_, AppState>,
    config: Config,
    pregain: Option<f64>,
    commit: Option<bool>,
) -> Result<(), String> {
    state.apply_config(&config, pregain, commit.unwrap_or(false))
}

#[tauri::command]
pub fn get_tone(state: State<'_, AppState>) -> Result<Tone, String> {
    state.tone()
}

#[tauri::command]
pub fn set_tone(state: State<'_, AppState>, tone: Tone) -> Result<(), String> {
    state.set_tone(&tone)
}

#[tauri::command]
pub fn preset_categories(state: State<'_, AppState>) -> Result<BTreeMap<String, Category>, String> {
    state.categories()
}

#[tauri::command]
pub fn set_category(
    state: State<'_, AppState>,
    name: String,
    category: Option<Category>,
) -> Result<(), String> {
    state.set_category(&name, category)
}

#[tauri::command]
pub fn import_peace_presets(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ImportReport, String> {
    let report = state.import_peace_presets()?;
    let _ = tray::refresh(&app);
    Ok(report)
}

#[tauri::command]
pub fn import_peace_files(
    app: AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<ImportReport, String> {
    let report = state.import_peace_files(paths)?;
    let _ = tray::refresh(&app);
    Ok(report)
}

/// Read a user-picked text file (e.g. a REW measurement export) so the UI can
/// parse it. The path comes from the file dialog the user just confirmed; we
/// still verify it's a regular file and cap the size as defense-in-depth.
#[tauri::command]
pub fn read_text_file(path: String) -> Result<String, String> {
    const MAX_BYTES: u64 = 32 * 1024 * 1024; // measurements are small text files
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("Not a regular file".to_string());
    }
    if meta.len() > MAX_BYTES {
        return Err("File is too large".to_string());
    }
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn presets_dir(state: State<'_, AppState>) -> String {
    state.presets_dir()
}

#[tauri::command]
pub fn set_presets_dir(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    state.set_presets_dir(&path)?;
    let _ = tray::refresh(&app);
    Ok(())
}

#[tauri::command]
pub fn reset_presets_dir(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.reset_presets_dir()?;
    let _ = tray::refresh(&app);
    Ok(())
}

#[tauri::command]
pub fn open_presets_dir(state: State<'_, AppState>) -> Result<(), String> {
    state.open_presets_dir()
}

/// (Re)register the global hotkeys from the frontend's binding list. Returns the
/// ids that couldn't be registered (bad accelerator or combo already in use).
#[tauri::command]
pub fn set_hotkeys(app: AppHandle, bindings: Vec<crate::hotkeys::Binding>) -> Vec<String> {
    crate::hotkeys::set_hotkeys(&app, bindings)
}

/// The persisted hotkey bindings (an opaque JSON document owned by the
/// frontend's hotkeys store), or `null` when none have been saved yet. Stored
/// as `hotkeys.json` in the app data dir — a real file with atomic writes, not
/// WebView localStorage, so the bindings survive webview profile loss.
#[tauri::command]
pub fn load_hotkey_bindings(state: State<'_, AppState>) -> Option<String> {
    state.hotkey_bindings()
}

/// Persist the hotkey bindings JSON (see [`load_hotkey_bindings`]).
#[tauri::command]
pub fn save_hotkey_bindings(state: State<'_, AppState>, json: String) -> Result<(), String> {
    state.set_hotkey_bindings(&json)
}

/// A persisted UI state document (an opaque JSON document owned by one of the
/// frontend's stores — preset view state, targets, prefs, theme), or `null`
/// when it has never been saved. Stored as `<key>.json` in the app data dir —
/// a real file with atomic writes, not WebView localStorage, so the data
/// survives webview profile loss (same rationale as the hotkey bindings).
#[tauri::command]
pub fn load_ui_state(state: State<'_, AppState>, key: String) -> Result<Option<String>, String> {
    state.ui_state(&key)
}

/// Persist a UI state document (see [`load_ui_state`]).
#[tauri::command]
pub fn save_ui_state(state: State<'_, AppState>, key: String, json: String) -> Result<(), String> {
    state.set_ui_state(&key, &json)
}

/// List the system's audio output devices (for the "switch output device" hotkey
/// principal picker). Stateless OS query; doesn't touch [`AppState`].
#[tauri::command]
pub fn list_audio_devices() -> Result<Vec<crate::audio::AudioDevice>, String> {
    crate::audio::list_devices()
}

/// Make the given audio endpoint the default output device.
#[tauri::command]
pub fn set_default_audio_device(id: String) -> Result<(), String> {
    crate::audio::set_default(&id)
}

/// List supported hardware-EQ devices currently connected (for the hardware panel).
/// Async + `spawn_blocking`: HID enumeration takes ~1 s, so it must not run on the
/// UI thread (where synchronous commands execute).
#[tauri::command]
pub async fn list_hardware_devices() -> Result<Vec<fastpeq_hw::DetectedDevice>, String> {
    tauri::async_runtime::spawn_blocking(fastpeq_hw::detect)
        .await
        .map_err(|e| e.to_string())?
}

/// The current hardware-offload status (enabled device, firmware, errors, mode).
/// A cheap read — does not reconcile with the active output (see `refresh_hardware`).
#[tauri::command]
pub fn hardware_status(state: State<'_, AppState>) -> HardwareStatus {
    state.hardware_status()
}

/// Reconcile offload with the active output device, then return the fresh status.
/// The frontend calls this on demand — window focus, opening the panel, a mode
/// change, or after switching output. Output changes made *outside* fastpeq are
/// caught by the backend's OS watcher (`audio::watch_default_output`), so there
/// is no polling anywhere; this command doubles as the belt-and-braces resync.
/// The reconcile runs off the UI thread (its HID enumeration takes ~1 s).
#[tauri::command]
pub async fn refresh_hardware(app: AppHandle) -> Result<HardwareStatus, String> {
    let sync_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(state) = sync_app.try_state::<AppState>() {
            state.sync_offload();
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(app.state::<AppState>().hardware_status())
}

/// Which of `config`'s filters (positions in document order) are currently sent to
/// hardware — for the editor's per-band indicator. Empty when offload is off.
#[tauri::command]
pub fn offload_selection(state: State<'_, AppState>, config: Config) -> Vec<usize> {
    state.offload_selection(&config)
}

/// Set how bands are selected for hardware offload. Refreshes the tray since the
/// active-preset display can change.
#[tauri::command]
pub fn set_offload_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: OffloadMode,
) -> Result<(), String> {
    state.set_offload_mode(mode)?;
    let _ = tray::refresh(&app);
    Ok(())
}

/// Present the calling OSD window without activation, on the current virtual
/// desktop and at the top of the z-order. This is deliberately one native
/// operation: Tauri's frontend `show()` promise resolves before TAO necessarily
/// applies the Win32 visibility change, which made the old follow-up desktop
/// check race against a still-hidden HWND (docs/OSD_OVERLAY_BUG.md).
#[tauri::command]
pub fn osd_present(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(windows)]
    let result = crate::overlay::present_on_current_desktop(&window);
    #[cfg(not(windows))]
    let result = window
        .show()
        .and_then(|_| window.set_always_on_top(true))
        .map_err(|e| e.to_string());

    if let Err(e) = &result {
        eprintln!("fastpeq: OSD presentation failed: {e}");
    }
    result
}
