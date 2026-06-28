//! IPC commands invoked from the Svelte frontend. Each is a thin wrapper that
//! delegates to [`AppState`] and refreshes the tray when state changes.

use crate::state::{ApoStatus, AppState};
use crate::tray;
use fastpeq_core::{Category, Config, ImportReport, Tone};
use std::collections::BTreeMap;
use tauri::{AppHandle, State};

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

#[tauri::command]
pub fn delete_preset(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    state.delete(&name)?;
    let _ = tray::refresh(&app);
    Ok(())
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

#[tauri::command]
pub fn apply_live(state: State<'_, AppState>, config: Config) -> Result<(), String> {
    state.apply_config(&config)
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
