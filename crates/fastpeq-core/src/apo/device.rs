//! Whether Equalizer APO is actually hooked into a given audio endpoint.
//!
//! [`env::detect`](super::env::detect) answers "is APO installed on this
//! machine?". That is not the same question as "will APO process the audio
//! going to the output I'm listening on right now?" — APO's Configurator
//! installs it *per render endpoint*, so a machine can have APO installed and
//! still have outputs it never touches. On such an output the app writes
//! `config.txt` happily and nothing is audible.
//!
//! APO installs itself by registering its two COM classes as effect objects on
//! the endpoint:
//!
//! ```text
//! HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\{guid}\FxProperties
//! ```
//!
//! The *value names* under that key are property keys whose slot index varies
//! by Windows version and by which effect stage APO took (the legacy LFX/GFX
//! pair and the modern SFX/MFX/EFX trio have all been observed), so we don't
//! look up a fixed slot — we enumerate every value and match on the CLSID.
//!
//! Note that `HKLM\SOFTWARE\EqualizerAPO\Child APOs\{guid}` looks like it would
//! answer the same question and does not: it is APO's bookkeeping of the
//! effects it displaced so it can restore them on uninstall, and entries for
//! endpoints APO no longer touches stay behind.

/// `EqualizerAPO Pre-Mix Class`, from `HKCR\CLSID`.
const PRE_MIX_CLSID: &str = "{EACD2258-FCAC-4FF4-B36D-419E924A6D79}";
/// `EqualizerAPO Post-Mix Class`, from `HKCR\CLSID`.
const POST_MIX_CLSID: &str = "{EC1CC9CE-FAED-4822-828A-82A81A6F018F}";

/// `PKEY_AudioEndpoint_Disable_SysFx` — `1` when the user has turned off
/// "Enable audio enhancements" for the endpoint, which stops APO loading even
/// though it is still registered.
#[cfg(windows)]
const DISABLE_SYSFX: &str = "{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5";

/// Whether Equalizer APO is hooked into a specific render endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointApo {
    /// APO's effect CLSIDs are registered on the endpoint and enhancements are
    /// on — audio through this output goes through APO.
    Active,
    /// No APO effect on this endpoint. APO's Configurator hasn't been run for
    /// it (or it was unticked).
    NotOnEndpoint,
    /// APO is registered, but Windows audio enhancements are off for the
    /// endpoint, so it never loads.
    EnhancementsOff,
    /// Couldn't tell: the endpoint has no registry entry, the id wasn't in the
    /// expected shape, or the key wasn't readable.
    Unknown,
}

/// Whether a registry value holds one of APO's effect CLSIDs.
///
/// Windows writes these as plain strings whose casing isn't guaranteed, so the
/// comparison is case-insensitive.
#[cfg_attr(not(windows), allow(dead_code))]
fn is_apo_clsid(value: &str) -> bool {
    let value = value.trim();
    PRE_MIX_CLSID.eq_ignore_ascii_case(value) || POST_MIX_CLSID.eq_ignore_ascii_case(value)
}

/// The endpoint GUID inside an `IMMDevice::GetId()` string.
///
/// Ids look like `{0.0.0.00000000}.{1e5df3a0-7d96-4c72-ba7b-e3f08c76332b}`; the
/// trailing brace-GUID is the `MMDevices` subkey name. Split out from the
/// registry work so it can be tested off Windows.
#[cfg_attr(not(windows), allow(dead_code))]
fn endpoint_guid(mm_device_id: &str) -> Option<&str> {
    let start = mm_device_id.rfind('{')?;
    let guid = &mm_device_id[start..];
    // Only the trailing segment, and only if it actually closes.
    guid.ends_with('}').then_some(guid)
}

/// Check whether Equalizer APO will process the given render endpoint.
///
/// `mm_device_id` is an `IMMDevice::GetId()` string. Cheap — two registry key
/// opens and a short value enumeration — but it is a blocking syscall, so
/// callers still run it off the UI thread.
#[cfg(windows)]
pub fn endpoint_state(mm_device_id: &str) -> EndpointApo {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::types::FromRegValue;

    let Some(guid) = endpoint_guid(mm_device_id) else {
        return EndpointApo::Unknown;
    };
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let endpoint =
        format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\{guid}");
    let Ok(device) = hklm.open_subkey(&endpoint) else {
        // No entry for this endpoint at all — nothing we can say about it.
        return EndpointApo::Unknown;
    };

    // An endpoint with no effects at all has no `FxProperties` key; that is a
    // definite "APO isn't on it", not an unknown.
    let registered = match device.open_subkey("FxProperties") {
        Ok(fx) => fx
            .enum_values()
            .filter_map(Result::ok)
            .filter_map(|(_, value)| String::from_reg_value(&value).ok())
            .any(|value| is_apo_clsid(&value)),
        Err(_) => false,
    };
    if !registered {
        return EndpointApo::NotOnEndpoint;
    }

    // Registered — but the audio engine skips every effect on the endpoint when
    // the user has turned enhancements off. An absent value means "on".
    let disabled = device
        .open_subkey("Properties")
        .and_then(|props| props.get_value::<u32, _>(DISABLE_SYSFX))
        .map(|flag| flag != 0)
        .unwrap_or(false);
    if disabled {
        EndpointApo::EnhancementsOff
    } else {
        EndpointApo::Active
    }
}

/// Check whether Equalizer APO will process the given render endpoint
/// (non-Windows stub).
#[cfg(not(windows))]
pub fn endpoint_state(_mm_device_id: &str) -> EndpointApo {
    EndpointApo::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_endpoint_guid() {
        assert_eq!(
            endpoint_guid("{0.0.0.00000000}.{1e5df3a0-7d96-4c72-ba7b-e3f08c76332b}"),
            Some("{1e5df3a0-7d96-4c72-ba7b-e3f08c76332b}")
        );
    }

    #[test]
    fn rejects_ids_without_a_trailing_guid() {
        assert_eq!(endpoint_guid(""), None);
        assert_eq!(endpoint_guid("no-braces-here"), None);
        // A truncated id: the last brace-group never closes.
        assert_eq!(endpoint_guid("{0.0.0.00000000}.{1e5df3a0-7d96"), None);
    }

    #[test]
    fn matches_apo_clsids_case_insensitively() {
        assert!(is_apo_clsid("{EACD2258-FCAC-4FF4-B36D-419E924A6D79}"));
        assert!(is_apo_clsid("{ec1cc9ce-faed-4822-828a-82a81a6f018f}"));
        // Windows sometimes pads the stored string.
        assert!(is_apo_clsid("  {EACD2258-FCAC-4FF4-B36D-419E924A6D79}  "));
    }

    #[test]
    fn ignores_other_vendors_clsids() {
        // Microsoft's default pre-mix effect, seen on endpoints APO isn't on.
        assert!(!is_apo_clsid("{C9453E73-8C5C-4463-9984-AF8BAB2F5447}"));
        assert!(!is_apo_clsid("{00000000-0000-0000-0000-000000000000}"));
        assert!(!is_apo_clsid(""));
    }

    /// An endpoint that doesn't exist is unknown, not "APO isn't on it" — the
    /// UI must not turn red because a device vanished mid-check.
    #[test]
    fn unknown_for_a_malformed_id() {
        assert_eq!(endpoint_state("not-an-endpoint-id"), EndpointApo::Unknown);
    }
}
