//! "Start with Windows": the entry Task Manager lists under **Startup apps**.
//!
//! Backed by `tauri-plugin-autostart`, which on Windows writes the app's exe
//! path (plus our launch flag) to the per-user
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` key — the same key
//! Task Manager's Startup apps tab reads. Per-user, so it needs no admin
//! rights. (The MSI does not remove the value on uninstall; a stale entry is
//! inert, and Task Manager lets the user delete it.)
//!
//! Like the rest of the app surface, the frontend goes through our own IPC
//! commands rather than the plugin's JS API, so the plugin needs no extra
//! capability entry.

use tauri::{AppHandle, Runtime};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

/// Passed to the exe by the Run-key entry. fastpeq lives in the tray, so an
/// autostarted launch has no business stealing the foreground at login — it
/// starts with the window hidden (see [`launched_minimized`]).
pub const MINIMIZED_FLAG: &str = "--minimized";

/// The plugin, configured to register the entry with [`MINIMIZED_FLAG`].
pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    // The macOS launcher is ignored off macOS; LaunchAgent is the default there.
    tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![MINIMIZED_FLAG]))
}

/// Whether this process was started by the autostart entry (or by anything else
/// passing the flag), i.e. whether to keep the main window hidden at launch.
pub fn launched_minimized() -> bool {
    args_are_minimized(std::env::args().skip(1))
}

/// The same test against an explicit argument list — the single-instance
/// handler is given the *second* process's argv, not this one's.
pub fn args_are_minimized<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|a| a.as_ref() == MINIMIZED_FLAG)
}

/// Whether the startup entry is currently registered *and* enabled. Windows
/// keeps a separate per-entry on/off switch — the one Task Manager's Startup
/// apps tab flips — so an entry disabled there reads back as off here and the
/// settings toggle agrees with what Task Manager shows.
pub fn is_enabled(app: &AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

/// Register or remove the startup entry.
pub fn set(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    match result {
        Ok(()) => Ok(()),
        // Removing an entry that is already gone (deleted by hand, or by another
        // copy of the app) surfaces as a registry not-found error. The requested
        // state is the one we're in, so don't report that as a failure.
        Err(e) if is_enabled(app).is_ok_and(|live| live == enabled) => {
            eprintln!("fastpeq: autostart already {enabled}: {e}");
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_the_minimized_flag_among_other_args() {
        assert!(args_are_minimized(["--minimized"]));
        assert!(args_are_minimized(["--other", MINIMIZED_FLAG]));
    }

    #[test]
    fn plain_launches_are_not_minimized() {
        assert!(!args_are_minimized(Vec::<String>::new()));
        assert!(!args_are_minimized(["--minimize", "minimized"]));
    }
}
