//! Moondrop USB HID parametric-EQ driver (validated against the DHA15).
//!
//! Moondrop publishes no protocol spec; this is a native Rust port of the
//! community reverse-engineering in [`jeromeof/devicePEQ`] (`moondropUsbHidHandler.js`
//! plus the `"DHA15"` device entry). All output reports use HID report id `0x4B`.
//! A band is written as both a packed biquad (five coefficients at fs = 96 kHz,
//! scaled by 2^30, little-endian) *and* its human-readable freq/Q/gain/type; the
//! device echoes these back on read.
//!
//! Coefficient packing matches the reference encoder exactly (incl. its 32-bit
//! wrap of out-of-range coefficients), so the on-wire bytes equal what the working
//! community tool sends. Shelf behavior is the least-certain part of the
//! reverse-engineered spec — verify by read-back on real hardware.
//!
//! [`jeromeof/devicePEQ`]: https://github.com/jeromeof/devicePEQ

use super::{DeviceInfo, HardwareEq};
use fastpeq_core::{HardwareProfile, HwBand, HwFilterType, biquad_coeffs};
use hidapi::HidDevice;
use std::time::{Duration, Instant};

const REPORT_ID: u8 = 0x4B;
/// Payload length of a report (the on-wire report is this + 1 for the id). The EQ
/// write packet is the largest at 63 bytes; shorter commands are zero-padded, as
/// WebHID does for the reference tool.
const REPORT_LEN: usize = 63;

// Command bytes (first two payload bytes of every report).
const CMD_WRITE: u8 = 0x01;
const CMD_READ: u8 = 0x80;
const CMD_UPDATE_EQ: u8 = 0x09;
const CMD_COEFF_TO_REG: u8 = 0x0A;
const CMD_SAVE_TO_FLASH: u8 = 0x01;
const CMD_PRE_GAIN: u8 = 0x23;
const CMD_VER: u8 = 0x0C;

// Device filter-type codes.
const TYPE_PK: u8 = 2;
const TYPE_LSQ: u8 = 1;
const TYPE_HSQ: u8 = 3;

/// Biquad coefficient fixed-point scale (2^30).
const COEFF_SCALE: f64 = 1_073_741_824.0;

/// Small gap between consecutive reports within one push, so the device's MCU
/// keeps up. (Throttling *between* pushes is the worker's job.)
const INTER_PACKET: Duration = Duration::from_millis(4);
/// How long to wait for a device reply to a read command.
const READ_TIMEOUT: Duration = Duration::from_millis(1000);

/// Recognize a Moondrop device this driver can drive, returning its model name and
/// PEQ profile. Matches on the USB product string (the DHA15 reports `"DHA15"`).
pub(super) fn identify(info: &DeviceInfo) -> Option<(String, HardwareProfile)> {
    if info.product.to_ascii_uppercase().contains("DHA15") {
        return Some(("DHA15".to_string(), dha15_profile()));
    }
    None
}

/// The DHA15's capabilities (from the reverse-engineered `peq8Band12dBFullShelves`
/// constraints): 8 bands, ±12 dB, Q 0.1–10, peaking + low/high shelf, host-managed
/// pregain, biquads computed at 96 kHz.
fn dha15_profile() -> HardwareProfile {
    HardwareProfile {
        max_filters: 8,
        sample_rate: 96_000.0,
        gain_range: (-12.0, 12.0),
        q_range: (0.1, 10.0),
        freq_range: (20.0, 20_000.0),
        supports_low_shelf: true,
        supports_high_shelf: true,
    }
}

/// Open a recognized device into a driver instance.
pub(super) fn open(device: HidDevice, profile: HardwareProfile) -> Box<dyn HardwareEq> {
    Box::new(MoondropDevice { device, profile })
}

struct MoondropDevice {
    device: HidDevice,
    profile: HardwareProfile,
}

