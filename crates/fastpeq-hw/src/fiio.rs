//! FiiO USB HID parametric-EQ driver (validated against the KA17).
//!
//! FiiO publishes no protocol spec; this is a native Rust port of the community
//! reverse-engineering in [`jeromeof/devicePEQ`] (`fiioUsbHidHandler.js`, the
//! `"FIIO KA17"` device entry, and the `fiio_fiio_ka17.json` USB capture).
//!
//! Unlike the Moondrop/Walkplay family (biquad coefficients at a fixed report id
//! `0x4B`), FiiO devices take *readable parameters only* — the DSP synthesizes
//! its own filters. Framing:
//!
//! - set: `[0xAA, 0x0A, 0, 0, cmd, len, ...payload, 0xEE]` (fire-and-forget)
//! - get: `[0xBB, 0x0B, 0, 0, cmd, 0, 0, 0xEE]`; replies echo
//!   `[0xBB, 0x0B, ?, ?, cmd, len, ...payload]`
//!
//! Fields are big-endian: gain = dB × 10 (two's-complement i16), freq = Hz
//! (u16), Q = Q × 100 (u16). EQ lives in preset slots — Jazz/Pop/… are read-only,
//! USER1/2/3 (preset ids 4, 8, 9) are writable, and edits apply to the running
//! DSP immediately; a separate save command persists them into a slot.
//!
//! **HID interface + firmware (validated on real hardware, KA17 firmware V2.0 /
//! bcdDevice 2.25).** The control protocol runs over the KA17's Generic-Desktop
//! collection (HID usage page **0x0001**, report id **1**, 32-byte Output+Input
//! reports) — *not* the vendor-defined `0xFF00` collection, which this firmware
//! exposes as a 64-byte **Feature** report only (no Output/Input reports), so an
//! Output write there fails with `WriteFile: Incorrect function`. Hence
//! [`identify`] claims only the `0x0001` collection. Persisting to flash uses
//! **SAVE_V2 (`0x21`)**, not the legacy save `0x19`: on V2.0 firmware the legacy
//! save faulted the device (USB malfunction) in testing, while `0x21` — the save
//! FiiO's newer devices use in the reference tool — persists cleanly. Live
//! (volatile) writes apply to the running DSP with no save at all.
//!
//! Pregain rides the global-gain register. Per the reference tool, FiiO
//! firmware permanently reserves `max_gain` (12 dB) of output headroom while
//! EQ is active, so the register is written as `12 + pregain` and the
//! *effective* level change is just `pregain`.
//!
//! [`jeromeof/devicePEQ`]: https://github.com/jeromeof/devicePEQ

use super::{DeviceInfo, HardwareEq};
use fastpeq_core::{HardwareProfile, HwBand, HwFilterType};
use hidapi::HidDevice;
use std::time::{Duration, Instant};

/// The KA17's HID report id (per-model in the reference tool; default is 7).
const REPORT_ID: u8 = 0x01;
/// Payload length of a report (the on-wire report is this + 1 for the id). The
/// KA17's control collection defines 32-byte reports; every command packet
/// ([`params_packet`] is the longest at 16 bytes) fits well within it.
const REPORT_LEN: usize = 32;
/// HID usage page of the KA17 collection that carries the control protocol (the
/// Output/Input report pair). The `0xFF00` vendor collection is feature-only on
/// current firmware and must not be used — see the module docs.
const CONTROL_USAGE_PAGE: u16 = 0x0001;

// Packet framing.
const SET_HEADER: [u8; 2] = [0xAA, 0x0A];
const GET_HEADER: [u8; 2] = [0xBB, 0x0B];
const END_BYTE: u8 = 0xEE;

// Command bytes (payload byte 4).
const CMD_FILTER_PARAMS: u8 = 0x15;
const CMD_PRESET_SWITCH: u8 = 0x16;
const CMD_GLOBAL_GAIN: u8 = 0x17;
const CMD_FILTER_COUNT: u8 = 0x18;
/// Persist the running EQ to a slot's flash. The KA17's V2.0 firmware uses the
/// newer "SAVE_V2" command (`0x21`); the legacy save (`0x19`) faulted the device
/// in on-hardware testing. Live/volatile writes need no save at all.
const CMD_SAVE: u8 = 0x21;
/// Firmware-version query (flagged "different headers" upstream — probed on
/// real hardware by the ignored `ka17_version_probe` test).
const CMD_VERSION: u8 = 0x0B;

