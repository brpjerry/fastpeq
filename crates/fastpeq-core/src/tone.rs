//! The global "tone control" overlay: three knobs — bass, mid, treble — layered
//! on top of whatever preset is active.
//!
//! The overlay is written into `config.txt` as a block fenced by sentinel
//! comments, so it can be recomposed when a knob moves or a preset is applied,
//! and stripped back out before matching the live config against stored presets.
//! The knob values themselves are persisted separately (a sidecar), so the
//! filters here are kept out of the user's editable bands entirely.

use crate::apo::model::{Channel, Config, Filter, FilterKind, Line};
use serde::{Deserialize, Serialize};

/// Sentinel comments fencing the managed tone block in `config.txt`. APO ignores
/// comment lines, and the parser keeps them as [`Line::Raw`], so they round-trip.
const TONE_BEGIN: &str = "# fastpeq tone overlay (managed — edit with the app's tone knobs)";
const TONE_END: &str = "# end fastpeq tone overlay";

// Filter shapes the knobs drive. Bass is a Harman-style low shelf; mid a broad
// peak; treble a high shelf. Each knob sets its filter's gain in dB.
const BASS_FREQ: f64 = 105.0;
const BASS_Q: f64 = 0.71;
const MID_FREQ: f64 = 1000.0;
const MID_Q: f64 = 0.7;
const TREBLE_FREQ: f64 = 4000.0;
const TREBLE_Q: f64 = 0.71;

/// Bass / mid / treble gains (dB) plus two routing switches. All gains zero and
/// both switches off means "flat" — no overlay written. The bool fields default
/// so a `tone.json` written before they existed still deserializes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Tone {
    pub bass: f64,
    pub mid: f64,
    pub treble: f64,
    /// Flip polarity (180°) on both channels.
    #[serde(default)]
    pub invert: bool,
    /// Swap the left and right channels.
    #[serde(default)]
    pub swap: bool,
}

impl Tone {
    pub fn is_flat(&self) -> bool {
        self.bass == 0.0 && self.mid == 0.0 && self.treble == 0.0 && !self.invert && !self.swap
    }

    /// The filters this tone produces, skipping any knob left at 0 dB.
    fn filters(&self) -> Vec<Filter> {
        let mut out = Vec::new();
        let mut push = |kind, freq, gain: f64, q| {
            if gain != 0.0 {
                out.push(Filter {
                    enabled: true,
                    kind,
                    freq,
                    gain: Some(gain),
                    q: Some(q),
                    index: None,
                    channel: Channel::Both,
                });
            }
        };
        push(FilterKind::LowShelfQ, BASS_FREQ, self.bass, BASS_Q);
        push(FilterKind::Peak, MID_FREQ, self.mid, MID_Q);
        push(FilterKind::HighShelfQ, TREBLE_FREQ, self.treble, TREBLE_Q);
        out
    }

    /// The APO `Copy:` routing line for the polarity / swap switches, or `None`
    /// when both are off. Sits last in the block so it acts on the fully
    /// processed signal — swapping the final output channels.
    fn copy_line(&self) -> Option<String> {
        if !self.invert && !self.swap {
            return None;
        }
        let (l_src, r_src) = if self.swap { ("R", "L") } else { ("L", "R") };
        let coef = if self.invert { "-1*" } else { "" };
        Some(format!("Copy: L={coef}{l_src} R={coef}{r_src}"))
    }

    /// The fenced config lines for this tone, or empty when flat.
    pub fn lines(&self) -> Vec<Line> {
        let filters = self.filters();
        let copy = self.copy_line();
        if filters.is_empty() && copy.is_none() {
            return Vec::new();
        }
        let mut lines = Vec::with_capacity(filters.len() + 3);
        lines.push(Line::Raw(TONE_BEGIN.to_string()));
        lines.extend(filters.into_iter().map(Line::Filter));
        if let Some(c) = copy {
            lines.push(Line::Raw(c));
        }
        lines.push(Line::Raw(TONE_END.to_string()));
        lines
    }
}

