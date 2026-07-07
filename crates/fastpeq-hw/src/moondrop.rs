//! Moondrop USB HID parametric-EQ driver (validated against the DHA15).
//!
//! Moondrop publishes no protocol spec; this is a native Rust port of the
//! community reverse-engineering in [`jeromeof/devicePEQ`] (`moondropUsbHidHandler.js`
//! plus the `"DHA15"` device entry). The wire codec — report framing, packed
//! biquads, readable band fields — is the Moondrop-family format shared with
//! the Walkplay platform and lives in [`crate::moondrop_family`]; this module
//! keeps what is Moondrop-specific: the per-band register enable, the
//! save-to-flash and pregain commands, and the DHA15's commit-to-apply
//! behavior. Shelf behavior is the least-certain part of the reverse-engineered
//! spec — verify by read-back on real hardware.
//!
//! [`jeromeof/devicePEQ`]: https://github.com/jeromeof/devicePEQ

use super::{DeviceInfo, HardwareEq};
use crate::moondrop_family as family;
use crate::protocol::{self, FLAT_BAND, dry_run};
use family::{CMD_READ, CMD_WRITE, REPORT_ID, REPORT_LEN};
use fastpeq_core::{HardwareProfile, HwBand};
use hidapi::HidDevice;
use std::time::Duration;

// Moondrop-specific command bytes (byte 1 of a report; family::CMD_EQ_BAND is
// the shared per-band packet).
const CMD_COEFF_TO_REG: u8 = 0x0A;
const CMD_SAVE_TO_FLASH: u8 = 0x01;
const CMD_PRE_GAIN: u8 = 0x23;

/// Byte 35 of a band packet — a fixed "peqIndex" tag on Moondrop devices
/// (where Walkplay uses it for the active EQ slot).
const PEQ_INDEX: u8 = 7;

/// Small gap between consecutive reports within one push, so the device's MCU
/// keeps up. (Throttling *between* pushes is the worker's job.)
const INTER_PACKET: Duration = Duration::from_millis(4);

/// Recognize a Moondrop device this driver can drive, returning its model name and
/// PEQ profile. Matches on the USB product string (the DHA15 reports `"DHA15"`).
pub(super) fn identify(info: &DeviceInfo) -> Option<(String, HardwareProfile)> {
    if info.product.to_ascii_uppercase().contains("DHA15") {
        return Some(("DHA15".to_string(), dha15_profile()));
    }
    None
}

/// The DHA15's capabilities (from the reverse-engineered `peq8Band12dBFullShelves`
/// constraints): 8 bands, ±12 dB, Q 0.1–10, peaking + low/high shelf, biquads
/// computed at 96 kHz. It takes a host pregain via `0x23`, sent with every filter
/// write — without one the boosts overdrive its EQ (clipping, and at worst the DSP
/// faults off USB). `user_pregain: true` exposes the Device slider so the value is
/// host-adjustable.
fn dha15_profile() -> HardwareProfile {
    HardwareProfile {
        max_filters: 8,
        sample_rate: 96_000.0,
        gain_range: (-12.0, 12.0),
        q_range: (0.1, 10.0),
        freq_range: (20.0, 20_000.0),
        supports_low_shelf: true,
        supports_high_shelf: true,
        user_pregain: true,
        // The DHA15 only latches a pregain/EQ write on the flash save (0x01); RAM
        // writes stage but don't reach the audio, so the editor flashes on release.
        commit_to_apply: true,
        // Its audio drops out for a moment while the flash applies; freeze edits until
        // it's back.
        commit_delay_ms: 500,
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
        // Send the host pregain alongside the filters: the DHA15 clips (and can fault
        // off USB) if it writes boosts with no pregain packet.
        if self.profile.user_pregain {
            self.send(&pregain_packet(pregain))?;
        }
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
            self.send(&[CMD_READ, family::CMD_EQ_BAND, 0x18, 0x00, i as u8, 0x00])?;
            let p = self.read_reply(CMD_READ, family::CMD_EQ_BAND)?;
            bands.push(family::decode_band(&p));
        }
        Ok(bands)
    }

    fn version(&mut self) -> Result<String, String> {
        if dry_run() {
            return Ok("dry-run".to_string());
        }
        self.send(&[CMD_READ, family::CMD_VER])?;
        let p = self.read_reply(CMD_READ, family::CMD_VER)?;
        Ok(family::decode_version(&p))
    }
}

