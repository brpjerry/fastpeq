//! Walkplay-platform USB HID parametric-EQ driver (validated against the
//! Tanchjim Space Pro, protocol scheme "No16").
//!
//! Walkplay publishes no protocol spec; this is a native Rust port of the
//! community reverse-engineering in [`jeromeof/devicePEQ`] (`walkplayHidHandler.js`
//! plus the `SchemeNo16` device group and the Space Pro USB capture). The wire
//! format is a close sibling of the Moondrop protocol in [`super::moondrop`]:
//! report id `0x4B`, 63-byte zero-padded payloads, and per-band packets carrying
//! both packed biquads (fs = 96 kHz, scaled by 2^30, little-endian) and readable
//! freq/Q/gain/type fields. Differences from the DHA15:
//!
//! - byte 35 of a band packet is the EQ *slot* (101 = the app's "Custom" slot),
//! - registers are applied with one apply-all command (`0x0A`) instead of a
//!   per-band enable,
//! - a flash commit is the app's sequence `0x05`, `0x17`, `0x0A`, `0x01`,
//! - pregain goes to the 1-byte global-gain register (`0x03`), which *works*
//!   (unlike the DHA15's `0x23`). Walkplay hardware keeps a fixed −5 dB
//!   DAC-stage headroom buffer, so the register only receives the excess below
//!   it: `min(0, pregain + 5)` — the effective device gain is `min(-5, pregain)`.
//!
//! [`jeromeof/devicePEQ`]: https://github.com/jeromeof/devicePEQ

use super::{DeviceInfo, HardwareEq};
use fastpeq_core::{HardwareProfile, HwBand, HwFilterType, biquad_coeffs};
use hidapi::HidDevice;
use std::time::{Duration, Instant};

const REPORT_ID: u8 = 0x4B;
/// Payload length of a report (the on-wire report is this + 1 for the id).
const REPORT_LEN: usize = 63;

// Command bytes (first two payload bytes of every report).
const CMD_WRITE: u8 = 0x01;
const CMD_READ: u8 = 0x80;
/// Write with no args: persist registers to flash. With `(enable, slot)` args:
/// activate an EQ slot.
const CMD_FLASH_EQ: u8 = 0x01;
/// The 1-byte signed global-gain register (dB) — the working pregain path.
const CMD_GLOBAL_GAIN: u8 = 0x03;
const CMD_RESET_EQ: u8 = 0x05;
const CMD_PEQ_VALUES: u8 = 0x09;
/// Apply the written coefficients to the running DSP registers (all bands).
const CMD_TEMP_WRITE: u8 = 0x0A;
const CMD_VER: u8 = 0x0C;
const CMD_RESET_FLASH: u8 = 0x17;

/// The writable "Custom" EQ slot every band packet is tagged with.
const CUSTOM_SLOT: u8 = 101;

/// Fixed DAC-stage headroom Walkplay hardware always applies; the global-gain
/// register only carries what's needed beyond it.
const GAIN_BUFFER_DB: f64 = -5.0;

// Device filter-type codes (same family as Moondrop's).
const TYPE_PK: u8 = 2;
const TYPE_LSQ: u8 = 1;
const TYPE_HSQ: u8 = 3;

/// Biquad coefficient fixed-point scale (2^30).
const COEFF_SCALE: f64 = 1_073_741_824.0;

/// Gap between consecutive reports within one push — the reference tool waits
/// 20 ms between band writes so the device's MCU keeps up.
const INTER_PACKET: Duration = Duration::from_millis(20);
/// How long to wait for a device reply to a read command.
const READ_TIMEOUT: Duration = Duration::from_millis(1000);

/// Recognize a Walkplay device this driver can drive. Matches the Tanchjim
/// Space Pro by vendor id + product string (the connected "AT" hardware revision
/// reports `"TANCHJIM-SPACE PRO AT"` and a PID absent from the community PID
/// tables, so the string is the stable discriminator).
pub(super) fn identify(info: &DeviceInfo) -> Option<(String, HardwareProfile)> {
    if info.vendor_id == 0x3302 && info.product.to_ascii_uppercase().contains("SPACE PRO") {
        return Some(("Space Pro".to_string(), space_pro_profile()));
    }
    None
}