/// The writable user preset ids (USER1/2/3). We activate USER1 when the device
/// is sitting on a read-only factory preset.
const USER_SLOTS: [u8; 3] = [4, 8, 9];
const USER1_SLOT: u8 = 4;

/// Output headroom (dB) FiiO firmware reserves while EQ is active; the
/// global-gain register carries `MAX_GAIN + pregain` to net out to `pregain`.
const MAX_GAIN_DB: f64 = 12.0;

// Device filter-type codes.
const TYPE_PK: u8 = 0;
const TYPE_LSQ: u8 = 1;
const TYPE_HSQ: u8 = 2;

/// Gap between consecutive reports within one push.
const INTER_PACKET: Duration = Duration::from_millis(10);
/// Post-batch settle time the reference tool uses after the counter write and
/// before a save.
const SETTLE: Duration = Duration::from_millis(100);
/// How long to wait for a device reply to a read command.
const READ_TIMEOUT: Duration = Duration::from_millis(1000);

/// Recognize a FiiO device this driver can drive. Matches the KA17 by vendor id
/// and product string (`"FIIO KA17"`, including the `(MQA HID)` firmware variant),
/// and *only* on its control collection ([`CONTROL_USAGE_PAGE`]) — the KA17
/// exposes several HID collections and the protocol lives on just one, so this
/// picks the interface [`super::detect`]/[`super::open`] will actually drive
/// (see the module docs on why it isn't the `0xFF00` vendor collection).
pub(super) fn identify(info: &DeviceInfo) -> Option<(String, HardwareProfile)> {
    if info.vendor_id == 0x2972
        && info.product.to_ascii_uppercase().contains("KA17")
        && info.usage_page == CONTROL_USAGE_PAGE
    {
        return Some(("KA17".to_string(), ka17_profile()));
    }
    None
}

/// The KA17's capabilities (the reference `peq10Band12dBAllFilters7`
/// constraints): 10 bands, ±12 dB, Q 0.1–10, peaking + low/high shelf (the
/// device also has LP/HP/BP/AP types fastpeq doesn't use), working pregain via
/// the global-gain register. `sample_rate` is nominal — the device synthesizes
/// its own filters from the readable parameters, no host biquads.
fn ka17_profile() -> HardwareProfile {
    HardwareProfile {
        max_filters: 10,
        sample_rate: 96_000.0,
        gain_range: (-12.0, 12.0),
        q_range: (0.1, 10.0),
        freq_range: (20.0, 20_000.0),
        supports_low_shelf: true,
        supports_high_shelf: true,
        user_pregain: true,
        commit_to_apply: false, // applies live (RAM) writes immediately
        commit_delay_ms: 500,
    }
}

/// Open a recognized device into a driver instance.
pub(super) fn open(device: HidDevice, profile: HardwareProfile) -> Box<dyn HardwareEq> {
    Box::new(FiioDevice {
        device,
        profile,
        active_slot: None,
    })
}

struct FiioDevice {
    device: HidDevice,
    profile: HardwareProfile,
    /// The writable user slot edits land in (and saves persist to). Resolved on
    /// the first push: the device's current preset if it's USER1/2/3, else
    /// USER1 is activated.
    active_slot: Option<u8>,
}

impl HardwareEq for FiioDevice {
    fn push(&mut self, bands: &[HwBand], pregain: f64, commit: bool) -> Result<(), String> {
        let slot = self.ensure_user_slot()?;
        // Order mirrors the reference tool: gain, count, params, then save.
        self.send(&global_gain_packet(pregain))?;
        // Pin the count at the device maximum and write every slot, flat
        // (0 dB, inert) fillers past the real bands — the FiiO app's own
        // always-N-bands model. The count read-back stays at the maximum no
        // matter what was written (observed on the KA17), so a smaller count
        // can't be trusted to silence stale bands; overwriting them can.
        self.send(&set1_packet(
            CMD_FILTER_COUNT,
            self.profile.max_filters as u8,
        ))?;
        std::thread::sleep(SETTLE);
        for i in 0..self.profile.max_filters {
            let band = bands.get(i).cloned().unwrap_or(FLAT_BAND);
            self.send(&params_packet(i as u8, &band))?;
        }
        if commit {
            std::thread::sleep(SETTLE);
            self.send(&set1_packet(CMD_SAVE, slot))?;
        }
        Ok(())
    }

