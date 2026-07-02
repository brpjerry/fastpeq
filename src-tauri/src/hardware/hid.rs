//! Thin Windows USB HID layer over the `hidapi` crate: enumerate devices and open
//! one by path. Used by the device drivers in this module.
//!
//! `hidapi`'s context is process-global — creating a second `HidApi` while one is
//! live errors, and dropping it invalidates every open device. So we keep a single
//! `HidApi` for the whole process behind a mutex (never dropped), serialize
//! enumeration/open through it, and hand back owned `HidDevice` handles that stay
//! valid for as long as the program runs. Device I/O (read/write) happens on the
//! handle itself and needs no lock.

use super::DeviceInfo;
use hidapi::{HidApi, HidDevice};
use std::ffi::CString;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// The one process-wide `HidApi`. Initialized once (errors propagated), never
/// dropped, so open `HidDevice`s remain valid for the process lifetime.
fn api() -> Result<&'static Mutex<HidApi>, String> {
    static API: OnceLock<Mutex<HidApi>> = OnceLock::new();
    if let Some(m) = API.get() {
        return Ok(m);
    }
    // Serialize first-time construction: `HidApi::new` must not run concurrently.
    static INIT: Mutex<()> = Mutex::new(());
    let _g = INIT.lock().map_err(|e| e.to_string())?;
    if API.get().is_none() {
        let api = HidApi::new().map_err(|e| e.to_string())?;
        let _ = API.set(Mutex::new(api));
    }
    Ok(API.get().unwrap())
}

fn lock() -> Result<MutexGuard<'static, HidApi>, String> {
    api()?.lock().map_err(|e| e.to_string())
}

/// Enumerate every HID interface/collection currently present. Refreshes first so
/// a just-plugged device shows up.
pub(super) fn enumerate() -> Result<Vec<DeviceInfo>, String> {
    let mut guard = lock()?;
    guard.refresh_devices().map_err(|e| e.to_string())?;
    Ok(guard
        .device_list()
        .map(|d| DeviceInfo {
            vendor_id: d.vendor_id(),
            product_id: d.product_id(),
            product: d.product_string().unwrap_or_default().to_string(),
            manufacturer: d.manufacturer_string().unwrap_or_default().to_string(),
            path: d.path().to_string_lossy().into_owned(),
            usage_page: d.usage_page(),
        })
        .collect())
}

/// Open the HID interface at `path` (an id from [`enumerate`]). The returned
/// handle outlives the lock — it stays valid because the global `HidApi` is never
/// dropped.
pub(super) fn open(path: &str) -> Result<HidDevice, String> {
    let guard = lock()?;
    let cpath = CString::new(path).map_err(|e| e.to_string())?;
    guard.open_path(&cpath).map_err(|e| e.to_string())
}