impl HardwareEq for MoondropDevice {
    fn push(&mut self, bands: &[HwBand], pregain: f64, commit: bool) -> Result<(), String> {
        // Write every slot so stale bands from a previous push are cleared: unused
        // slots get a flat (0 dB) peak.
        for i in 0..self.profile.max_filters {
            let band = bands.get(i).cloned().unwrap_or(FLAT_BAND);
            self.send(&write_packet(i as u8, &band, self.profile.sample_rate))?;
            self.send(&enable_packet(i as u8))?;
        }
        // DHA15 expects the host to supply input headroom (deviceHandlesPregain:false).
        self.send(&pregain_packet(pregain))?;
        if commit {
            self.send(&[CMD_WRITE, CMD_SAVE_TO_FLASH])?;
        }
        Ok(())
    }

    fn pull(&mut self) -> Result<Vec<HwBand>, String> {
        if dry_run() {
            return Ok(Vec::new());
        }
        let mut bands = Vec::with_capacity(self.profile.max_filters);
        for i in 0..self.profile.max_filters {
            self.send(&[CMD_READ, CMD_UPDATE_EQ, 0x18, 0x00, i as u8, 0x00])?;
            let p = self.read_reply(CMD_READ, CMD_UPDATE_EQ)?;
            bands.push(decode_band(&p));
        }
        Ok(bands)
    }

    fn version(&mut self) -> Result<String, String> {
        if dry_run() {
            return Ok("dry-run".to_string());
        }
        self.send(&[CMD_READ, CMD_VER])?;
        let p = self.read_reply(CMD_READ, CMD_VER)?;
        Ok(decode_version(&p))
    }
}

impl MoondropDevice {
    /// Send one report (`payload` is zero-padded to the report length and prefixed
    /// with the report id). Honors the dry-run guard and paces consecutive packets.
    fn send(&self, payload: &[u8]) -> Result<(), String> {
        let mut buf = [0u8; 1 + REPORT_LEN];
        buf[0] = REPORT_ID;
        buf[1..1 + payload.len()].copy_from_slice(payload);
        if dry_run() {
            eprintln!("[hw dry-run] moondrop send {:02x?}", &buf[..1 + payload.len()]);
            return Ok(());
        }
        self.device.write(&buf).map_err(|e| e.to_string())?;
        std::thread::sleep(INTER_PACKET);
        Ok(())
    }

    /// Read input reports until one whose first two payload bytes match
    /// `(c0, c1)`, or time out. The HID report id, if present, is stripped so the
    /// payload indices match the reference decoder.
    fn read_reply(&self, c0: u8, c1: u8) -> Result<Vec<u8>, String> {
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
            let payload = if buf[0] == REPORT_ID { &buf[1..n] } else { &buf[..n] };
            if payload.len() > 1 && payload[0] == c0 && payload[1] == c1 {
                return Ok(payload.to_vec());
            }
        }
        Err("Timed out waiting for a device reply".to_string())
    }
}

/// A flat band used to clear unused device slots.
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

/// Build the 63-byte EQ-write payload for one slot.
fn write_packet(index: u8, band: &HwBand, fs: f64) -> Vec<u8> {
    let mut p = vec![0u8; REPORT_LEN];
    p[0] = CMD_WRITE;
    p[1] = CMD_UPDATE_EQ;
    p[2] = 0x18; // bLength = 24
    p[4] = index;
    // Packed biquad: [b0, b1, b2, -a1, -a2] (the chip's feedback-negated form),
    // each round(c * 2^30) little-endian, at bytes 7..27.
    let [b0, b1, b2, a1, a2] = biquad_coeffs(band.kind, band.freq, band.gain, band.q, fs);
    for (i, c) in [b0, b1, b2, -a1, -a2].into_iter().enumerate() {
        p[7 + i * 4..11 + i * 4].copy_from_slice(&coeff_bytes(c));
    }
    p[27..29].copy_from_slice(&le16(band.freq.round() as i64));
    p[29..31].copy_from_slice(&le16((band.q * 256.0).round() as i64));
    p[31..33].copy_from_slice(&le16((band.gain * 256.0).round() as i64));
    p[33] = type_byte(band.kind);
    p[35] = 7; // peqIndex
    p
}