    // Read the bands on the device. Non-essential — the offload path only ever
    // writes; this backs the smoke test and a reserved device→app sync. Works
    // when a user slot with written bands is active over a stable link; a flaky
    // USB connection can make the per-band reads time out.
    fn pull(&mut self) -> Result<Vec<HwBand>, String> {
        if dry_run() {
            return Ok(Vec::new());
        }
        self.drain_input();
        self.send(&get_packet(CMD_FILTER_COUNT))?;
        let count = self.read_reply(CMD_FILTER_COUNT)?[6] as usize;
        let n = count.min(self.profile.max_filters);
        let mut bands = vec![FLAT_BAND; n];
        for i in 0..n {
            self.send(&get_band_packet(i as u8))?;
            let p = self.read_reply(CMD_FILTER_PARAMS)?;
            // Replies carry their band index; trust it over arrival order.
            let idx = p[6] as usize;
            if idx < n {
                bands[idx] = decode_band(&p);
            }
        }
        Ok(bands)
    }

    fn version(&mut self) -> Result<String, String> {
        if dry_run() {
            return Ok("dry-run".to_string());
        }
        self.drain_input();
        // The version query is best-effort (upstream flags it as using
        // "different headers"); fall back to the preset query as the
        // connection handshake so a device without it still connects.
        self.send(&get_packet(CMD_VERSION))?;
        if let Ok(p) = self.read_reply(CMD_VERSION) {
            let v = decode_version(&p);
            if !v.is_empty() {
                return Ok(v);
            }
        }
        self.drain_input();
        self.send(&get_packet(CMD_PRESET_SWITCH))?;
        self.read_reply(CMD_PRESET_SWITCH)?;
        Ok("connected".to_string())
    }
}

impl FiioDevice {
    /// Resolve (once per session) the writable user slot: keep the device's
    /// current preset if it's already USER1/2/3, otherwise activate USER1 so
    /// edits are audible and don't overwrite a factory curve.
    fn ensure_user_slot(&mut self) -> Result<u8, String> {
        if let Some(slot) = self.active_slot {
            return Ok(slot);
        }
        if dry_run() {
            self.active_slot = Some(USER1_SLOT);
            return Ok(USER1_SLOT);
        }
        self.drain_input();
        self.send(&get_packet(CMD_PRESET_SWITCH))?;
        let current = self.read_reply(CMD_PRESET_SWITCH).ok().map(|p| p[6]);
        let slot = match current {
            Some(s) if USER_SLOTS.contains(&s) => s,
            _ => {
                self.switch_preset(USER1_SLOT)?;
                USER1_SLOT
            }
        };
        self.active_slot = Some(slot);
        Ok(slot)
    }

    /// Switch the active preset and verify it took — the firmware drops preset
    /// switches that arrive while it's still settling, so read back and retry.
    fn switch_preset(&self, preset: u8) -> Result<(), String> {
        for _ in 0..3 {
            self.send(&set1_packet(CMD_PRESET_SWITCH, preset))?;
            std::thread::sleep(SETTLE * 3);
            self.drain_input();
            self.send(&get_packet(CMD_PRESET_SWITCH))?;
            if self.read_reply(CMD_PRESET_SWITCH).ok().map(|p| p[6]) == Some(preset) {
                return Ok(());
            }
        }
        Err(format!("Device refused to switch to preset {preset}"))
    }

    /// Discard any queued input reports so a read matches its own reply, not a
    /// stale one.
    fn drain_input(&self) {
        let mut buf = [0u8; 1 + REPORT_LEN];
        while matches!(self.device.read_timeout(&mut buf, 0), Ok(n) if n > 0) {}
    }