/// Remove a previously-written tone block (the sentinels and the filters between
/// them) from a config, leaving the base EQ. A no-op when no block is present.
pub fn strip(config: &Config) -> Config {
    let lines = &config.lines;
    let Some(begin) = lines.iter().position(|l| is_marker(l, TONE_BEGIN)) else {
        return config.clone();
    };
    // The end sentinel sits after begin; if it's missing (hand-edited file),
    // drop everything from the begin sentinel onward.
    let end = lines[begin..]
        .iter()
        .position(|l| is_marker(l, TONE_END))
        .map(|rel| begin + rel)
        .unwrap_or(lines.len() - 1);
    // A swap overlay flips the per-channel preamps at compose (see `compose`), so
    // undo that here — keeping `strip(compose(base, tone)) == base` and the
    // active-preset match intact even while swap is on.
    let had_swap = lines[begin..=end.min(lines.len() - 1)]
        .iter()
        .any(|l| matches!(l, Line::Raw(s) if copy_is_swap(s)));
    let mut kept = lines[..begin].to_vec();
    kept.extend_from_slice(&lines[(end + 1).min(lines.len())..]);
    let base = Config { lines: kept };
    if had_swap {
        flip_preamp_channels(&base)
    } else {
        base
    }
}

/// Lay the tone overlay over a base config. The base is first stripped of any
/// existing block, so recomposing repeatedly never stacks duplicate overlays.
///
/// When the overlay swaps L/R, the final `Copy:` line swaps the *output*
/// channels — which would flip the preset's balance (a one-sided preamp) to the
/// wrong side. EQ filters should follow the swap, but a balance is physical, so
/// flip the per-channel preamps first to cancel the swap for them only.
pub fn compose(base: &Config, tone: &Tone) -> Config {
    let mut out = strip(base);
    if tone.swap {
        out = flip_preamp_channels(&out);
    }
    out.lines.extend(tone.lines());
    out
}

/// Swap `Left`/`Right` on every `Preamp:` line, leaving filters and `Both`/other
/// channels untouched. Its own inverse, so compose/strip round-trip cleanly.
fn flip_preamp_channels(config: &Config) -> Config {
    let lines = config
        .lines
        .iter()
        .map(|l| match l {
            Line::Preamp {
                gain,
                channel: Channel::Left,
            } => Line::Preamp {
                gain: *gain,
                channel: Channel::Right,
            },
            Line::Preamp {
                gain,
                channel: Channel::Right,
            } => Line::Preamp {
                gain: *gain,
                channel: Channel::Left,
            },
            other => other.clone(),
        })
        .collect();
    Config { lines }
}

/// Whether a `Copy:` line swaps the channels — i.e. its left output is sourced
/// from the right input (`L=R` or `L=-1*R`), as opposed to an invert-only
/// `L=-1*L`. Format is fixed by [`Tone::copy_line`].
fn copy_is_swap(line: &str) -> bool {
    let s = line.trim();
    if !s.starts_with("Copy:") {
        return false;
    }
    s.split("L=")
        .nth(1)
        .map(|src| src.trim_start_matches("-1*").starts_with('R'))
        .unwrap_or(false)
}