/// Build the payload that commits a written slot into the active register set.
fn enable_packet(index: u8) -> Vec<u8> {
    vec![CMD_WRITE, CMD_COEFF_TO_REG, index, 0, 255, 255, 255]
}

/// Build the host-pregain payload (`gain` in dB, scaled by 256, little-endian).
fn pregain_packet(gain: f64) -> Vec<u8> {
    let v = le16((gain * 256.0).round() as i64);
    vec![CMD_WRITE, CMD_PRE_GAIN, 0, v[0], v[1]]
}

/// One biquad coefficient as 4 little-endian bytes: `round(c * 2^30)` kept to its
/// low 32 bits. Mirrors the reference JS encoder, whose 32-bit bitwise ops wrap
/// coefficients that exceed i32 range (possible only at extreme gain + low Q).
fn coeff_bytes(c: f64) -> [u8; 4] {
    let low32 = (c * COEFF_SCALE).round().rem_euclid(4_294_967_296.0) as u32;
    low32.to_le_bytes()
}

/// A signed 16-bit little-endian value (two's-complement low 16 bits), matching
/// the reference encoder's masking of negative gains.
fn le16(v: i64) -> [u8; 2] {
    (v.rem_euclid(65_536) as u16).to_le_bytes()
}

/// Decode the version reply. The DHA15 returns an ASCII string starting at byte 3
/// (e.g. `"V0.5.0"`); read the printable run. Falls back to the raw numeric form if
/// the payload isn't text.
fn decode_version(p: &[u8]) -> String {
    let text: String = p[3..]
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .collect();
    let text = text.trim().to_string();
    if text.is_empty() {
        format!("{}.{}.{}", p[3], p[4], p[5])
    } else {
        text
    }
}

/// Decode a band from a read reply (inverse of [`write_packet`]'s readable fields).
/// On the read-back path (`pull`), so only reached by the hardware smoke test today.
#[allow(dead_code)]
fn decode_band(p: &[u8]) -> HwBand {
    let read_i16 = |lo: usize| i16::from_le_bytes([p[lo], p[lo + 1]]) as f64;
    let freq = u16::from_le_bytes([p[27], p[28]]) as f64;
    let q = read_i16(29) / 256.0;
    let gain = read_i16(31) / 256.0;
    let kind = match p.get(33) {
        Some(&TYPE_LSQ) => HwFilterType::LowShelf,
        Some(&TYPE_HSQ) => HwFilterType::HighShelf,
        _ => HwFilterType::Peak,
    };
    HwBand { kind, freq, gain, q }
}

