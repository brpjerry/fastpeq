//! The system tray: shows the currently active preset, plus bypass / open /
//! quit. Switching presets happens in the window — a full list of every preset
//! here would be unwieldy once a library grows past a handful.

use crate::state::AppState;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

pub const TRAY_ID: &str = "fastpeq-tray";
const CHANGED_EVENT: &str = "fastpeq:changed";

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    let icon = app
        .default_window_icon()
        .cloned()
        .expect("bundle defines a default window icon");

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("fastpeq")
        .menu(&menu)
        // Right-click opens the menu; left-click opens the window instead.
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Bring the main window to the front (used by the tray's left-click and the
/// "Open fastpeq" menu item).
fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Rebuild the tray menu so it reflects current state. Does *not* notify the
/// frontend: command wrappers call this and the frontend updates its own UI, so
/// emitting here would trigger a redundant full reload after every action.
pub fn refresh(app: &AppHandle) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let menu = build_menu(app)?;
        tray.set_menu(Some(menu))?;
    }
    Ok(())
}

/// Tell the frontend to reload. Only for state changes the frontend *didn't*
/// initiate — i.e. the tray menu and the global hotkey — so it can sync.
pub fn notify_changed(app: &AppHandle) {
    let _ = app.emit(CHANGED_EVENT, ());
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<AppState>();
    let menu = Menu::new(app)?;

    // A single, non-clickable line showing what's currently active. Switching is
    // done from the window, so we don't list the whole library here.
    let label = if state.is_bypassed() {
        "Bypassed".to_string()
    } else {
        state
            .active()
            .unwrap_or_else(|| "No preset active".to_string())
    };
    menu.append(&MenuItem::with_id(
        app,
        "active",
        label,
        false,
        None::<&str>,
    )?)?;

    menu.append(&PredefinedMenuItem::separator(app)?)?;
    let bypass = CheckMenuItem::with_id(
        app,
        "bypass",
        "Bypass (flat)",
        true,
        state.is_bypassed(),
        None::<&str>,
    )?;
    menu.append(&bypass)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "show",
        "Open fastpeq",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        "Quit fastpeq",
        true,
        None::<&str>,
    )?)?;

    Ok(menu)
}

fn on_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let state = app.state::<AppState>();
    match event.id().as_ref() {
        "quit" => app.exit(0),
        "show" => show_main(app),
        "bypass" => {
            let _ = state.toggle_bypass();
            let _ = refresh(app);
            notify_changed(app); // tray-initiated, so sync the window
        }
        _ => {} // the "active" label is disabled and fires nothing
    }
}