impl MoondropDevice {
    fn send(&self, payload: &[u8]) -> Result<(), String> {
        protocol::send_report(
            &self.device,
            "moondrop",
            REPORT_ID,
            REPORT_LEN,
            payload,
            INTER_PACKET,
        )
    }

    fn read_reply(&self, c0: u8, c1: u8) -> Result<Vec<u8>, String> {
        family::read_reply(&self.device, c0, c1)
    }
}

/// The family band packet, tagged with the DHA15's fixed peqIndex.
fn write_packet(index: u8, band: &HwBand, fs: f64) -> Vec<u8> {
    family::write_packet(index, band, fs, PEQ_INDEX)
}

/// Build the payload that commits a written slot into the active register set.
fn enable_packet(index: u8) -> Vec<u8> {
    vec![CMD_WRITE, CMD_COEFF_TO_REG, index, 0, 255, 255, 255]
}

/// Build the host-pregain payload (`gain` in dB, scaled by 256, little-endian).
fn pregain_packet(gain: f64) -> Vec<u8> {
    let v = family::le16((gain * 256.0).round() as i64);
    vec![CMD_WRITE, CMD_PRE_GAIN, 0, v[0], v[1]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastpeq_core::HwFilterType;

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
        assert_eq!(p[1], family::CMD_EQ_BAND);
        assert_eq!(p[2], 0x18);
        assert_eq!(p[4], 3); // slot index
        assert_eq!(u16::from_le_bytes([p[27], p[28]]), 1000); // freq
        assert_eq!(u16::from_le_bytes([p[29], p[30]]), 256); // Q * 256
        assert_eq!(i16::from_le_bytes([p[31], p[32]]), 6 * 256); // gain * 256
        assert_eq!(p[33], 2); // TYPE_PK
        assert_eq!(p[35], PEQ_INDEX);
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
    fn pregain_packet_encodes_scaled_gain() {
        let p = pregain_packet(-6.0);
        assert_eq!(p[0], CMD_WRITE);
        assert_eq!(p[1], CMD_PRE_GAIN);
        assert_eq!(i16::from_le_bytes([p[3], p[4]]), -6 * 256);
    }

    /// Times HID enumeration — the cost the app's background reconciler keeps off
    /// the UI path. Run with:
    /// `cargo test -p fastpeq-hw -- --ignored enum_timing --nocapture`.
    #[test]
    #[ignore]
    fn enum_timing() {
        let t = std::time::Instant::now();
        let n = super::super::detect().map(|d| d.len()).unwrap_or(0);
        println!("detect(): {} devices in {:?}", n, t.elapsed());
    }

    /// Correlation smoke test: the DHA15's audio-endpoint friendly name resolves to
    /// its HID device, while an unrelated output does not. Needs the DHA15 connected.
    /// Run with: `cargo test -p fastpeq-hw -- --ignored dha15_correlates`.
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

    /// Probe: is the DHA15's pregain (`0x23`) actually user-controllable?
    ///
    /// The Device preamp slider appears dead on the DHA15. Upstream devicePEQ
    /// *reads* "pregain" from register `0x03` (DAC offset) but *writes* it to
    /// `0x23`, and comments that Moondrop devices auto-compute headroom from the
    /// written biquads. This probe (RAM only, no flash):
    ///   1. reads `0x03` and `0x23` baselines,
    ///   2. writes a boosted band (no pregain) → does `0x03` move by itself?
    ///   3. writes pregain −6 dB via `0x23` → does `0x03` or `0x23` move?
    ///
    /// then restores a flat band and 0 pregain.
    /// Run with: `cargo test -p fastpeq-hw -- --ignored dha15_pregain_probe --nocapture`.
    ///
    /// Result (firmware V0.1): `0x23` never replies to reads, and `0x03` stays at 0
    /// through band writes and pregain writes alike — but that only means the register
    /// isn't read-back-able, not that the write has no effect. The device does need a
    /// pregain packet present (boosts clip and it can fault off USB without one), so
    /// [`dha15_profile`] sends it (`user_pregain: true`).
    #[test]
    #[ignore]
    fn dha15_pregain_probe() {
        const CMD_DAC_OFFSET: u8 = 0x03;
        let devices = super::super::detect().expect("enumeration should succeed");
        let dha = devices
            .iter()
            .find(|d| d.model.contains("DHA15"))
            .expect("a DHA15 should be connected");
        let dev = MoondropDevice {
            device: super::super::hid::open(&dha.id).expect("open DHA15"),
            profile: dha15_profile(),
        };

        let read_reg = |cmd: u8, label: &str| {
            if dev.send(&[CMD_READ, cmd]).is_err() {
                println!("{label}: send failed");
                return;
            }
            match dev.read_reply(CMD_READ, cmd) {
                Ok(p) => {
                    let head = &p[..p.len().min(10)];
                    let byte = |i: usize| p.get(i).copied().unwrap_or(0);
                    let fixed = i16::from_le_bytes([byte(3), byte(4)]) as f64 / 256.0;
                    println!("{label}: {head:02x?}  (bytes3..5 as 8.8 fixed: {fixed} dB)");
                }
                Err(e) => println!("{label}: no reply ({e})"),
            }
        };

        read_reg(CMD_DAC_OFFSET, "baseline 0x03 (DAC offset)");
        read_reg(CMD_PRE_GAIN, "baseline 0x23 (pre-gain)");

        // A +6 dB band, no pregain write: does the device pull 0x03 down itself?
        let boosted = HwBand {
            kind: HwFilterType::Peak,
            freq: 1000.0,
            gain: 6.0,
            q: 1.0,
        };
        dev.send(&write_packet(0, &boosted, 96_000.0)).unwrap();
        dev.send(&enable_packet(0)).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        read_reg(CMD_DAC_OFFSET, "0x03 after +6 dB band (no pregain write)");
        read_reg(CMD_PRE_GAIN, "0x23 after +6 dB band (no pregain write)");

        // Now write pregain −6 dB the way push() does.
        dev.send(&pregain_packet(-6.0)).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        read_reg(CMD_DAC_OFFSET, "0x03 after pregain −6 via 0x23");
        read_reg(CMD_PRE_GAIN, "0x23 after pregain −6 via 0x23");

        // Restore: flat band, 0 pregain.
        dev.send(&write_packet(0, &FLAT_BAND, 96_000.0)).unwrap();
        dev.send(&enable_packet(0)).unwrap();
        dev.send(&pregain_packet(0.0)).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        read_reg(CMD_DAC_OFFSET, "0x03 after restore");
        read_reg(CMD_PRE_GAIN, "0x23 after restore");
    }

    /// Real-hardware smoke test. Ignored by default (needs a connected DHA15);
    /// run with: `cargo test -p fastpeq-hw -- --ignored dha15_roundtrip`.
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
        dev.push(std::slice::from_ref(&band), -6.0, false)
            .expect("push band");
        let read_back = dev.pull().expect("pull bands");
        let first = &read_back[0];
        assert!((first.freq - 1000.0).abs() < 2.0, "freq: {}", first.freq);
        assert!((first.gain - 6.0).abs() < 0.5, "gain: {}", first.gain);
    }
}
