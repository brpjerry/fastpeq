//! Hardware parametric-EQ devices: enumerating them and pushing EQ bands to them
//! over USB HID.
//!
//! Like output-device switching in [`crate::audio`], this is a platform/device
//! concern that lives in the shell, not in `fastpeq-core`. The pure split + biquad
//! math is in [`fastpeq_core::offload`]; this module is the I/O half.
//!
//! **Modularity.** Each supported device family is a *driver* (see [`moondrop`]).
//! A driver identifies its HID devices and opens them into a [`HardwareEq`].
//! Adding a device = add a driver module and register it in `drivers()`. The
//! non-Windows build provides stubs so the crate still compiles and tests stay
//! green off-Windows, mirroring [`crate::audio`] and `fastpeq_core::apo::env`.

use fastpeq_core::{HardwareProfile, HwBand};
use serde::Serialize;

/// A detected hardware-EQ device, as surfaced to the UI's hardware panel.
#[derive(Serialize, Clone, Debug)]
pub struct DetectedDevice {
    /// Opaque, session-stable id (the HID device path) used to open it later.
    pub id: String,
    /// Friendly name for display, e.g. `"MOONDROP DHA15"`.
    pub name: String,
    pub manufacturer: String,
    pub model: String,
    /// Band budget — how many of a preset's filters this device runs.
    pub max_filters: usize,
    /// Whether the device's pregain is host-adjustable (see
    /// [`HardwareProfile::user_pregain`]). The UI hides the Device preamp
    /// slider when it isn't.
    pub user_pregain: bool,
}

/// A connected hardware-EQ device that EQ bands can be pushed to. Implemented per
/// device family by a driver.
///
/// Not `Send` by design: the push worker opens its device on its own thread and
/// never moves the handle across threads (HID handles are thread-affine).
pub trait HardwareEq {
    /// Write `bands` to the device. A `bands` slice shorter than the band budget
    /// clears the remaining slots (flat). `pregain` (dB, `≤ 0`) is the input
    /// headroom. When `commit` is set, also persist to the device's flash so the EQ
    /// survives a power-cycle; otherwise the write is volatile (live preview).
    fn push(&mut self, bands: &[HwBand], pregain: f64, commit: bool) -> Result<(), String>;
    /// Read the bands currently on the device. Used by the hardware smoke test and
    /// reserved for a future device→app sync; not on the normal push path.
    #[allow(dead_code)]
    fn pull(&mut self) -> Result<Vec<HwBand>, String>;
    /// The device firmware version string (read as a connection handshake).
    fn version(&mut self) -> Result<String, String>;
}

#[cfg(windows)]
mod hid;
#[cfg(windows)]
mod moondrop;
#[cfg(windows)]
mod walkplay;
mod worker;

pub use worker::HardwareSession;

/// The detected device that corresponds to an audio output's friendly name — i.e.
/// the supported device whose model appears in the name (e.g. the output
/// `"DAC/Amp (Moondrop DHA15)"` → the DHA15). Used to offload to the *active output*
/// only when it's a device we actually support. `None` when the output is some other
/// device (or nothing supported is connected).
pub fn device_for_output(output_name: &str) -> Option<DetectedDevice> {
    let name = output_name.to_uppercase();
    detect()
        .ok()?
        .into_iter()
        .find(|d| !d.model.is_empty() && name.contains(&d.model.to_uppercase()))
}

/// Internal view of one enumerated HID interface/collection. `pub(crate)` so the
/// sibling driver modules can match against it.
#[cfg(windows)]
pub(crate) struct DeviceInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub product: String,
    pub manufacturer: String,
    pub path: String,
    pub usage_page: u16,
}

/// A registered driver: recognize a device, and open it into a [`HardwareEq`].
#[cfg(windows)]
struct Driver {
    identify: fn(&DeviceInfo) -> Option<(String, HardwareProfile)>,
    open: fn(hidapi::HidDevice, HardwareProfile) -> Box<dyn HardwareEq>,
}

