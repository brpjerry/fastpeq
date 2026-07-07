//! Walkplay-platform USB HID parametric-EQ driver (validated against the
//! Tanchjim Space Pro, protocol scheme "No16").
//!
//! Walkplay publishes no protocol spec; this is a native Rust port of the
//! community reverse-engineering in [`jeromeof/devicePEQ`] (`walkplayHidHandler.js`
//! plus the `SchemeNo16` device group and the Space Pro USB capture). The wire
//! codec is the Moondrop-family format in [`crate::moondrop_family`]; what's
//! Walkplay-specific stays here:
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
use crate::moondrop_family as family;
use crate::protocol::{self, FLAT_BAND, dry_run};
use family::{CMD_READ, CMD_WRITE, REPORT_ID, REPORT_LEN};
use fastpeq_core::{HardwareProfile, HwBand};
use hidapi::HidDevice;
use std::time::Duration;

// Walkplay-specific command bytes (byte 1 of a report; family::CMD_EQ_BAND is
// the shared per-band packet).
/// Write with no args: persist registers to flash. With `(enable, slot)` args:
/// activate an EQ slot.
const CMD_FLASH_EQ: u8 = 0x01;
/// The 1-byte signed global-gain register (dB) — the working pregain path.
const CMD_GLOBAL_GAIN: u8 = 0x03;
const CMD_RESET_EQ: u8 = 0x05;
/// Apply the written coefficients to the running DSP registers (all bands).
const CMD_TEMP_WRITE: u8 = 0x0A;
const CMD_RESET_FLASH: u8 = 0x17;

/// The writable "Custom" EQ slot every band packet is tagged with.
const CUSTOM_SLOT: u8 = 101;

/// Fixed DAC-stage headroom Walkplay hardware always applies; the global-gain
/// register only carries what's needed beyond it.
const GAIN_BUFFER_DB: f64 = -5.0;

/// Gap between consecutive reports within one push — the reference tool waits
/// 20 ms between band writes so the device's MCU keeps up.
const INTER_PACKET: Duration = Duration::from_millis(20);

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
            self.send(&[CMD_READ, family::CMD_EQ_BAND, 0x00, 0x00, i as u8, 0x00])?;
            let p = self.read_reply(CMD_READ, family::CMD_EQ_BAND)?;
            bands.push(family::decode_band(&p));
        }
        Ok(bands)
    }

    fn version(&mut self) -> Result<String, String> {
        if dry_run() {
            return Ok("dry-run".to_string());
        }
        self.drain_input();
        self.send(&[CMD_READ, family::CMD_VER, 0x00])?;
        let p = self.read_reply(CMD_READ, family::CMD_VER)?;
        Ok(family::decode_version(&p))
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
        self.send(&[CMD_READ, family::CMD_EQ_BAND, 0x00])?;
        let active = self
            .read_reply(CMD_READ, family::CMD_EQ_BAND)
            .ok()
            .and_then(|p| p.get(35).copied());
        if active != Some(CUSTOM_SLOT) {
            // Activate: CMD_FLASH_EQ with (enable = 1, slot) args.
            self.send(&[CMD_WRITE, CMD_FLASH_EQ, 0x01, CUSTOM_SLOT, 0x00])?;
        }
        self.slot_checked = true;
        Ok(())
    }

    /// Discard queued input reports. The bulk EQ read (`0x80 0x09` with no band
    /// index) streams one report per band; whoever consumed only the first
    /// leaves the rest queued, and a later read would match those stale reports
    /// instead of its own reply.
    fn drain_input(&self) {
        protocol::drain_input(&self.device, REPORT_LEN);
    }

    fn send(&self, payload: &[u8]) -> Result<(), String> {
        protocol::send_report(
            &self.device,
            "walkplay",
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

/// The family band packet, tagged with the Walkplay Custom slot.
fn write_packet(index: u8, band: &HwBand, fs: f64) -> Vec<u8> {
    family::write_packet(index, band, fs, CUSTOM_SLOT)
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
        assert_eq!(p[4], 3); // band index
        assert_eq!(u16::from_le_bytes([p[27], p[28]]), 1000); // freq
        assert_eq!(u16::from_le_bytes([p[29], p[30]]), 256); // Q * 256
        assert_eq!(i16::from_le_bytes([p[31], p[32]]), 6 * 256); // gain * 256
        assert_eq!(p[33], 2); // TYPE_PK
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
    /// run with: `cargo test -p fastpeq-hw -- --ignored space_pro_roundtrip --nocapture`.
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
    /// `cargo test -p fastpeq-hw -- --ignored space_pro_raw_probe --nocapture`.
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

        dev.send(&[CMD_READ, family::CMD_EQ_BAND, 0x00]).unwrap();
        dump(
            "bulk EQ read",
            &dev.read_reply(CMD_READ, family::CMD_EQ_BAND),
        );

        dev.send(&[CMD_READ, family::CMD_EQ_BAND, 0x00, 0x00, 0, 0x00])
            .unwrap();
        dump(
            "band 0 before",
            &dev.read_reply(CMD_READ, family::CMD_EQ_BAND),
        );

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

        dev.send(&[CMD_READ, family::CMD_EQ_BAND, 0x00, 0x00, 0, 0x00])
            .unwrap();
        dump(
            "band 0 after write+apply",
            &dev.read_reply(CMD_READ, family::CMD_EQ_BAND),
        );

        // Try activating the Custom slot, rewrite, re-read.
        dev.send(&[CMD_WRITE, CMD_FLASH_EQ, 0x01, CUSTOM_SLOT, 0x00])
            .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        dev.send(&write_packet(0, &band, 96_000.0)).unwrap();
        dev.send(&apply_packet()).unwrap();
        std::thread::sleep(Duration::from_millis(200));

        dev.send(&[CMD_READ, family::CMD_EQ_BAND, 0x00, 0x00, 0, 0x00])
            .unwrap();
        dump(
            "band 0 after activate(101)+write+apply",
            &dev.read_reply(CMD_READ, family::CMD_EQ_BAND),
        );
        dev.send(&[CMD_READ, family::CMD_EQ_BAND, 0x00]).unwrap();
        dump(
            "bulk EQ read after activate",
            &dev.read_reply(CMD_READ, family::CMD_EQ_BAND),
        );

        // Restore a flat band 0 (leaves the device audibly unchanged at 0 dB).
        dev.send(&write_packet(0, &FLAT_BAND, 96_000.0)).unwrap();
        dev.send(&apply_packet()).unwrap();
    }

    /// Probe the global-gain register round-trip on real hardware. Run with:
    /// `cargo test -p fastpeq-hw -- --ignored space_pro_gain_probe --nocapture`.
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
        dev.send(&[CMD_READ, family::CMD_EQ_BAND, 0x00]).unwrap();
        match dev.read_reply(CMD_READ, family::CMD_EQ_BAND) {
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
