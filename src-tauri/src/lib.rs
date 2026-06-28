//! The fastpeq Tauri application: a thin shell over `fastpeq-core`.
//!
//! All audio/config logic lives in the core; this crate only wires it to a
//! window, a system tray, a global hotkey, and the IPC commands the UI calls.

mod audio;
mod commands;
mod hotkeys;
mod state;
mod tray;

use state::AppState;
use tauri::Manager;

/// Recolor the native Windows title bar/border to match the app's dark theme,
/// so the window frame doesn't stick out as a light strip above a dark UI.
#[cfg(windows)]
mod titlebar {
    use core::ffi::c_void;
    use tauri::WebviewWindow;

    // DWM window attributes. Dark mode lands on Windows 10 1809+; the explicit
    // caption/border/text colors need Windows 11 and are ignored before that.
    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    const DWMWA_BORDER_COLOR: u32 = 34;
    const DWMWA_CAPTION_COLOR: u32 = 35;
    const DWMWA_TEXT_COLOR: u32 = 36;

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(hwnd: isize, attr: u32, value: *const c_void, size: u32) -> i32;
    }

    // COLORREF byte order is 0x00BBGGRR.
    const fn rgb(r: u8, g: u8, b: u8) -> u32 {
        (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
    }

    fn set_attr(hwnd: isize, attr: u32, value: u32) {
        // SAFETY: `hwnd` is a live window handle and the attribute payload is a
        // 4-byte DWORD, matching every attribute we set here.
        unsafe {
            DwmSetWindowAttribute(hwnd, attr, &value as *const u32 as *const c_void, 4);
        }
    }

    /// Colors mirror app.css: --bg (caption) / --border / --text, so the title
    /// bar blends into the window's background.
    pub fn apply(window: &WebviewWindow) {
        let Ok(hwnd) = window.hwnd() else {
            return;
        };
        let hwnd = hwnd.0 as isize;
        set_attr(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, 1); // light caption buttons
        set_attr(hwnd, DWMWA_CAPTION_COLOR, rgb(0x15, 0x17, 0x1c));
        set_attr(hwnd, DWMWA_BORDER_COLOR, rgb(0x2d, 0x32, 0x3c));
        set_attr(hwnd, DWMWA_TEXT_COLOR, rgb(0xe6, 0xe9, 0xef));
    }
}

/// Make the OSD overlay a non-activating tool window: showing it never steals
/// focus from whatever app is in the foreground, and it stays out of Alt-Tab.
/// The `focus: false` window config alone doesn't guarantee this across repeated
/// `show()` calls — the extended window styles do.
#[cfg(windows)]
mod overlay {
    use tauri::WebviewWindow;

    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_NOACTIVATE: isize = 0x0800_0000;
    const WS_EX_TOOLWINDOW: isize = 0x0000_0080;

    // GetWindowLongPtrW/SetWindowLongPtrW are the 64-bit-correct accessors; the
    // app builds for x64 MSVC, where user32 exports them directly.
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetWindowLongPtrW(hwnd: isize, index: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: isize, index: i32, new: isize) -> isize;
    }

    pub fn make_noactivate(window: &WebviewWindow) {
        let Ok(hwnd) = window.hwnd() else {
            return;
        };
        let hwnd = hwnd.0 as isize;
        // SAFETY: `hwnd` is a live top-level window handle; we read the current
        // extended style and OR in the no-activate / tool-window bits.
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Single instance: focus the existing window instead of opening a second.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        // Closing the window hides it to the tray rather than quitting, so the
        // tray stays a usable fast-switching surface with no window open. The
        // tray's "Quit fastpeq" is the real exit.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();

            // Match the native window frame to the app's dark theme.
            #[cfg(windows)]
            if let Some(window) = app.get_webview_window("main") {
                titlebar::apply(&window);
            }

            // The OSD overlay must never grab focus when it pops up.
            #[cfg(windows)]
            if let Some(osd) = app.get_webview_window("osd") {
                overlay::make_noactivate(&osd);
            }

            // Detect Equalizer APO and prepare the preset library.
            app.manage(AppState::initialize(&handle)?);

            // System tray with live preset switching.
            tray::build_tray(&handle)?;

            // Configurable global hotkeys: the frontend owns the bindings and
            // registers them via `set_hotkeys` once it mounts. The plugin handler
            // just emits `hotkey-pressed` with the binding id for the UI to act on.
            app.manage(hotkeys::HotkeyMap::default());
            let _ = handle.plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(|app, shortcut, event| {
                        hotkeys::on_event(app, shortcut, event.state());
                    })
                    .build(),
            );

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::apo_status,
            commands::list_presets,
            commands::active_preset,
            commands::apply_preset,
            commands::toggle_bypass,
            commands::bypassed,
            commands::capture_current,
            commands::delete_preset,
            commands::rename_preset,
            commands::get_preset,
            commands::save_preset,
            commands::apply_live,
            commands::get_tone,
            commands::set_tone,
            commands::preset_categories,
            commands::set_category,
            commands::import_peace_presets,
            commands::import_peace_files,
            commands::read_text_file,
            commands::presets_dir,
            commands::set_presets_dir,
            commands::reset_presets_dir,
            commands::open_presets_dir,
            commands::set_hotkeys,
            commands::list_audio_devices,
            commands::set_default_audio_device,
        ])
        .run(tauri::generate_context!())
        .expect("error while running fastpeq");
}
