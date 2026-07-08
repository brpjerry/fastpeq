//! The wire codec shared by the Moondrop-family HID protocol — Moondrop's own
//! devices (DHA15) and the Walkplay platform (Tanchjim Space Pro), whose
//! formats are byte-identical here: report id `0x4B`, 63-byte zero-padded
//! payloads, and per-band packets carrying both a packed biquad (five
//! coefficients scaled by 2^30, little-endian) and readable freq/Q/gain/type
//! fields the device echoes back on read.
//!
//! What differs between the two families stays in their driver modules:
//! command sequences (per-band enable vs apply-all, flash/save commands,
//! pregain registers), packet pacing, and byte 35 of a band packet — the EQ
//! slot tag ([`write_packet`]'s `slot` parameter).

use crate::protocol;
use fastpeq_core::{HwBand, HwFilterType, biquad_coeffs};
use hidapi::HidDevice;

pub(crate) const REPORT_ID: u8 = 0x4B;
/// Payload length of a report (the on-wire report is this + 1 for the id). The
/// EQ write packet is the largest at 63 bytes; shorter commands are
/// zero-padded, as WebHID does for the reference tool.
pub(crate) const REPORT_LEN: usize = 63;

// Command bytes (first two payload bytes of every report).
pub(crate) const CMD_WRITE: u8 = 0x01;
pub(crate) const CMD_READ: u8 = 0x80;
/// The per-band EQ packet (Moondrop calls it "update EQ", Walkplay "PEQ values").
pub(crate) const CMD_EQ_BAND: u8 = 0x09;
/// Firmware-version query.
pub(crate) const CMD_VER: u8 = 0x0C;

// Device filter-type codes.
const TYPE_PK: u8 = 2;
const TYPE_LSQ: u8 = 1;
const TYPE_HSQ: u8 = 3;

/// Biquad coefficient fixed-point scale (2^30).
const COEFF_SCALE: f64 = 1_073_741_824.0;

/// Read input reports until a `(c0, c1)`-tagged reply arrives, or time out.
pub(crate) fn read_reply(dev: &HidDevice, c0: u8, c1: u8) -> Result<Vec<u8>, String> {
    protocol::read_matching(dev, REPORT_ID, REPORT_LEN, |p| {
        p.len() > 1 && p[0] == c0 && p[1] == c1
    })
}

pub(crate) fn type_byte(kind: HwFilterType) -> u8 {
    match kind {
        HwFilterType::Peak => TYPE_PK,
        HwFilterType::LowShelf => TYPE_LSQ,
        HwFilterType::HighShelf => TYPE_HSQ,
    }
}

/// Build the 63-byte EQ-write payload for one band slot. Byte 35 carries the
/// per-family `slot` tag (the DHA15's fixed peqIndex, Walkplay's Custom slot).
pub(crate) fn write_packet(index: u8, band: &HwBand, fs: f64, slot: u8) -> Vec<u8> {
    let mut p = vec![0u8; REPORT_LEN];
    p[0] = CMD_WRITE;
    p[1] = CMD_EQ_BAND;
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
    p[35] = slot;
    p
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
pub(crate) fn le16(v: i64) -> [u8; 2] {
    (v.rem_euclid(65_536) as u16).to_le_bytes()
}

/// Decode the version reply: an ASCII string starting at payload byte 3 (the
/// DHA15 reports `"V0.5.0"`, the Space Pro `"1.0"`); read the printable run.
/// Falls back to the raw numeric form if the payload isn't text. Total:
/// `read_reply` only guarantees the two command bytes, so a short (malformed)
/// reply decodes to zeros rather than panicking the worker thread.
pub(crate) fn decode_version(p: &[u8]) -> String {
    let byte = |i: usize| p.get(i).copied().unwrap_or(0);
    let text: String = p
        .get(3..)
        .unwrap_or_default()
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .collect();
    let text = text.trim().to_string();
    if text.is_empty() {
        format!("{}.{}.{}", byte(3), byte(4), byte(5))
    } else {
        text
    }
}

/// Decode a band from a read reply (inverse of [`write_packet`]'s readable
/// fields). Total, like [`decode_version`]: missing bytes read as zero.
pub(crate) fn decode_band(p: &[u8]) -> HwBand {
    let byte = |i: usize| p.get(i).copied().unwrap_or(0);
    let read_i16 = |lo: usize| i16::from_le_bytes([byte(lo), byte(lo + 1)]) as f64;
    let freq = u16::from_le_bytes([byte(27), byte(28)]) as f64;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_is_inverse_of_encode_readable_fields() {
        let band = HwBand {
            kind: HwFilterType::HighShelf,
            freq: 8000.0,
            gain: -3.5,
            q: 0.7,
        };
        let p = write_packet(0, &band, 96_000.0, 101);
        let back = decode_band(&p);
        assert_eq!(back.kind, HwFilterType::HighShelf);
        assert_eq!(back.freq, 8000.0);
        assert!((back.gain - (-3.5)).abs() < 0.01);
        assert!((back.q - 0.7).abs() < 0.01);
    }

    /// A truncated (malformed) reply must decode to something inert, never
    /// panic — a decoder panic kills the worker thread mid-session.
    #[test]
    fn short_replies_decode_without_panicking() {
        // Two bytes is the minimum read_reply() will hand a decoder.
        let short = [CMD_READ, CMD_VER];
        assert_eq!(decode_version(&short), "0.0.0");
        let band = decode_band(&[CMD_READ, CMD_EQ_BAND]);
        assert_eq!(band.gain, 0.0);
        assert_eq!(band.kind, HwFilterType::Peak);
    }
}
