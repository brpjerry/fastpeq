//! Audio output-device enumeration and default-device switching, backing the
//! "switch output device" global-hotkey action.
//!
//! This is a Windows-only OS concern, so it lives here in the shell rather than
//! in the pure `fastpeq-core` crate. Enumeration uses the documented Core Audio
//! `IMMDeviceEnumerator`; setting the default endpoint uses the **undocumented**
//! `IPolicyConfig` COM interface — the same mechanism nircmd / SoundVolumeView /
//! AudioSwitcher rely on, stable since Windows 7. We declare it by hand because
//! it isn't part of the public Windows metadata the `windows` crate is built from.
//!
//! The non-Windows build provides stubs so the crate still compiles and tests
//! stay green off-Windows, mirroring `fastpeq_core::apo::env::detect`.

use serde::Serialize;

/// A render (output) audio endpoint, as shown in the device picker.
#[derive(Serialize, Clone, Debug)]
pub struct AudioDevice {
    /// Stable MMDevice endpoint id, e.g. `"{0.0.0.00000000}.{guid}"`. This is
    /// what a hotkey binding stores, so it survives unplug/replug of the device.
    pub id: String,
    /// Friendly name, e.g. `"Speakers (Realtek(R) Audio)"`.
    pub name: String,
    /// Whether this is the current default render device (the `eConsole` role).
    pub is_default: bool,
}

#[cfg(windows)]
mod imp {
    // The hand-declared IPolicyConfig mirrors the COM interface's PascalCase
    // method names (and the macro generates matching vtable items), so the
    // standard snake_case lint doesn't apply here.
    #![allow(non_snake_case)]