/// The Space Pro's capabilities (the `SchemeNo16` group's `peq10Band10dBFullShelves`
/// constraints): 10 bands, ±10 dB, Q 0.1–10, peaking + low/high shelf, biquads at
/// 96 kHz, and a working host-writable pregain (the global-gain register).
fn space_pro_profile() -> HardwareProfile {
    HardwareProfile {
        max_filters: 10,
        sample_rate: 96_000.0,
        gain_range: (-10.0, 10.0),
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
    Box::new(WalkplayDevice {
        device,
        profile,
        slot_checked: false,
    })
}

struct WalkplayDevice {
    device: HidDevice,
    profile: HardwareProfile,
    /// Whether we've verified (once per session) that the Custom slot is active,
    /// so the bands we write are the ones the device runs and persists.
    slot_checked: bool,
}

impl HardwareEq for WalkplayDevice {
    fn push(&mut self, bands: &[HwBand], pregain: f64, commit: bool) -> Result<(), String> {
        self.ensure_custom_slot()?;
        // Write every slot so stale bands from a previous push are cleared: unused
        // slots get a flat (0 dB) peak.
        for i in 0..self.profile.max_filters {
            let band = bands.get(i).cloned().unwrap_or(FLAT_BAND);
            self.send(&write_packet(i as u8, &band, self.profile.sample_rate))?;
        }
        self.send(&global_gain_packet(pregain))?;
        if commit {
            // The Walkplay app's save order: reset, then apply, then flash.
            self.send(&[CMD_WRITE, CMD_RESET_EQ])?;
            self.send(&[CMD_WRITE, CMD_RESET_FLASH])?;
            self.send(&apply_packet())?;
            self.send(&[CMD_WRITE, CMD_FLASH_EQ])?;
        } else {
            // Live preview: apply the registers, leave flash alone.
            self.send(&apply_packet())?;
        }
        Ok(())
    }

    fn pull(&mut self) -> Result<Vec<HwBand>, String> {
        if dry_run() {
            return Ok(Vec::new());
        }
        self.drain_input();
        let mut bands = Vec::with_capacity(self.profile.max_filters);
        for i in 0..self.profile.max_filters {
            self.send(&[CMD_READ, CMD_PEQ_VALUES, 0x00, 0x00, i as u8, 0x00])?;
            let p = self.read_reply(CMD_READ, CMD_PEQ_VALUES)?;
            bands.push(decode_band(&p));
        }
        Ok(bands)
    }

    fn version(&mut self) -> Result<String, String> {
        if dry_run() {
            return Ok("dry-run".to_string());
        }
        self.drain_input();
        self.send(&[CMD_READ, CMD_VER, 0x00])?;
        let p = self.read_reply(CMD_READ, CMD_VER)?;
        Ok(decode_version(&p))
    }
}

impl WalkplayDevice {
    /// Once per session: make sure the device is running the Custom slot our band
    /// packets are tagged with, so writes are audible and survive a power cycle.
    /// The current slot sits at payload byte 35 of the bulk EQ read.
    fn ensure_custom_slot(&mut self) -> Result<(), String> {
        if self.slot_checked || dry_run() {
            self.slot_checked = true;
            return Ok(());
        }
        self.drain_input();
        self.send(&[CMD_READ, CMD_PEQ_VALUES, 0x00])?;
        let active = self
            .read_reply(CMD_READ, CMD_PEQ_VALUES)
            .ok()
            .and_then(|p| p.get(35).copied());
        if active != Some(CUSTOM_SLOT) {
            // Activate: CMD_FLASH_EQ with (enable = 1, slot) args.
            self.send(&[CMD_WRITE, CMD_FLASH_EQ, 0x01, CUSTOM_SLOT, 0x00])?;
        }
        self.slot_checked = true;
        Ok(())
    }

    /// Discard any queued input reports. The bulk EQ read (`0x80 0x09` with no
    /// band index) streams one report per band; whoever consumed only the first
    /// leaves the rest queued, and a later read would match those stale reports
    /// instead of its own reply.
    fn drain_input(&self) {
        let mut buf = [0u8; 1 + REPORT_LEN];
        while matches!(self.device.read_timeout(&mut buf, 0), Ok(n) if n > 0) {}
    }

    /// Send one report (`payload` is zero-padded to the report length and prefixed
    /// with the report id). Honors the dry-run guard and paces consecutive packets.
    fn send(&self, payload: &[u8]) -> Result<(), String> {
        let mut buf = [0u8; 1 + REPORT_LEN];
        buf[0] = REPORT_ID;
        buf[1..1 + payload.len()].copy_from_slice(payload);
        if dry_run() {
            eprintln!(
                "[hw dry-run] walkplay send {:02x?}",
                &buf[..1 + payload.len()]
            );
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
            let payload = if buf[0] == REPORT_ID {
                &buf[1..n]
            } else {
                &buf[..n]
            };
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

/// Build the 63-byte EQ-write payload for one band slot. Identical layout to the
/// Moondrop packet except byte 35 carries the EQ slot the band belongs to.
fn write_packet(index: u8, band: &HwBand, fs: f64) -> Vec<u8> {
    let mut p = vec![0u8; REPORT_LEN];
    p[0] = CMD_WRITE;
    p[1] = CMD_PEQ_VALUES;
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
    p[35] = CUSTOM_SLOT;
    p
}

/// Build the apply-all payload that loads the written coefficients into the
/// running DSP registers (the reference tool's "TEMP_WRITE" step).
fn apply_packet() -> Vec<u8> {
    vec![CMD_WRITE, CMD_TEMP_WRITE, 0x04, 0x00, 0x00, 0xFF, 0xFF]
}

/// Build the global-gain payload for a requested `pregain` (dB, ≤ 0). The
/// register is one signed whole-dB byte and sits on top of the hardware's fixed
/// −5 dB buffer, so it receives only the excess: `min(0, pregain + 5)`.
fn global_gain_packet(pregain: f64) -> Vec<u8> {
    let reg = (pregain - GAIN_BUFFER_DB).min(0.0).round().max(-60.0) as i8;
    vec![CMD_WRITE, CMD_GLOBAL_GAIN, 0x02, 0x00, reg as u8]
}

/// One biquad coefficient as 4 little-endian bytes: `round(c * 2^30)` kept to its
/// low 32 bits, mirroring the reference JS encoder's 32-bit wrap.
fn coeff_bytes(c: f64) -> [u8; 4] {
    let low32 = (c * COEFF_SCALE).round().rem_euclid(4_294_967_296.0) as u32;
    low32.to_le_bytes()
}

/// A signed 16-bit little-endian value (two's-complement low 16 bits).
fn le16(v: i64) -> [u8; 2] {
    (v.rem_euclid(65_536) as u16).to_le_bytes()
}

/// Decode the version reply: ASCII starting at payload byte 3 (the Space Pro
/// capture shows `"1.0"`). Falls back to the raw numeric form if not text.
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
    HwBand {
        kind,
        freq,
        gain,
        q,
    }
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
        assert_eq!(p[1], CMD_PEQ_VALUES);
        assert_eq!(p[2], 0x18);
        assert_eq!(p[4], 3); // band index
        assert_eq!(u16::from_le_bytes([p[27], p[28]]), 1000); // freq
        assert_eq!(u16::from_le_bytes([p[29], p[30]]), 256); // Q * 256
        assert_eq!(i16::from_le_bytes([p[31], p[32]]), 6 * 256); // gain * 256
        assert_eq!(p[33], TYPE_PK);
        assert_eq!(p[35], CUSTOM_SLOT); // slot tag — differs from the DHA15's 7
    }

    #[test]
    fn global_gain_register_carries_only_the_excess_below_the_buffer() {
        // Hardware already provides −5 dB: a −3 dB preamp writes 0…
        assert_eq!(global_gain_packet(-3.0), vec![0x01, 0x03, 0x02, 0x00, 0]);
        // …and a −7 dB preamp writes the −2 dB excess (0xFE two's-complement).
        assert_eq!(global_gain_packet(-7.0), vec![0x01, 0x03, 0x02, 0x00, 0xFE]);
        // Never positive, even for a (nonsensical) boosted request.
        assert_eq!(global_gain_packet(2.0)[4], 0);
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
    fn identify_matches_the_space_pro_product_string() {
        let info = DeviceInfo {
            vendor_id: 0x3302,
            product_id: 0x4307,
            product: "TANCHJIM-SPACE PRO AT".to_string(),
            manufacturer: "TANCHJIM".to_string(),
            path: "p".to_string(),
            usage_page: 0x000C,
        };
        let (model, profile) = identify(&info).expect("should identify");
        assert_eq!(model, "Space Pro");
        assert_eq!(profile.max_filters, 10);
        assert!(profile.user_pregain);

        // Same string on another vendor id is not claimed.
        let other = DeviceInfo {
            vendor_id: 0x1234,
            ..info
        };
        assert!(identify(&other).is_none());
    }

    /// Real-hardware smoke test. Ignored by default (needs a connected Space Pro);
    /// run with: `cargo test -p fastpeq -- --ignored space_pro_roundtrip --nocapture`.
    /// Writes to RAM only (no flash commit) and restores a flat band after.
    #[test]
    #[ignore]
    fn space_pro_roundtrip() {
        let devices = super::super::detect().expect("enumeration should succeed");
        let dev_info = devices
            .iter()
            .find(|d| d.model.contains("Space Pro"))
            .expect("a Space Pro should be connected");
        let mut dev = super::super::open(&dev_info.id).expect("open Space Pro");
        let version = dev.version().expect("read version");
        println!("Space Pro firmware: {version}");

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
        dev.push(&bands, -6.0, false).expect("push bands");
        let read_back = dev.pull().expect("pull bands");
        println!("read back: {:?}", &read_back[..3.min(read_back.len())]);
        let first = &read_back[0];
        assert!((first.freq - 1000.0).abs() < 2.0, "freq: {}", first.freq);
        assert!((first.gain - 6.0).abs() < 0.5, "gain: {}", first.gain);
        let shelf = &read_back[1];
        assert_eq!(shelf.kind, HwFilterType::LowShelf, "shelf type survives");
        assert!(
            (shelf.freq - 120.0).abs() < 2.0,
            "shelf freq: {}",
            shelf.freq
        );
        assert!(
            (shelf.gain - (-4.0)).abs() < 0.5,
            "shelf gain: {}",
            shelf.gain
        );

        // Restore flat.
        dev.push(&[], 0.0, false).expect("restore flat");
    }

    /// Raw-bytes probe: dump the bulk EQ read, a per-band read, then write one
    /// test band + apply and re-read, with and without slot activation — to pin
    /// down the slot byte and whether writes land. RAM only. Run with:
    /// `cargo test -p fastpeq -- --ignored space_pro_raw_probe --nocapture`.
    #[test]
    #[ignore]
    fn space_pro_raw_probe() {
        let devices = super::super::detect().expect("enumeration should succeed");
        let dev_info = devices
            .iter()
            .find(|d| d.model.contains("Space Pro"))
            .expect("a Space Pro should be connected");
        let dev = WalkplayDevice {
            device: super::super::hid::open(&dev_info.id).expect("open"),
            profile: space_pro_profile(),
            slot_checked: true,
        };

        let dump = |label: &str, p: &Result<Vec<u8>, String>| match p {
            Ok(p) => println!("{label}: {:02x?}", p),
            Err(e) => println!("{label}: no reply ({e})"),
        };

        dev.send(&[CMD_READ, CMD_PEQ_VALUES, 0x00]).unwrap();
        dump("bulk EQ read", &dev.read_reply(CMD_READ, CMD_PEQ_VALUES));

        dev.send(&[CMD_READ, CMD_PEQ_VALUES, 0x00, 0x00, 0, 0x00])
            .unwrap();
        dump("band 0 before", &dev.read_reply(CMD_READ, CMD_PEQ_VALUES));

        // Write a distinctive band into slot index 0 and apply.
        let band = HwBand {
            kind: HwFilterType::Peak,
            freq: 1234.0,
            gain: 5.0,
            q: 2.0,
        };
        dev.send(&write_packet(0, &band, 96_000.0)).unwrap();
        dev.send(&apply_packet()).unwrap();
        std::thread::sleep(Duration::from_millis(200));

        dev.send(&[CMD_READ, CMD_PEQ_VALUES, 0x00, 0x00, 0, 0x00])
            .unwrap();
        dump(
            "band 0 after write+apply",
            &dev.read_reply(CMD_READ, CMD_PEQ_VALUES),
        );

        // Try activating the Custom slot, rewrite, re-read.
        dev.send(&[CMD_WRITE, CMD_FLASH_EQ, 0x01, CUSTOM_SLOT, 0x00])
            .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        dev.send(&write_packet(0, &band, 96_000.0)).unwrap();
        dev.send(&apply_packet()).unwrap();
        std::thread::sleep(Duration::from_millis(200));

        dev.send(&[CMD_READ, CMD_PEQ_VALUES, 0x00, 0x00, 0, 0x00])
            .unwrap();
        dump(
            "band 0 after activate(101)+write+apply",
            &dev.read_reply(CMD_READ, CMD_PEQ_VALUES),
        );
        dev.send(&[CMD_READ, CMD_PEQ_VALUES, 0x00]).unwrap();
        dump(
            "bulk EQ read after activate",
            &dev.read_reply(CMD_READ, CMD_PEQ_VALUES),
        );

        // Restore a flat band 0 (leaves the device audibly unchanged at 0 dB).
        dev.send(&write_packet(0, &FLAT_BAND, 96_000.0)).unwrap();
        dev.send(&apply_packet()).unwrap();
    }

    /// Probe the global-gain register round-trip on real hardware. Run with:
    /// `cargo test -p fastpeq -- --ignored space_pro_gain_probe --nocapture`.
    #[test]
    #[ignore]
    fn space_pro_gain_probe() {
        let devices = super::super::detect().expect("enumeration should succeed");
        let dev_info = devices
            .iter()
            .find(|d| d.model.contains("Space Pro"))
            .expect("a Space Pro should be connected");
        let dev = WalkplayDevice {
            device: super::super::hid::open(&dev_info.id).expect("open"),
            profile: space_pro_profile(),
            slot_checked: true,
        };

        let read_gain = |label: &str| -> Option<i8> {
            dev.send(&[CMD_READ, CMD_GLOBAL_GAIN, 0x00]).unwrap();
            match dev.read_reply(CMD_READ, CMD_GLOBAL_GAIN) {
                Ok(p) => {
                    let v = p.get(4).map(|&b| b as i8);
                    println!(
                        "{label}: {:02x?} (byte4 as i8: {v:?} dB)",
                        &p[..p.len().min(8)]
                    );
                    v
                }
                Err(e) => {
                    println!("{label}: no reply ({e})");
                    None
                }
            }
        };

        // Also show the current slot (payload byte 35 of the bulk EQ read).
        dev.send(&[CMD_READ, CMD_PEQ_VALUES, 0x00]).unwrap();
        match dev.read_reply(CMD_READ, CMD_PEQ_VALUES) {
            Ok(p) => println!("current slot (byte 35): {:?}", p.get(35)),
            Err(e) => println!("bulk EQ read: no reply ({e})"),
        }

        let baseline = read_gain("baseline global gain");
        dev.send(&global_gain_packet(-7.0)).unwrap(); // → register −2
        std::thread::sleep(Duration::from_millis(100));
        read_gain("after requesting −7 dB (expect register −2)");
        // Restore exactly what was there before the probe.
        let restore = baseline.unwrap_or(0);
        dev.send(&[CMD_WRITE, CMD_GLOBAL_GAIN, 0x02, 0x00, restore as u8])
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        read_gain("after restoring the baseline");
    }
}