/// The registry. Add a device family by adding a module and an entry here.
#[cfg(windows)]
fn drivers() -> &'static [Driver] {
    &[
        Driver {
            identify: moondrop::identify,
            open: moondrop::open,
        },
        Driver {
            identify: walkplay::identify,
            open: walkplay::open,
        },
    ]
}

#[cfg(windows)]
fn identify(info: &DeviceInfo) -> Option<(String, HardwareProfile)> {
    drivers().iter().find_map(|d| (d.identify)(info))
}

/// Enumerate supported hardware-EQ devices currently connected.
#[cfg(windows)]
pub fn detect() -> Result<Vec<DetectedDevice>, String> {
    use std::collections::HashSet;

    let infos = hid::enumerate()?;
    // Among interfaces a driver recognizes, prefer the vendor collection
    // (usage page ≥ 0xFF00) — that's where the PEQ reports live — and emit one
    // entry per physical device.
    let mut candidates: Vec<(&DeviceInfo, String, HardwareProfile)> = infos
        .iter()
        .filter_map(|i| identify(i).map(|(model, profile)| (i, model, profile)))
        .collect();
    candidates.sort_by_key(|(i, ..)| u8::from(i.usage_page < 0xFF00));

    let mut seen: HashSet<(u16, u16, String)> = HashSet::new();
    let mut out = Vec::new();
    for (info, model, profile) in candidates {
        if !seen.insert((info.vendor_id, info.product_id, model.clone())) {
            continue;
        }
        let manufacturer = info.manufacturer.trim().to_string();
        let name = if manufacturer.is_empty() {
            model.clone()
        } else {
            format!("{manufacturer} {model}")
        };
        out.push(DetectedDevice {
            id: info.path.clone(),
            name,
            manufacturer,
            model,
            max_filters: profile.max_filters,
            user_pregain: profile.user_pregain,
        });
    }
    Ok(out)
}

/// The PEQ profile (capabilities) of a detected device, without opening it — so a
/// caller can size the hardware/software split before the worker connects.
#[cfg(windows)]
pub fn profile(id: &str) -> Result<HardwareProfile, String> {
    let infos = hid::enumerate()?;
    let info = infos
        .iter()
        .find(|i| i.path == id)
        .ok_or_else(|| "Device is no longer connected".to_string())?;
    identify(info)
        .map(|(_, profile)| profile)
        .ok_or_else(|| "Unsupported device".to_string())
}

/// Open a previously-detected device by its [`DetectedDevice::id`].
#[cfg(windows)]
pub fn open(id: &str) -> Result<Box<dyn HardwareEq>, String> {
    let infos = hid::enumerate()?;
    let info = infos
        .iter()
        .find(|i| i.path == id)
        .ok_or_else(|| "Device is no longer connected".to_string())?;
    let (_, profile) = identify(info).ok_or_else(|| "Unsupported device".to_string())?;
    let driver = drivers()
        .iter()
        .find(|d| (d.identify)(info).is_some())
        .ok_or_else(|| "No driver for this device".to_string())?;
    let device = hid::open(&info.path)?;
    Ok((driver.open)(device, profile))
}

/// Enumerate hardware-EQ devices (non-Windows stub).
#[cfg(not(windows))]
pub fn detect() -> Result<Vec<DetectedDevice>, String> {
    Ok(Vec::new())
}

/// Device PEQ profile (non-Windows stub).
#[cfg(not(windows))]
pub fn profile(_id: &str) -> Result<HardwareProfile, String> {
    Err("Hardware EQ offload is only supported on Windows".to_string())
}

/// Open a hardware-EQ device (non-Windows stub).
#[cfg(not(windows))]
pub fn open(_id: &str) -> Result<Box<dyn HardwareEq>, String> {
    Err("Hardware EQ offload is only supported on Windows".to_string())
}