/// Whether `FASTPEQ_HW_DRYRUN` is set — log packets instead of writing, for safe
/// first-contact debugging with an unverified protocol.
fn dry_run() -> bool {
    std::env::var_os("FASTPEQ_HW_DRYRUN").is_some_and(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_packet_layout_matches_reference() {
        let band = HwBand {
            kind: HwFilterType::Peak,
            freq: 1000.0,
            gain: 6.0,
            q: 1.0,
        };
        let p = write_packet(3, &band, 96_000.0);
        assert_eq!(p.len(), REPORT_LEN);
        assert_eq!(p[0], CMD_WRITE);
        assert_eq!(p[1], CMD_UPDATE_EQ);
        assert_eq!(p[2], 0x18);
        assert_eq!(p[4], 3); // slot index
        assert_eq!(u16::from_le_bytes([p[27], p[28]]), 1000); // freq
        assert_eq!(u16::from_le_bytes([p[29], p[30]]), 256); // Q * 256
        assert_eq!(i16::from_le_bytes([p[31], p[32]]), 6 * 256); // gain * 256
        assert_eq!(p[33], TYPE_PK);
        assert_eq!(p[35], 7);
    }

    #[test]
    fn negative_gain_encodes_twos_complement() {
        let band = HwBand {
            kind: HwFilterType::Peak,
            freq: 200.0,
            gain: -6.0,
            q: 1.0,
        };
        let p = write_packet(0, &band, 96_000.0);
        assert_eq!(i16::from_le_bytes([p[31], p[32]]), -6 * 256);
    }

    #[test]
    fn decode_is_inverse_of_encode_readable_fields() {
        let band = HwBand {
            kind: HwFilterType::HighShelf,
            freq: 8000.0,
            gain: -3.5,
            q: 0.7,
        };
        let p = write_packet(0, &band, 96_000.0);
        let back = decode_band(&p);
        assert_eq!(back.kind, HwFilterType::HighShelf);
        assert_eq!(back.freq, 8000.0);
        assert!((back.gain - (-3.5)).abs() < 0.01);
        assert!((back.q - 0.7).abs() < 0.01);
    }

    #[test]
    fn pregain_packet_encodes_scaled_gain() {
        let p = pregain_packet(-6.0);
        assert_eq!(p[0], CMD_WRITE);
        assert_eq!(p[1], CMD_PRE_GAIN);
        assert_eq!(i16::from_le_bytes([p[3], p[4]]), -6 * 256);
    }

    /// Times HID enumeration vs the cheap default-output lookup, to gauge how much
    /// work the background reconciler saves the UI. Run with:
    /// `cargo test -p fastpeq -- --ignored enum_timing --nocapture`.
    #[test]
    #[ignore]
    fn enum_timing() {
        let t = std::time::Instant::now();
        let n = super::super::detect().map(|d| d.len()).unwrap_or(0);
        println!("hardware::detect(): {} devices in {:?}", n, t.elapsed());
        let t = std::time::Instant::now();
        let name = crate::audio::default_output_name();
        println!("default_output_name(): {name:?} in {:?}", t.elapsed());
    }

    /// Correlation smoke test: the DHA15's audio-endpoint friendly name resolves to
    /// its HID device, while an unrelated output does not. Needs the DHA15 connected.
    /// Run with: `cargo test -p fastpeq -- --ignored dha15_correlates`.
    #[test]
    #[ignore]
    fn dha15_correlates_to_output_name() {
        let matched = super::super::device_for_output("DAC/Amp (Moondrop DHA15)");
        assert!(
            matched.is_some_and(|d| d.model.contains("DHA15")),
            "the DHA15 audio name should resolve to its HID device"
        );
        assert!(
            super::super::device_for_output("Headphones (FIIO Air Link)").is_none(),
            "an unrelated output must not offload"
        );
    }

    /// Real-hardware smoke test. Ignored by default (needs a connected DHA15);
    /// run with: `cargo test -p fastpeq -- --ignored dha15_roundtrip`.
    #[test]
    #[ignore]
    fn dha15_roundtrip() {
        let devices = super::super::detect().expect("enumeration should succeed");
        let dha = devices
            .iter()
            .find(|d| d.model.contains("DHA15"))
            .expect("a DHA15 should be connected");
        let mut dev = super::super::open(&dha.id).expect("open DHA15");
        let version = dev.version().expect("read version");
        println!("DHA15 firmware: {version}");

        let band = HwBand {
            kind: HwFilterType::Peak,
            freq: 1000.0,
            gain: 6.0,
            q: 1.0,
        };
        dev.push(&[band.clone()], -6.0, false).expect("push band");
        let read_back = dev.pull().expect("pull bands");
        let first = &read_back[0];
        assert!((first.freq - 1000.0).abs() < 2.0, "freq: {}", first.freq);
        assert!((first.gain - 6.0).abs() < 0.5, "gain: {}", first.gain);
    }
}
