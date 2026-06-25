//! Dynamic global-hotkey registration. The frontend owns the bindings (which
//! action each combo triggers and the user's editable list); the backend just
//! (re)registers the accelerators and, when one fires, emits `hotkey-pressed`
//! with the binding id so the UI can perform the action.

use std::str::FromStr;
use std::sync::Mutex;

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// Live map of registered shortcut → binding id, read by the press handler.
#[derive(Default)]
pub struct HotkeyMap(pub Mutex<Vec<(Shortcut, String)>>);

#[derive(Deserialize)]
pub struct Binding {
    pub id: String,
    pub accelerator: String,
}

/// Map a single key (`A`–`Z` / `0`–`9`) to its `Code`.
fn key_to_code(key: &str) -> Option<Code> {
    let mut chars = key.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None; // more than one character
    }
    let name = match c {
        'A'..='Z' => format!("Key{c}"),
        '0'..='9' => format!("Digit{c}"),
        _ => return None,
    };
    Code::from_str(&name).ok()
}

/// Parse the frontend's `"Ctrl+Alt+X"` / `"Ctrl+Shift+X"` accelerator into a
/// `Shortcut`. Deliberately strict about the format we emit, rather than relying
/// on a general accelerator parser.
fn parse(accel: &str) -> Option<Shortcut> {
    let mut mods = Modifiers::empty();
    let mut code: Option<Code> = None;
    for part in accel.split('+') {
        match part {
            "Ctrl" | "Control" => mods |= Modifiers::CONTROL,
            "Alt" => mods |= Modifiers::ALT,
            "Shift" => mods |= Modifiers::SHIFT,
            key => code = key_to_code(key),
        }
    }
    code.map(|c| Shortcut::new(Some(mods), c))
}

/// Re-register every global hotkey from `bindings`. Returns the ids that failed —
/// an unparseable accelerator, or a combo already grabbed by another app.
pub fn set_hotkeys(app: &AppHandle, bindings: Vec<Binding>) -> Vec<String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut map: Vec<(Shortcut, String)> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    for b in bindings {
        match parse(&b.accelerator) {
            Some(sc) if gs.register(sc).is_ok() => map.push((sc, b.id)),
            _ => failed.push(b.id),
        }
    }
    if let Ok(mut guard) = app.state::<HotkeyMap>().0.lock() {
        *guard = map;
    }
    failed
}

/// Plugin handler: on a key press, emit `hotkey-pressed` with the matching id.
pub fn on_event(app: &AppHandle, shortcut: &Shortcut, state: ShortcutState) {
    if state != ShortcutState::Pressed {
        return;
    }
    let id = app
        .state::<HotkeyMap>()
        .0
        .lock()
        .ok()
        .and_then(|guard| guard.iter().find(|(sc, _)| sc == shortcut).map(|(_, id)| id.clone()));
    if let Some(id) = id {
        let _ = app.emit("hotkey-pressed", id);
    }
}