    use super::AudioDevice;
    use core::ffi::c_void;
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        DEVICE_STATE_ACTIVE, IMMDeviceEnumerator, MMDeviceEnumerator, eCommunications, eConsole,
        eMultimedia, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
        CoUninitialize, STGM_READ,
    };
    use windows::Win32::System::Variant::VT_LPWSTR;
    use windows::core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, PCWSTR, PWSTR, interface};

    /// CLSID of the policy-config class object that implements `IPolicyConfig`.
    const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    /// The undocumented `IPolicyConfig`. Only `SetDefaultEndpoint` is called; the
    /// ten methods before it exist purely to place that method at the correct
    /// vtable slot (COM dispatches by slot, so order and count must match the
    /// real interface). Their signatures are placeholders — never invoked.
    #[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    unsafe trait IPolicyConfig: IUnknown {
        unsafe fn GetMixFormat(&self, _id: PCWSTR, _fmt: *mut *mut c_void) -> HRESULT;
        unsafe fn GetDeviceFormat(&self, _id: PCWSTR, _def: i32, _fmt: *mut *mut c_void)
        -> HRESULT;
        unsafe fn ResetDeviceFormat(&self, _id: PCWSTR) -> HRESULT;
        unsafe fn SetDeviceFormat(
            &self,
            _id: PCWSTR,
            _ep: *mut c_void,
            _mix: *mut c_void,
        ) -> HRESULT;
        unsafe fn GetProcessingPeriod(
            &self,
            _id: PCWSTR,
            _def: i32,
            _def_period: *mut i64,
            _min_period: *mut i64,
        ) -> HRESULT;
        unsafe fn SetProcessingPeriod(&self, _id: PCWSTR, _period: *mut i64) -> HRESULT;
        unsafe fn GetShareMode(&self, _id: PCWSTR, _mode: *mut c_void) -> HRESULT;
        unsafe fn SetShareMode(&self, _id: PCWSTR, _mode: *mut c_void) -> HRESULT;
        unsafe fn GetPropertyValue(
            &self,
            _id: PCWSTR,
            _store: i32,
            _key: *const c_void,
            _value: *mut c_void,
        ) -> HRESULT;
        unsafe fn SetPropertyValue(
            &self,
            _id: PCWSTR,
            _store: i32,
            _key: *const c_void,
            _value: *mut c_void,
        ) -> HRESULT;
        /// Make `device_id` the default for the given role (eConsole / eMultimedia
        /// / eCommunications). This is the one method we actually use.
        unsafe fn SetDefaultEndpoint(
            &self,
            device_id: PCWSTR,
            role: windows::Win32::Media::Audio::ERole,
        ) -> HRESULT;
        unsafe fn SetEndpointVisibility(&self, _id: PCWSTR, _visible: i32) -> HRESULT;
    }

    /// Initializes COM (MTA) for the current call and undoes it on drop, so each
    /// command is self-contained regardless of which worker thread runs it.
    struct ComGuard {
        owned: bool,
    }

    impl ComGuard {
        fn new() -> Self {
            // SAFETY: standard COM init. S_FALSE means COM was already initialized
            // on this thread (we still pair an uninit); RPC_E_CHANGED_MODE means
            // it was initialized as STA elsewhere — fine, we just don't own it.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            ComGuard { owned: hr.is_ok() }
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.owned {
                // SAFETY: balances the successful CoInitializeEx above.
                unsafe { CoUninitialize() };
            }
        }
    }

    /// Read a `PWSTR` returned by Core Audio into an owned `String` and free the
    /// COM-allocated buffer.
    ///
    /// SAFETY: `p` must be a valid, COM-allocated, NUL-terminated wide string
    /// (e.g. from `IMMDevice::GetId`); ownership transfers here.
    unsafe fn take_pwstr(p: PWSTR) -> String {
        if p.is_null() {
            return String::new();
        }
        // SAFETY: per the contract above, `p` is a valid COM-allocated wide
        // string whose ownership we now hold and free here.
        unsafe {
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const c_void));
            s
        }
    }

    pub fn list_devices() -> Result<Vec<AudioDevice>, String> {
        let _com = ComGuard::new();
        // SAFETY: all calls below are guarded by the COM init above and operate on
        // interfaces we own; raw pointers come straight from Core Audio.
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| e.to_string())?;

            // The current default; used only to flag the active row. A missing
            // default (no output at all) is not an error.
            let default_id = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .ok()
                .and_then(|d| d.GetId().ok())
                .map(|p| take_pwstr(p))
                .unwrap_or_default();

            let collection = enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
                .map_err(|e| e.to_string())?;
            let count = collection.GetCount().map_err(|e| e.to_string())?;

            let mut out = Vec::with_capacity(count as usize);
            for i in 0..count {
                let device = collection.Item(i).map_err(|e| e.to_string())?;
                let id = take_pwstr(device.GetId().map_err(|e| e.to_string())?);
                // PKEY_Device_FriendlyName is a VT_LPWSTR PROPVARIANT; read the
                // wide string straight out of the union (the PROPVARIANT's Drop
                // frees the buffer after we've copied it into a Rust String).
                let name = device
                    .OpenPropertyStore(STGM_READ)
                    .ok()
                    .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName).ok())
                    .and_then(|prop| {
                        if prop.Anonymous.Anonymous.vt == VT_LPWSTR {
                            prop.Anonymous.Anonymous.Anonymous.pwszVal.to_string().ok()
                        } else {
                            None
                        }
                    })
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| id.clone());
                let is_default = !id.is_empty() && id == default_id;
                out.push(AudioDevice {
                    id,
                    name,
                    is_default,
                });
            }
            Ok(out)
        }
    }

    pub fn set_default(id: &str) -> Result<(), String> {
        let _com = ComGuard::new();
        let id_w: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: COM is initialized; `id_w` outlives every call below; the
        // policy-config object is created and used on this thread only.
        unsafe {
            let policy: IPolicyConfig = CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL)
                .map_err(|e| e.to_string())?;
            // Set all three roles so both the default and default-communication
            // device follow, matching what users expect from device switchers.
            for role in [eConsole, eMultimedia, eCommunications] {
                policy
                    .SetDefaultEndpoint(PCWSTR(id_w.as_ptr()), role)
                    .ok()
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// The friendly name of the current default render endpoint, without
    /// enumerating every device. Cheap enough to poll from the offload reconciler
    /// (which only needs to notice when the active output *changes*).
    pub fn default_output_name() -> Option<String> {
        let _com = ComGuard::new();
        // SAFETY: guarded by the COM init above; the endpoint + its property store
        // are owned here, and the friendly-name PROPVARIANT is read like in
        // `list_devices`.
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            device
                .OpenPropertyStore(STGM_READ)
                .ok()
                .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName).ok())
                .and_then(|prop| {
                    if prop.Anonymous.Anonymous.vt == VT_LPWSTR {
                        prop.Anonymous.Anonymous.Anonymous.pwszVal.to_string().ok()
                    } else {
                        None
                    }
                })
                .filter(|s| !s.is_empty())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{list_devices, set_default};

        /// Smoke test against the real machine; ignored by default because it
        /// needs actual audio hardware. Run with:
        /// `cargo test -- --ignored lists_real_devices`
        #[test]
        #[ignore]
        fn lists_real_devices() {
            let devices = list_devices().expect("enumeration should succeed");
            for d in &devices {
                println!(
                    "{}{}  [{}]",
                    if d.is_default { "* " } else { "  " },
                    d.name,
                    d.id
                );
            }
            assert!(!devices.is_empty(), "expected at least one output device");
        }

        /// Exercises the undocumented `IPolicyConfig::SetDefaultEndpoint` vtable
        /// slot end-to-end by re-setting the default to the device that's *already*
        /// default — no audible change, but proves the COM call dispatches to the
        /// right method. Ignored by default (needs real hardware and mutates the
        /// system default). Run with:
        /// `cargo test -- --ignored sets_default_to_current`
        #[test]
        #[ignore]
        fn sets_default_to_current() {
            let devices = list_devices().expect("enumeration should succeed");
            let current = devices
                .iter()
                .find(|d| d.is_default)
                .expect("expected a current default device");
            set_default(&current.id).expect("re-setting the current default should succeed");
        }
    }
}

#[cfg(windows)]
pub use imp::{default_output_name, list_devices, set_default};

/// Enumerate output devices (non-Windows stub).
#[cfg(not(windows))]
pub fn list_devices() -> Result<Vec<AudioDevice>, String> {
    Ok(Vec::new())
}

/// Set the default output device (non-Windows stub).
#[cfg(not(windows))]
pub fn set_default(_id: &str) -> Result<(), String> {
    Err("Switching audio devices is only supported on Windows".to_string())
}

/// The default output device name (non-Windows stub).
#[cfg(not(windows))]
pub fn default_output_name() -> Option<String> {
    None
}