    /// Send one report (`payload` is zero-padded to the report length and
    /// prefixed with the report id). Honors the dry-run guard and paces
    /// consecutive packets.
    fn send(&self, payload: &[u8]) -> Result<(), String> {
        let mut buf = [0u8; 1 + REPORT_LEN];
        buf[0] = REPORT_ID;
        buf[1..1 + payload.len()].copy_from_slice(payload);
        if dry_run() {
            eprintln!("[hw dry-run] fiio send {:02x?}", &buf[..1 + payload.len()]);
            return Ok(());
        }
        self.device.write(&buf).map_err(|e| e.to_string())?;
        std::thread::sleep(INTER_PACKET);
        Ok(())
    }

    /// Read input reports until a `[0xBB, 0x0B, …, cmd, …]` reply for `cmd`
    /// arrives, or time out. The HID report id, if present, is stripped so the
    /// payload indices match the reference decoder.
    fn read_reply(&self, cmd: u8) -> Result<Vec<u8>, String> {
        let mut buf = [0u8; 1 + REPORT_LEN];
        let deadline = Instant::now() + READ_TIMEOUT;
        while Instant::now() < deadline {
            let n = self
                .device
                .read_timeout(&mut buf, 200)
                .map_err(|e| e.to_string())?;
            if n == 0 {
                continue;
            }
            let payload = if buf[0] == REPORT_ID {
                &buf[1..n]
            } else {
                &buf[..n]
            };
            if payload.len() > 6
                && payload[0] == GET_HEADER[0]
                && payload[1] == GET_HEADER[1]
                && payload[4] == cmd
            {
                return Ok(payload.to_vec());
            }
        }
        Err("Timed out waiting for a device reply".to_string())
    }
}

/// A flat band (0 dB peak) for unset slots in a pull.
const FLAT_BAND: HwBand = HwBand {
    kind: HwFilterType::Peak,
    freq: 1000.0,
    gain: 0.0,
    q: 1.0,
};

fn type_byte(kind: HwFilterType) -> u8 {
    match kind {
        HwFilterType::Peak => TYPE_PK,
        HwFilterType::LowShelf => TYPE_LSQ,
        HwFilterType::HighShelf => TYPE_HSQ,
    }
}

/// Big-endian two's-complement of `v` rounded — the wire form of gain (dB × 10).
fn be16(v: f64) -> [u8; 2] {
    let i = v.round() as i64;
    let u = i.rem_euclid(65_536) as u16;
    u.to_be_bytes()
}

/// Build a set packet with a 1-byte argument: `[AA 0A 0 0 cmd 1 arg 0 EE]`.
fn set1_packet(cmd: u8, arg: u8) -> Vec<u8> {
    vec![SET_HEADER[0], SET_HEADER[1], 0, 0, cmd, 1, arg, 0, END_BYTE]
}

/// Build the global-gain set packet. The device reserves [`MAX_GAIN_DB`] of
/// headroom while EQ runs, so the register gets `MAX_GAIN_DB + pregain`
/// (clamped to the writable ±12 dB) to net out to `pregain`.
///
/// Pregain is input *headroom* (attenuation) and is clamped to `≤ 0` first: a
/// positive request would only try to push the register past the reserved level
/// — i.e. boost the device output — which we never do, no matter how much
/// built-in headroom the device advertises.
fn global_gain_packet(pregain: f64) -> Vec<u8> {
    let pregain = pregain.min(0.0);
    let db = (MAX_GAIN_DB + pregain).clamp(-MAX_GAIN_DB, MAX_GAIN_DB);
    let [hi, lo] = be16(db * 10.0);
    vec![
        SET_HEADER[0],
        SET_HEADER[1],
        0,
        0,
        CMD_GLOBAL_GAIN,
        2,
        hi,
        lo,
        0,
        END_BYTE,
    ]
}

