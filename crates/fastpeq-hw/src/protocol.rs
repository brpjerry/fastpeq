//! Shared USB HID plumbing for the device drivers: report framing and pacing,
//! read-until-match with timeout, the dry-run guard, and the flat filler band.
//! Everything here is protocol-agnostic — per-family wire formats live in the
//! driver modules (and [`crate::moondrop_family`] for the codec Moondrop and
//! Walkplay share).

use fastpeq_core::{HwBand, HwFilterType};
use hidapi::HidDevice;
use std::time::{Duration, Instant};

/// How long to wait for a device reply to a read command.
pub(crate) const READ_TIMEOUT: Duration = Duration::from_millis(1000);

/// A flat (0 dB) band used to clear unused device slots.
pub(crate) const FLAT_BAND: HwBand = HwBand {
    kind: HwFilterType::Peak,
    freq: 1000.0,
    gain: 0.0,
    q: 1.0,
};

/// Whether `FASTPEQ_HW_DRYRUN` is set — log packets instead of writing, for
/// safe first-contact debugging with an unverified protocol.
pub(crate) fn dry_run() -> bool {
    std::env::var_os("FASTPEQ_HW_DRYRUN").is_some_and(|v| !v.is_empty())
}

/// Send one report: `payload` zero-padded to `report_len` and prefixed with
/// `report_id`. Honors the dry-run guard (logging under `tag`) and paces
/// consecutive packets by sleeping `pace` after the write, so the device's MCU
/// keeps up within one push. (Throttling *between* pushes is the worker's job.)
pub(crate) fn send_report(
    dev: &HidDevice,
    tag: &str,
    report_id: u8,
    report_len: usize,
    payload: &[u8],
    pace: Duration,
) -> Result<(), String> {
    let mut buf = vec![0u8; 1 + report_len];
    buf[0] = report_id;
    buf[1..1 + payload.len()].copy_from_slice(payload);
    if dry_run() {
        eprintln!("[hw dry-run] {tag} send {:02x?}", &buf[..1 + payload.len()]);
        return Ok(());
    }
    dev.write(&buf).map_err(|e| e.to_string())?;
    std::thread::sleep(pace);
    Ok(())
}

/// Read input reports until one whose payload `matches`, or time out. The HID
/// report id, if present, is stripped so payload indices match the reference
/// decoders.
pub(crate) fn read_matching(
    dev: &HidDevice,
    report_id: u8,
    report_len: usize,
    matches: impl Fn(&[u8]) -> bool,
) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; 1 + report_len];
    let deadline = Instant::now() + READ_TIMEOUT;
    while Instant::now() < deadline {
        let n = dev.read_timeout(&mut buf, 200).map_err(|e| e.to_string())?;
        if n == 0 {
            continue;
        }
        let payload = if buf[0] == report_id {
            &buf[1..n]
        } else {
            &buf[..n]
        };
        if matches(payload) {
            return Ok(payload.to_vec());
        }
    }
    Err("Timed out waiting for a device reply".to_string())
}

/// Discard any queued input reports so a read matches its own reply, not a
/// stale one (e.g. the leftovers of a bulk read that streamed several reports).
pub(crate) fn drain_input(dev: &HidDevice, report_len: usize) {
    let mut buf = vec![0u8; 1 + report_len];
    while matches!(dev.read_timeout(&mut buf, 0), Ok(n) if n > 0) {}
}