fn is_marker(line: &Line, marker: &str) -> bool {
    matches!(line, Line::Raw(s) if s.trim() == marker)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Config {
        Config {
            lines: vec![
                Line::Preamp {
                    gain: -3.0,
                    channel: Channel::Both,
                },
                Line::Filter(Filter::peak(1000.0, 2.0, 1.0)),
            ],
        }
    }

    #[test]
    fn flat_tone_writes_nothing() {
        let tone = Tone::default();
        assert!(tone.is_flat());
        assert!(tone.lines().is_empty());
        assert_eq!(compose(&base(), &tone), base());
    }

    #[test]
    fn compose_then_strip_is_identity() {
        let tone = Tone {
            bass: 4.0,
            mid: 0.0,
            treble: -2.0,
            ..Default::default()
        };
        let composed = compose(&base(), &tone);

        // Only the non-zero knobs become filters, fenced by the sentinels.
        let tone_filters: Vec<_> = composed
            .lines
            .iter()
            .skip(2) // base preamp + filter
            .filter_map(|l| match l {
                Line::Filter(f) => Some(f),
                _ => None,
            })
            .collect();
        assert_eq!(tone_filters.len(), 2);
        assert_eq!(tone_filters[0].kind, FilterKind::LowShelfQ);
        assert_eq!(tone_filters[0].freq, BASS_FREQ);
        assert_eq!(tone_filters[1].kind, FilterKind::HighShelfQ);

        // Stripping the overlay restores the original base exactly.
        assert_eq!(strip(&composed), base());
    }

    #[test]
    fn switches_emit_copy_line() {
        // Swap only: a Copy that exchanges the channels.
        let swap = Tone {
            swap: true,
            ..Default::default()
        };
        assert!(!swap.is_flat());
        let lines = swap.lines();
        assert!(
            matches!(&lines[1], Line::Raw(s) if s == "Copy: L=R R=L"),
            "{lines:?}"
        );

        // Invert only: a Copy that negates both channels.
        let inv = Tone {
            invert: true,
            ..Default::default()
        };
        let lines = inv.lines();
        assert!(
            matches!(&lines[1], Line::Raw(s) if s == "Copy: L=-1*L R=-1*R"),
            "{lines:?}"
        );

        // Both, over a base preset: the copy comes last and strips cleanly.
        let both = Tone {
            bass: 3.0,
            invert: true,
            swap: true,
            ..Default::default()
        };
        let composed = compose(&base(), &both);
        let copy = composed.lines.iter().find_map(|l| match l {
            Line::Raw(s) if s.starts_with("Copy:") => Some(s.clone()),
            _ => None,
        });
        assert_eq!(copy.as_deref(), Some("Copy: L=-1*R R=-1*L"));
        assert_eq!(strip(&composed), base());
    }

    #[test]
    fn swap_flips_balance_preamp_but_round_trips() {
        // A preset with a balance trim (cut the left channel) under a Swap L/R overlay.
        let base = Config {
            lines: vec![
                Line::Preamp {
                    gain: -6.0,
                    channel: Channel::Left,
                },
                Line::Filter(Filter::peak(1000.0, 2.0, 1.0)),
            ],
        };
        let composed = compose(
            &base,
            &Tone {
                swap: true,
                ..Default::default()
            },
        );

        // The balance preamp is flipped to the right so the final Copy swap lands
        // it back on the intended (left) output; the EQ filter is left to be
        // swapped by the Copy line.
        assert!(
            composed.lines.iter().any(|l| matches!(
                l,
                Line::Preamp { gain, channel: Channel::Right } if *gain == -6.0
            )),
            "{composed:?}"
        );
        assert!(
            !composed
                .lines
                .iter()
                .any(|l| matches!(l, Line::Preamp { channel: Channel::Left, .. }))
        );

        // Round-trip identity holds, so the active-preset match survives swap.
        assert_eq!(strip(&composed), base);
    }

    #[test]
    fn recompose_replaces_rather_than_stacks() {
        let first = compose(
            &base(),
            &Tone {
                bass: 4.0,
                ..Default::default()
            },
        );
        let second = compose(
            &first,
            &Tone {
                treble: 3.0,
                ..Default::default()
            },
        );
        // The old bass overlay is gone; only the new treble one remains.
        assert_eq!(strip(&second), base());
        let tone_filters = second
            .lines
            .iter()
            .filter(|l| matches!(l, Line::Filter(_)))
            .count();
        assert_eq!(tone_filters, 2); // base peak + treble shelf
    }
}