/// Build the per-band parameter packet:
/// `[AA 0A 0 0 15 8 idx gainHi gainLo freqHi freqLo qHi qLo type 0 EE]`.
fn params_packet(index: u8, band: &HwBand) -> Vec<u8> {
    let [gain_hi, gain_lo] = be16(band.gain * 10.0);
    let freq = (band.freq.round() as u16).to_be_bytes();
    let q = (((band.q * 100.0).round()) as u16).to_be_bytes();
    vec![
        SET_HEADER[0],
        SET_HEADER[1],
        0,
        0,
        CMD_FILTER_PARAMS,
        8,
        index,
        gain_hi,
        gain_lo,
        freq[0],
        freq[1],
        q[0],
        q[1],
        type_byte(band.kind),
        0,
        END_BYTE,
    ]
}

/// Build a get packet: `[BB 0B 0 0 cmd 0 0 EE]`.
fn get_packet(cmd: u8) -> Vec<u8> {
    vec![GET_HEADER[0], GET_HEADER[1], 0, 0, cmd, 0, 0, END_BYTE]
}

/// Build the per-band get packet: `[BB 0B 0 0 15 1 idx 0 EE]`.
fn get_band_packet(index: u8) -> Vec<u8> {
    vec![
        GET_HEADER[0],
        GET_HEADER[1],
        0,
        0,
        CMD_FILTER_PARAMS,
        1,
        index,
        0,
        END_BYTE,
    ]
}

/// Decode a band from a params reply (`[BB 0B ? ? 15 8 idx gHi gLo fHi fLo qHi qLo type …]`).
fn decode_band(p: &[u8]) -> HwBand {
    let gain_raw = i16::from_be_bytes([p[7], p[8]]);
    let freq = u16::from_be_bytes([p[9], p[10]]) as f64;
    let q_raw = u16::from_be_bytes([p[11], p[12]]);
    let kind = match p.get(13) {
        Some(&TYPE_LSQ) => HwFilterType::LowShelf,
        Some(&TYPE_HSQ) => HwFilterType::HighShelf,
        _ => HwFilterType::Peak,
    };
    HwBand {
        kind,
        freq,
        gain: gain_raw as f64 / 10.0,
        q: if q_raw == 0 {
            1.0
        } else {
            q_raw as f64 / 100.0
        },
    }
}

/// Decode a version reply: printable ASCII in the payload after the length
/// byte, e.g. firmware strings like `"1.23"`. Empty when the reply carries none.
fn decode_version(p: &[u8]) -> String {
    p[6..]
        .iter()
        .take_while(|&&b| b != 0 && b != END_BYTE)
        .map(|&b| b as char)
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .collect::<String>()
        .trim()
        .to_string()
}

/// Whether `FASTPEQ_HW_DRYRUN` is set — log packets instead of writing.
fn dry_run() -> bool {
    std::env::var_os("FASTPEQ_HW_DRYRUN").is_some_and(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_packet_matches_the_ka17_capture() {
        // Capture frame: [AA 0A 00 00 15 08 00 FF E2 00 14 01 2C 00 00 EE]
        // = band 0, −3.0 dB, 20 Hz, Q 3.0, peaking.
        let band = HwBand {
            kind: HwFilterType::Peak,
            freq: 20.0,
            gain: -3.0,
            q: 3.0,
        };
        assert_eq!(
            params_packet(0, &band),
            vec![
                0xAA, 0x0A, 0, 0, 0x15, 8, 0, 0xFF, 0xE2, 0x00, 0x14, 0x01, 0x2C, 0, 0, 0xEE
            ]
        );
    }

    #[test]
    fn global_gain_nets_out_the_reserved_headroom() {
        // Capture frame: [AA 0A 00 00 17 02 00 46 00 EE] = +7.0 dB register,
        // i.e. a −5 dB effective pregain on top of the reserved −12 dB.
        assert_eq!(
            global_gain_packet(-5.0),
            vec![0xAA, 0x0A, 0, 0, 0x17, 2, 0x00, 0x46, 0, 0xEE]
        );
        // 0 pregain → the full +12 offset; deep pregain clamps at −12.
        assert_eq!(&global_gain_packet(0.0)[6..8], &[0x00, 0x78]);
        assert_eq!(&global_gain_packet(-30.0)[6..8], &[0xFF, 0x88]);
        // A positive request never boosts: it caps at the reserved level (the
        // register stays at +12, i.e. an effective 0 dB), same as 0 pregain.
        assert_eq!(&global_gain_packet(5.0)[6..8], &[0x00, 0x78]);
    }

    #[test]
    fn decode_is_inverse_of_encode() {
        let band = HwBand {
            kind: HwFilterType::HighShelf,
            freq: 8000.0,
            gain: -3.5,
            q: 0.7,
        };
        let sent = params_packet(2, &band);
        // A device reply echoes the same field layout behind a BB 0B header.
        let mut reply = vec![0xBB, 0x0B, 0, 0, 0x15, 8];
        reply.extend_from_slice(&sent[6..15]);
        let back = decode_band(&reply);
        assert_eq!(back.kind, HwFilterType::HighShelf);
        assert_eq!(back.freq, 8000.0);
        assert!((back.gain - (-3.5)).abs() < 0.01);
        assert!((back.q - 0.7).abs() < 0.01);
    }

    #[test]
    fn identify_matches_the_ka17_product_string() {
        let info = DeviceInfo {
            vendor_id: 0x2972,
            product_id: 0x0093,
            product: "FIIO KA17".to_string(),
            manufacturer: "FiiO".to_string(),
            path: "p".to_string(),
            usage_page: CONTROL_USAGE_PAGE,
        };
        let (model, profile) = identify(&info).expect("should identify");
        assert_eq!(model, "KA17");
        assert_eq!(profile.max_filters, 10);
        assert!(profile.user_pregain);

        // Another vendor's "KA17" is not claimed.
        let other = DeviceInfo {
            vendor_id: 0x1234,
            ..info.clone()
        };
        assert!(identify(&other).is_none());

        // The same KA17 on its other HID collections is NOT claimed — only the
        // control collection is (the `0xFF00` vendor collection is feature-only,
        // and `0x000C` is the media-key consumer collection).
        for up in [0xFF00u16, 0x000C] {
            let iface = DeviceInfo {
                usage_page: up,
                ..info.clone()
            };
            assert!(
                identify(&iface).is_none(),
                "usage page {up:#06x} must not be claimed"
            );
        }
    }

    /// Utility: put the device back on USER1 (verified switch). Run with:
    /// `cargo test -p fastpeq-hw -- --ignored ka17_restore_user_slot --nocapture`.
    #[test]
    #[ignore]
    fn ka17_restore_user_slot() {
        let devices = super::super::detect().expect("enumeration should succeed");
        let info = devices
            .iter()
            .find(|d| d.model.contains("KA17"))
            .expect("a KA17 should be connected");
        let dev = FiioDevice {
            device: super::super::hid::open(&info.id).expect("open"),
            profile: ka17_profile(),
            active_slot: None,
        };
        dev.switch_preset(USER1_SLOT).expect("switch to USER1");
        println!("device is back on USER1 (preset {USER1_SLOT})");
    }

    /// Utility: reset USER1 to a flat curve AND persist it (SAVE_V2), so the slot
    /// isn't left holding a test EQ across a power cycle. Run with:
    /// `cargo test -p fastpeq-hw -- --ignored ka17_clear_user_slot --nocapture`.
    #[test]
    #[ignore]
    fn ka17_clear_user_slot() {
        let info = super::super::detect()
            .expect("enumeration should succeed")
            .into_iter()
            .find(|d| d.model.contains("KA17"))
            .expect("a KA17 should be connected");
        let mut dev = super::super::open(&info.id).expect("open KA17");
        dev.push(&[], 0.0, true).expect("flatten + persist USER1");
        println!("USER1 reset to flat and saved");
    }

    /// Read-only probe: current preset, filter count, global gain, and the
    /// best-effort version query. Run with:
    /// `cargo test -p fastpeq-hw -- --ignored ka17_version_probe --nocapture`.
    #[test]
    #[ignore]
    fn ka17_version_probe() {
        let devices = super::super::detect().expect("enumeration should succeed");
        let info = devices
            .iter()
            .find(|d| d.model.contains("KA17"))
            .expect("a KA17 should be connected");
        let dev = FiioDevice {
            device: super::super::hid::open(&info.id).expect("open"),
            profile: ka17_profile(),
            active_slot: None,
        };

        let read = |cmd: u8, label: &str| {
            dev.drain_input();
            dev.send(&get_packet(cmd)).unwrap();
            match dev.read_reply(cmd) {
                Ok(p) => println!("{label}: {:02x?}", &p[..p.len().min(20)]),
                Err(e) => println!("{label}: no reply ({e})"),
            }
        };

        read(CMD_PRESET_SWITCH, "current preset (0x16)");
        read(CMD_FILTER_COUNT, "filter count (0x18)");
        read(CMD_GLOBAL_GAIN, "global gain (0x17)");
        read(CMD_VERSION, "version (0x0B)");

        for i in 0..4u8 {
            dev.drain_input();
            dev.send(&get_band_packet(i)).unwrap();
            match dev.read_reply(CMD_FILTER_PARAMS) {
                Ok(p) => println!("band {i}: {:?}", decode_band(&p)),
                Err(e) => println!("band {i}: no reply ({e})"),
            }
        }
    }

    /// Real-hardware smoke test. Ignored by default (needs a connected KA17);
    /// run with: `cargo test -p fastpeq-hw -- --ignored ka17_roundtrip --nocapture`.
    /// Live (volatile) writes only — no flash save — flattened after.
    ///
    /// The core assertion is that [`FiioDevice::push`] completes on the control
    /// interface and is audible. Read-back is best-effort: current KA17 firmware
    /// answers the preset/count reads but not the per-band parameter reads, so
    /// [`pull`] may return nothing here — the offload path only ever writes.
    ///
    /// [`pull`]: FiioDevice::pull
    #[test]
    #[ignore]
    fn ka17_roundtrip() {
        let devices = super::super::detect().expect("enumeration should succeed");
        let info = devices
            .iter()
            .find(|d| d.model.contains("KA17"))
            .expect("a KA17 should be connected");

        // Snapshot the current preset so we can put the device back on it.
        let raw = FiioDevice {
            device: super::super::hid::open(&info.id).expect("open"),
            profile: ka17_profile(),
            active_slot: None,
        };
        raw.drain_input();
        raw.send(&get_packet(CMD_PRESET_SWITCH)).unwrap();
        let preset = raw.read_reply(CMD_PRESET_SWITCH).ok().map(|p| p[6]);
        println!("baseline preset: {preset:?}");
        drop(raw);

        let mut dev = super::super::open(&info.id).expect("open KA17");
        let version = dev.version().expect("handshake");
        println!("KA17 handshake: {version}");

        let bands = [
            HwBand {
                kind: HwFilterType::Peak,
                freq: 1000.0,
                gain: 6.0,
                q: 1.0,
            },
            HwBand {
                kind: HwFilterType::LowShelf,
                freq: 120.0,
                gain: -4.0,
                q: 0.7,
            },
        ];
        // The write path completing (and applying live) is the real check.
        dev.push(&bands, -6.0, false).expect("push bands");

        // Best-effort read-back — only assert if the firmware actually answers.
        match dev.pull() {
            Ok(read_back) if read_back.len() >= 2 => {
                println!("read back: {read_back:?}");
                let first = &read_back[0];
                assert!((first.freq - 1000.0).abs() < 2.0, "freq: {}", first.freq);
                assert!((first.gain - 6.0).abs() < 0.5, "gain: {}", first.gain);
                assert_eq!(
                    read_back[1].kind,
                    HwFilterType::LowShelf,
                    "shelf type survives"
                );
            }
            other => println!("pull unsupported on this firmware ({other:?}) — write-only path"),
        }

        // Cleanup: flatten the running table (volatile — never saved) and switch
        // back to the preset the device started on.
        dev.push(&[], 0.0, false).expect("restore flat");
        if let Some(p) = preset {
            let raw = FiioDevice {
                device: super::super::hid::open(&info.id).expect("reopen"),
                profile: ka17_profile(),
                active_slot: None,
            };
            let _ = raw.switch_preset(p);
            println!("restored preset {p}");
        }
    }
}
