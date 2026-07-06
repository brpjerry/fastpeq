//! Hardware EQ offload: dividing a [`Config`]'s parametric bands between a
//! hardware parametric-EQ device and Equalizer APO software, plus the biquad math
//! the device's coefficient packets need.
//!
//! This module is pure and transport-agnostic — the actual USB/HID I/O lives in
//! the Tauri shell (`src-tauri/src/hardware/`), mirroring how output-device
//! switching lives in `src-tauri/src/audio.rs` rather than in the core. Keeping
//! the split logic and the RBJ biquad computation here makes both unit-testable
//! without a device attached.
//!
//! The model: a device can run the *first X* bands of a preset (X = its band
//! budget); everything that doesn't fit — overflow bands, per-channel bands, and
//! filter types the device can't represent — stays in software. The RBJ formulas
//! mirror the live-curve math in `src/lib/eq.ts`, so the hardware and the on-screen
//! response curve agree.

use crate::apo::model::{Channel, Config, Filter, FilterKind, Line};
use crate::tone::Tone;
use serde::{Deserialize, Serialize};
use std::f64::consts::{FRAC_1_SQRT_2, PI};

/// Sample rate the software/APO stage runs at, for the auto-preamp peak math —
/// matches `SAMPLE_RATE` in `src/lib/eq.ts` (the device's own biquads use the
/// per-device rate in [`HardwareProfile::sample_rate`]).
const SOFTWARE_FS: f64 = 48_000.0;

/// The EQ-routing selection: whether to offload, and if so which bands go to the
/// hardware device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OffloadMode {
    /// Don't offload — every band stays in Equalizer APO. (Offload off.)
    #[default]
    ApoOnly,
    /// The first X offloadable bands in document order.
    FirstX,
    /// The X bands that alter the response most — largest area under the
    /// bell/shelf (Σ|magnitude| across the spectrum).
    LargestChange,
    /// The X bands that let Equalizer APO's preamp sit closest to 0: the boosts
    /// move to the device (whose pregain absorbs the headroom), so the software
    /// side needs little or no attenuation. Also recomputes the software preamp.
    MinimizePreamp,
    /// The EQ runs entirely on the device and Equalizer APO stays flat: the
    /// software side keeps no preamp and no filters at all. Every candidate goes
    /// to hardware when it fits; past the budget the most impactful bands win
    /// (the [`LargestChange`](Self::LargestChange) ranking) and the rest are
    /// dropped — muted, not run in software. The tone overlay is also withheld
    /// while this mode is engaged (the shell's responsibility).
    HardwareOnly,
}

/// The parametric-filter classes a hardware PEQ device can represent. fastpeq's
/// richer [`FilterKind`] set is reduced to these three; kinds that don't map (low
/// pass, high pass, notch, …) are not offload candidates and stay in software.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwFilterType {
    Peak,
    LowShelf,
    HighShelf,
}

impl HwFilterType {
    /// Map a fastpeq [`FilterKind`] to the hardware class, or `None` if no device
    /// of this family can represent it. Both the fixed-slope (`LowShelf`) and
    /// adjustable-Q (`LowShelfQ`) shelves collapse to the same hardware class.
    pub fn from_kind(kind: FilterKind) -> Option<Self> {
        use FilterKind::*;
        Some(match kind {
            Peak => HwFilterType::Peak,
            LowShelf | LowShelfQ => HwFilterType::LowShelf,
            HighShelf | HighShelfQ => HwFilterType::HighShelf,
            _ => return None,
        })
    }

    /// The default Q for this class when the source filter carries none (fixed
    /// shelves) — matches `defaultQ` in `src/lib/eq.ts`.
    fn default_q(self) -> f64 {
        match self {
            HwFilterType::Peak => 1.0,
            _ => FRAC_1_SQRT_2,
        }
    }
}

/// What a hardware PEQ device can do — its band budget and parameter ranges. Each
/// device's driver in the shell constructs one of these; it lives here so the pure
/// [`split`] can consult it.
#[derive(Debug, Clone, PartialEq)]
pub struct HardwareProfile {
    /// Maximum number of bands the device can run — the "X" in "first X filters".
    pub max_filters: usize,
    /// Sample rate (Hz) the device computes its biquads at (96 kHz for Moondrop).
    /// [`biquad_coeffs`] uses this so the coefficients match the device's DSP.
    pub sample_rate: f64,
    /// Inclusive gain range (dB) a band is clamped to.
    pub gain_range: (f64, f64),
    /// Inclusive Q range a band is clamped to.
    pub q_range: (f64, f64),
    /// Inclusive centre/corner-frequency range (Hz) a band is clamped to.
    pub freq_range: (f64, f64),
    /// Whether the device can represent low-shelf bands.
    pub supports_low_shelf: bool,
    /// Whether the device can represent high-shelf bands.
    pub supports_high_shelf: bool,
    /// Whether the device's input headroom (pregain) is host-adjustable — i.e. the
    /// UI shows a Device preamp slider the user can turn.
    pub user_pregain: bool,
    /// Whether the device only *applies* an EQ/pregain write once it's committed to
    /// flash — its live (RAM) registers stage the value but don't take effect until
    /// the save (the DHA15). The editor flashes it on mouse release so a live edit
    /// still takes hold, without a flash per drag frame.
    pub commit_to_apply: bool,
    /// How long (ms) the editor freezes the write controls after a flash commit — the
    /// device's audio drops out while it applies, so we hold off writing again until
    /// it's back. Only meaningful when [`commit_to_apply`](Self::commit_to_apply);
    /// default 500.
    pub commit_delay_ms: u32,
}

/// A single band assigned to the hardware device, already clamped to the device's
/// ranges. The driver turns this into device-specific coefficient packets via
/// [`biquad_coeffs`].
#[derive(Debug, Clone, PartialEq)]
pub struct HwBand {
    pub kind: HwFilterType,
    pub freq: f64,
    pub gain: f64,
    pub q: f64,
}

/// The result of dividing a config between hardware and software.
#[derive(Debug, Clone, PartialEq)]
pub struct Split {
    /// Bands assigned to the hardware device, in document order, clamped to range.
    pub hw: Vec<HwBand>,
    /// The remaining config for Equalizer APO: the offloaded `Filter:` lines
    /// removed, everything else (preamp, raw lines, per-channel/overflow filters,
    /// the tone block) preserved in place and in order.
    pub software: Config,
    /// Suggested device pregain (dB, `≤ 0`) so the hardware bands' combined boost
    /// doesn't clip the device's input: `-max(0, peak_gain_db(hw))`.
    pub hw_pregain: f64,
}

/// Divide `config`'s parametric bands between a hardware device and software.
///
/// A band is an *offload candidate* iff it is enabled, applies to both channels
/// ([`Channel::Both`]), and its type maps to a hardware class the device supports.
/// Up to `profile.max_filters` candidates are chosen for hardware per `mode`
/// (parameters clamped to the device's ranges); every other line — unselected
/// candidates, per-channel bands, disabled or unsupported-type bands, the preamp,
/// comments, and the tone block — stays in `software`, untouched and in order.
///
/// [`OffloadMode::HardwareOnly`] is the exception: its software side is *flat* —
/// every `Preamp:` and `Filter:` line is removed (whatever didn't make the device
/// is dropped, not kept), and only non-EQ lines (comments, `Device:`/`Include:`
/// raw lines) survive. Equalizer APO becomes a pass-through.
///
/// See [`OffloadMode`] for the selection strategies. For
/// [`OffloadMode::MinimizePreamp`] the caller is expected to set the software
/// master preamp via [`auto_preamp`] / [`set_master_preamp`] — that step needs the
/// global tone overlay (composed downstream), which this pure function doesn't see.
pub fn split(config: &Config, profile: &HardwareProfile, mode: OffloadMode) -> Split {
    let fs = profile.sample_rate;

    // Offload candidates with their source line index, in document order.
    let candidates: Vec<(usize, HwBand)> = config
        .lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| match line {
            Line::Filter(f) if f.enabled && f.channel == Channel::Both => {
                offload_band(f, profile).map(|band| (i, band))
            }
            _ => None,
        })
        .collect();

    let chosen = select(&candidates, profile.max_filters, fs, mode);

    let hw: Vec<HwBand> = candidates
        .iter()
        .filter(|(i, _)| chosen.contains(i))
        .map(|(_, band)| band.clone())
        .collect();

    let software = Config {
        lines: config
            .lines
            .iter()
            .enumerate()
            .filter(|(i, line)| match mode {
                // Hardware-only leaves APO flat: no preamp, no filters — offloaded
                // or not. Only non-EQ lines pass through.
                OffloadMode::HardwareOnly => !matches!(line, Line::Filter(_) | Line::Preamp { .. }),
                _ => !chosen.contains(i),
            })
            .map(|(_, line)| line.clone())
            .collect(),
    };

    let hw_pregain = -peak_gain_db(&hw, fs).max(0.0);
    Split {
        hw,
        software,
        hw_pregain,
    }
}

/// The positions of the `Filter:` lines — counted in document order over *all*
/// filters, enabled or not — that [`split`] would offload to the device under
/// `mode`. For the UI's per-band "→ hardware" indicator; empty when nothing
/// offloads. Shares the candidate set and [`select`] with `split`, so it always
/// matches what `split` actually sends.
pub fn selected_filter_positions(
    config: &Config,
    profile: &HardwareProfile,
    mode: OffloadMode,
) -> Vec<usize> {
    // Same candidates as `split`, but keyed by filter position (not line index) so
    // the result maps straight onto the editor's band rows.
    let mut candidates: Vec<(usize, HwBand)> = Vec::new();
    let mut filter_pos = 0usize;
    for line in &config.lines {
        if let Line::Filter(f) = line {
            let pos = filter_pos;
            filter_pos += 1;
            if f.enabled
                && f.channel == Channel::Both
                && let Some(band) = offload_band(f, profile)
            {
                candidates.push((pos, band));
            }
        }
    }
    let mut chosen = select(&candidates, profile.max_filters, profile.sample_rate, mode);
    chosen.sort_unstable();
    chosen
}

/// Choose which candidate line indices go to hardware (up to `max`), per `mode`.
fn select(candidates: &[(usize, HwBand)], max: usize, fs: f64, mode: OffloadMode) -> Vec<usize> {
    match mode {
        // ApoOnly offloads nothing (a session normally isn't even open for it).
        OffloadMode::ApoOnly => Vec::new(),
        OffloadMode::FirstX => candidates.iter().take(max).map(|(i, _)| *i).collect(),
        // HardwareOnly shares the impact ranking: when the whole set fits it takes
        // everything, and past the budget dropping the least impactful bands (they
        // are muted, not kept in software) loses the least sound.
        OffloadMode::LargestChange | OffloadMode::HardwareOnly => {
            // Area computed once per candidate, not inside the comparator.
            let mut ranked: Vec<(usize, f64)> = candidates
                .iter()
                .map(|(i, band)| (*i, band_area(band, fs)))
                .collect();
            // Largest area first; ties keep document order for stability.
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
            ranked.into_iter().take(max).map(|(i, _)| i).collect()
        }
        OffloadMode::MinimizePreamp => select_min_preamp(candidates, max, fs),
    }
}

/// Total area under a band's magnitude response (Σ|magnitude| over the log-spaced
/// probe frequencies) — how much the band alters the sound. Wide/low-Q bands and
/// shelves score higher than narrow peaks of the same gain.
fn band_area(band: &HwBand, fs: f64) -> f64 {
    band_response(band, fs).iter().map(|m| m.abs()).sum()
}

/// Greedily move the band whose removal most lowers the combined software boost
/// peak, until that peak can't drop further or the budget runs out; then spend any
/// leftover budget on the largest-area remaining *boosts*. Only boosts are ever
/// offloaded — they're what consumes APO headroom, so moving them to the device
/// lets APO's preamp sit at just the cut + tone level. Cuts stay in software: they
/// need no headroom, and offloading one could re-expose a boost that didn't fit
/// (raising the very preamp this mode minimizes).
///
/// Each candidate's magnitude response over the probe grid is computed once up
/// front, so every trial in the greedy loop is a vector sum rather than a fresh
/// round of biquad evaluations — this runs on every live-edit write while
/// offloading, so it has to stay cheap.
fn select_min_preamp(candidates: &[(usize, HwBand)], max: usize, fs: f64) -> Vec<usize> {
    let responses: Vec<Vec<f64>> = candidates
        .iter()
        .map(|(_, band)| band_response(band, fs))
        .collect();
    // Peak of the summed response of the candidates still in software (`taken`
    // marks the ones moved to hardware). An all-zero sum (every band taken)
    // peaks at 0.0, matching `peak_gain_db` for an empty set.
    let software_peak = |taken: &[bool]| -> f64 {
        let probes = responses.first().map_or(0, Vec::len);
        (0..probes)
            .map(|k| {
                responses
                    .iter()
                    .zip(taken)
                    .filter(|&(_, &t)| !t)
                    .map(|(r, _)| r[k])
                    .sum::<f64>()
            })
            .fold(f64::NEG_INFINITY, f64::max)
    };

    let mut taken = vec![false; candidates.len()];
    let mut chosen: Vec<usize> = Vec::new();
    while chosen.len() < max {
        let current = software_peak(&taken);
        if current <= 0.0 {
            break; // no positive peak left — APO preamp is already ~0
        }
        // The unchosen band whose removal yields the lowest remaining peak.
        let mut best: Option<(usize, f64)> = None;
        for pos in 0..candidates.len() {
            if taken[pos] {
                continue;
            }
            taken[pos] = true;
            let peak = software_peak(&taken);
            taken[pos] = false;
            match best {
                Some((_, bp)) if peak >= bp => {}
                _ => best = Some((pos, peak)),
            }
        }
        match best {
            Some((pos, peak)) if peak < current => {
                taken[pos] = true;
                chosen.push(candidates[pos].0);
            }
            _ => break, // only cuts remain; removing one would raise the peak
        }
    }

    if chosen.len() < max {
        // Leftover budget takes only the remaining boosts (gain > 0): a boost on
        // the device can't raise the software peak, whereas offloading a cut both
        // wastes headroom-free slots and can strand a boost in APO. Reuses the
        // precomputed responses for the area ranking (see `select`).
        let mut rest: Vec<(usize, f64)> = candidates
            .iter()
            .enumerate()
            .filter(|&(pos, _)| !taken[pos] && candidates[pos].1.gain > 0.0)
            .map(|(pos, (i, _))| (*i, responses[pos].iter().map(|m| m.abs()).sum()))
            .collect();
        rest.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        chosen.extend(rest.into_iter().take(max - chosen.len()).map(|(i, _)| i));
    }
    chosen
}

/// The master preamp (dB, `≤ 0`) that keeps a software config from clipping — i.e.
/// Equalizer APO's "Auto Preamp" value: the negated peak boost of its peaking/shelf
/// bands *plus* the global tone overlay. Other kinds (LP/HP/notch/…) don't boost and
/// are ignored; channels are summed (conservative — never under-estimates the
/// headroom one channel needs). Evaluated at the software sample rate, matching the
/// live curve in `src/lib/eq.ts`.
///
/// Used for [`OffloadMode::MinimizePreamp`]: with the boosts moved to the device,
/// this preamp sits as close to 0 as the remaining (cut) bands and the tone overlay
/// allow.
pub fn auto_preamp(config: &Config, tone: &Tone) -> f64 {
    let mut bands: Vec<HwBand> = config
        .filters()
        .filter(|f| f.enabled)
        .filter_map(band_unclamped)
        .collect();
    bands.extend(tone.filters().iter().filter_map(band_unclamped));
    -peak_gain_db(&bands, SOFTWARE_FS).max(0.0)
}

/// Map any peaking/shelf filter to an [`HwBand`] for magnitude math, *without*
/// clamping to a device's ranges (used to gauge the software side's headroom).
fn band_unclamped(f: &Filter) -> Option<HwBand> {
    let kind = HwFilterType::from_kind(f.kind)?;
    Some(HwBand {
        kind,
        freq: f.freq,
        gain: f.gain.unwrap_or(0.0),
        q: f.q.unwrap_or(kind.default_q()),
    })
}

/// Set the master (`Both`-channel) preamp gain — replacing an existing master
/// preamp line, or inserting one at the top. Per-channel balance trims are left
/// untouched.
pub fn set_master_preamp(config: &mut Config, value: f64) {
    for line in &mut config.lines {
        if let Line::Preamp {
            channel: Channel::Both,
            gain,
        } = line
        {
            *gain = value;
            return;
        }
    }
    config.lines.insert(
        0,
        Line::Preamp {
            gain: value,
            channel: Channel::Both,
        },
    );
}

/// Convert a filter to a hardware band if the device can represent it, clamping
/// each parameter to the device's ranges. `None` for unsupported types or a shelf
/// the device lacks.
fn offload_band(f: &Filter, profile: &HardwareProfile) -> Option<HwBand> {
    let kind = HwFilterType::from_kind(f.kind)?;
    match kind {
        HwFilterType::LowShelf if !profile.supports_low_shelf => return None,
        HwFilterType::HighShelf if !profile.supports_high_shelf => return None,
        _ => {}
    }
    let clamp = |v: f64, (lo, hi): (f64, f64)| v.clamp(lo, hi);
    Some(HwBand {
        kind,
        freq: clamp(f.freq, profile.freq_range),
        gain: clamp(f.gain.unwrap_or(0.0), profile.gain_range),
        q: clamp(f.q.unwrap_or(kind.default_q()), profile.q_range),
    })
}

/// Normalized biquad coefficients `[b0, b1, b2, a1, a2]` (with `a0 == 1`) for a
/// band at sample rate `fs`, using the RBJ Audio EQ Cookbook — the same formulas
/// the live curve uses in `src/lib/eq.ts`.
///
/// These are in the standard normalized form. A device driver packs them into its
/// own convention; e.g. the Moondrop chips expect the feedback coefficients negated
/// (`[b0, b1, b2, -a1, -a2]`), scaled and little-endian.
pub fn biquad_coeffs(kind: HwFilterType, freq: f64, gain: f64, q: f64, fs: f64) -> [f64; 5] {
    let w0 = 2.0 * PI * freq / fs;
    let cw = w0.cos();
    let sw = w0.sin();
    let alpha = sw / (2.0 * q);
    let a = 10f64.powf(gain / 40.0);

    let (b0, b1, b2, a0, a1, a2) = match kind {
        HwFilterType::Peak => (
            1.0 + alpha * a,
            -2.0 * cw,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cw,
            1.0 - alpha / a,
        ),
        HwFilterType::LowShelf => {
            let s = 2.0 * a.sqrt() * alpha;
            (
                a * (a + 1.0 - (a - 1.0) * cw + s),
                2.0 * a * (a - 1.0 - (a + 1.0) * cw),
                a * (a + 1.0 - (a - 1.0) * cw - s),
                a + 1.0 + (a - 1.0) * cw + s,
                -2.0 * (a - 1.0 + (a + 1.0) * cw),
                a + 1.0 + (a - 1.0) * cw - s,
            )
        }
        HwFilterType::HighShelf => {
            let s = 2.0 * a.sqrt() * alpha;
            (
                a * (a + 1.0 + (a - 1.0) * cw + s),
                -2.0 * a * (a - 1.0 + (a + 1.0) * cw),
                a * (a + 1.0 + (a - 1.0) * cw - s),
                a + 1.0 - (a - 1.0) * cw + s,
                2.0 * (a - 1.0 - (a + 1.0) * cw),
                a + 1.0 - (a - 1.0) * cw - s,
            )
        }
    };
    [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0]
}

/// Magnitude (dB) of one band's biquad at `freq`, evaluated at sample rate `fs`.
/// Mirrors `magnitudeDb` in `src/lib/eq.ts`. Test-only convenience — production
/// paths evaluate over the whole probe grid via [`band_response`], which computes
/// the coefficients once instead of per point.
#[cfg(test)]
fn magnitude_db(band: &HwBand, freq: f64, fs: f64) -> f64 {
    let coeffs = biquad_coeffs(band.kind, band.freq, band.gain, band.q, fs);
    coeffs_magnitude_db(&coeffs, freq, fs)
}

/// Magnitude (dB) at `freq` of a normalized biquad, from its coefficients — the
/// evaluation half of the response math, split out so a band's coefficients are
/// computed once and reused across a whole frequency grid.
fn coeffs_magnitude_db(&[b0, b1, b2, a1, a2]: &[f64; 5], freq: f64, fs: f64) -> f64 {
    let w = 2.0 * PI * freq / fs;
    let (c1, s1) = ((-w).cos(), (-w).sin());
    let (c2, s2) = ((-2.0 * w).cos(), (-2.0 * w).sin());
    let nr = b0 + b1 * c1 + b2 * c2;
    let ni = b1 * s1 + b2 * s2;
    let dr = 1.0 + a1 * c1 + a2 * c2; // a0 == 1 (normalized)
    let di = a1 * s1 + a2 * s2;
    let den = dr.hypot(di);
    if den == 0.0 {
        return 0.0;
    }
    20.0 * (nr.hypot(ni) / den).log10()
}

/// One band's magnitude (dB) at every probe frequency, with the biquad
/// coefficients computed once for the whole grid. The peak/area math sums and
/// scans these vectors, so no trig runs inside selection loops.
fn band_response(band: &HwBand, fs: f64) -> Vec<f64> {
    let coeffs = biquad_coeffs(band.kind, band.freq, band.gain, band.q, fs);
    probe_freqs()
        .map(|f| coeffs_magnitude_db(&coeffs, f, fs))
        .collect()
}

/// Log-spaced probe frequencies (20 Hz – 20 kHz), matching the live curve's
/// resolution in `src/lib/eq.ts`.
fn probe_freqs() -> impl Iterator<Item = f64> {
    const N: usize = 240;
    (0..N).map(|i| 20.0 * (20000f64 / 20.0).powf(i as f64 / (N - 1) as f64))
}

/// The peak combined boost (dB) of a set of hardware bands across the audible
/// band — the most any single frequency is amplified once the bands sum. Used to
/// size the device pregain so the boost doesn't clip. Returns `0.0` for no bands;
/// can be negative when the bands only ever cut.
pub fn peak_gain_db(bands: &[HwBand], fs: f64) -> f64 {
    if bands.is_empty() {
        return 0.0;
    }
    let responses: Vec<Vec<f64>> = bands.iter().map(|b| band_response(b, fs)).collect();
    (0..responses[0].len())
        .map(|k| responses.iter().map(|r| r[k]).sum::<f64>())
        .fold(f64::NEG_INFINITY, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apo::model::Filter;
    use crate::tone::Tone;

    /// A representative 8-band profile (the DHA15's), so split/clamp tests don't
    /// depend on the shell's driver constants.
    fn profile() -> HardwareProfile {
        HardwareProfile {
            max_filters: 8,
            sample_rate: 96000.0,
            gain_range: (-12.0, 12.0),
            q_range: (0.1, 10.0),
            freq_range: (20.0, 20000.0),
            supports_low_shelf: true,
            supports_high_shelf: true,
            user_pregain: true,
            commit_to_apply: false,
            commit_delay_ms: 500,
        }
    }

    fn peak_filter(freq: f64, gain: f64, q: f64) -> Line {
        Line::Filter(Filter::peak(freq, gain, q))
    }

    fn low_shelf(freq: f64, gain: f64) -> Line {
        Line::Filter(Filter {
            kind: FilterKind::LowShelfQ,
            ..Filter::peak(freq, gain, FRAC_1_SQRT_2)
        })
    }

    /// The master (`Both`) preamp gain in a config, if any.
    fn master_preamp(c: &Config) -> Option<f64> {
        c.lines.iter().find_map(|l| match l {
            Line::Preamp {
                channel: Channel::Both,
                gain,
            } => Some(*gain),
            _ => None,
        })
    }

    #[test]
    fn split_takes_first_x_and_overflows_the_rest_to_software() {
        let mut lines = vec![Line::Preamp {
            gain: -6.0,
            channel: Channel::Both,
        }];
        // 10 peaking bands; only the first 8 fit on the device.
        for i in 0..10 {
            lines.push(peak_filter(100.0 * (i as f64 + 1.0), 3.0, 1.0));
        }
        let split = split(&Config { lines }, &profile(), OffloadMode::FirstX);

        assert_eq!(split.hw.len(), 8);
        // Overflow (2 bands) plus the preamp remain in software.
        assert_eq!(split.software.filters().count(), 2);
        // First-X leaves the master preamp untouched (current behavior).
        assert_eq!(master_preamp(&split.software), Some(-6.0));
        // The hardware bands are the first eight, in order.
        assert_eq!(split.hw[0].freq, 100.0);
        assert_eq!(split.hw[7].freq, 800.0);
    }

    #[test]
    fn unsupported_kinds_and_per_channel_bands_stay_in_software() {
        let lowpass = Line::Filter(Filter {
            enabled: true,
            kind: FilterKind::LowPass,
            freq: 5000.0,
            gain: None,
            q: None,
            index: None,
            channel: Channel::Both,
        });
        let left_only = Line::Filter(Filter {
            channel: Channel::Left,
            ..Filter::peak(1000.0, 2.0, 1.0)
        });
        let disabled = Line::Filter(Filter {
            enabled: false,
            ..Filter::peak(2000.0, 2.0, 1.0)
        });
        let normal = peak_filter(500.0, 2.0, 1.0);

        let split = split(
            &Config {
                lines: vec![lowpass, left_only, disabled, normal],
            },
            &profile(),
            OffloadMode::FirstX,
        );

        // Only the one plain both-channel peaking band is offloaded.
        assert_eq!(split.hw.len(), 1);
        assert_eq!(split.hw[0].freq, 500.0);
        // The other three lines survive in software, in order.
        assert_eq!(split.software.filters().count(), 3);
    }

    #[test]
    fn shelves_unsupported_when_profile_disallows() {
        let mut p = profile();
        p.supports_low_shelf = false;
        let split = split(
            &Config {
                lines: vec![low_shelf(80.0, 4.0)],
            },
            &p,
            OffloadMode::FirstX,
        );
        assert!(split.hw.is_empty());
        assert_eq!(split.software.filters().count(), 1);
    }

    #[test]
    fn parameters_are_clamped_to_device_ranges() {
        // Gain and Q beyond the device's range get clamped.
        let split = split(
            &Config {
                lines: vec![peak_filter(1000.0, 20.0, 50.0)],
            },
            &profile(),
            OffloadMode::FirstX,
        );
        assert_eq!(split.hw.len(), 1);
        assert_eq!(split.hw[0].gain, 12.0); // clamped from +20
        assert_eq!(split.hw[0].q, 10.0); // clamped from 50
    }

    #[test]
    fn pregain_is_negative_for_boost_and_zero_for_cut() {
        let boost = split(
            &Config {
                lines: vec![peak_filter(1000.0, 6.0, 1.0)],
            },
            &profile(),
            OffloadMode::FirstX,
        );
        assert!(boost.hw_pregain < 0.0, "boost should reserve headroom");
        assert!(
            (boost.hw_pregain + 6.0).abs() < 0.5,
            "≈ -6 dB for a +6 dB peak"
        );

        let cut = split(
            &Config {
                lines: vec![peak_filter(1000.0, -6.0, 1.0)],
            },
            &profile(),
            OffloadMode::FirstX,
        );
        assert_eq!(cut.hw_pregain, 0.0, "a pure cut needs no headroom");
    }

    #[test]
    fn largest_change_mode_offloads_the_most_impactful_bands() {
        // Two device slots; doc order is small → big-shelf → big-peak.
        let p = HardwareProfile {
            max_filters: 2,
            ..profile()
        };
        let config = Config {
            lines: vec![
                peak_filter(1000.0, 1.0, 4.0), // tiny, narrow → least area
                low_shelf(120.0, 6.0),         // wide shelf → large area
                peak_filter(2000.0, 8.0, 1.0), // tall peak → large area
            ],
        };
        let split = split(&config, &p, OffloadMode::LargestChange);

        assert_eq!(split.hw.len(), 2);
        let hw_freqs: Vec<f64> = split.hw.iter().map(|b| b.freq).collect();
        assert!(hw_freqs.contains(&120.0) && hw_freqs.contains(&2000.0));
        // The small narrow peak is the one left in software.
        assert_eq!(split.software.filters().count(), 1);
        assert_eq!(split.software.filters().next().unwrap().freq, 1000.0);
    }

    #[test]
    fn hardware_only_leaves_software_flat() {
        // A preamp, a comment, 2 both-channel bands, a left-only band, and a
        // disabled band. Hardware gets the candidates; software keeps ONLY the
        // comment — no preamp, no filters of any kind.
        let config = Config {
            lines: vec![
                Line::Preamp {
                    gain: -6.0,
                    channel: Channel::Both,
                },
                Line::Raw("# a comment".to_string()),
                peak_filter(100.0, 3.0, 1.0),
                peak_filter(1000.0, -2.0, 2.0),
                Line::Filter(Filter {
                    channel: Channel::Left,
                    ..Filter::peak(2000.0, 2.0, 1.0)
                }),
                Line::Filter(Filter {
                    enabled: false,
                    ..Filter::peak(4000.0, 2.0, 1.0)
                }),
            ],
        };
        let split = split(&config, &profile(), OffloadMode::HardwareOnly);

        assert_eq!(split.hw.len(), 2); // the two both-channel candidates
        assert_eq!(
            split.software.lines,
            vec![Line::Raw("# a comment".to_string())],
            "APO must be a pass-through: no preamp, no filters"
        );
    }

    #[test]
    fn hardware_only_overflow_drops_the_least_impactful() {
        // Two device slots, three candidates: the tiny narrow peak is the one
        // dropped (largest-change ranking), and it does NOT fall back to software.
        let p = HardwareProfile {
            max_filters: 2,
            ..profile()
        };
        let config = Config {
            lines: vec![
                peak_filter(1000.0, 1.0, 4.0), // tiny, narrow → dropped
                low_shelf(120.0, 6.0),
                peak_filter(2000.0, 8.0, 1.0),
            ],
        };
        let split = split(&config, &p, OffloadMode::HardwareOnly);

        let hw_freqs: Vec<f64> = split.hw.iter().map(|b| b.freq).collect();
        assert_eq!(hw_freqs, vec![120.0, 2000.0]); // document order preserved
        assert!(
            split.software.lines.is_empty(),
            "the overflow band is muted"
        );
    }

    #[test]
    fn hardware_only_positions_match_the_impact_ranking() {
        let p = HardwareProfile {
            max_filters: 2,
            ..profile()
        };
        let config = Config {
            lines: vec![
                peak_filter(1000.0, 1.0, 4.0), // pos 0 — least impactful
                low_shelf(120.0, 6.0),         // pos 1
                peak_filter(2000.0, 8.0, 1.0), // pos 2
            ],
        };
        let pos = selected_filter_positions(&config, &p, OffloadMode::HardwareOnly);
        assert_eq!(pos, vec![1, 2]);
    }

    #[test]
    fn minimize_preamp_mode_offloads_the_boost_not_the_cut() {
        // One device slot. Doc order puts the cut first, the boost second — so this
        // differs from First-X (which would take the cut).
        let p = HardwareProfile {
            max_filters: 1,
            ..profile()
        };
        let config = Config {
            lines: vec![
                peak_filter(5000.0, -6.0, 1.0), // a cut (first)
                peak_filter(500.0, 6.0, 1.0),   // a boost (second)
            ],
        };
        let split = split(&config, &p, OffloadMode::MinimizePreamp);

        // The boost is offloaded so the device's pregain carries the headroom; the
        // cut stays in software (its preamp is set separately via `auto_preamp`).
        assert_eq!(split.hw.len(), 1);
        assert_eq!(split.hw[0].freq, 500.0);
        assert!(split.hw_pregain < 0.0);
        assert_eq!(split.software.filters().count(), 1);
        assert_eq!(split.software.filters().next().unwrap().freq, 5000.0);
    }

    #[test]
    fn minimize_preamp_gives_leftover_budget_to_boosts_not_cuts() {
        // Budget 2. The greedy takes the dominant boost (pos 0), dropping the peak
        // to ~0 — but a masked boost (pos 1, buried under the pos-2 cut) is still in
        // software. The leftover slot must go to that boost, not the larger-area
        // cut: otherwise a positive filter is stranded in APO (inflating its preamp)
        // while a cut needlessly lands on the device — the reported bug.
        let p = HardwareProfile {
            max_filters: 2,
            ..profile()
        };
        let config = Config {
            lines: vec![
                peak_filter(1000.0, 8.0, 1.0),  // pos 0: dominant boost
                peak_filter(5000.0, 2.0, 1.0),  // pos 1: boost, masked by the cut ↓
                peak_filter(5000.0, -8.0, 1.0), // pos 2: cut (larger area than pos 1)
            ],
        };
        let split = split(&config, &p, OffloadMode::MinimizePreamp);

        // Both boosts go to the device; the cut stays in APO (needs no headroom).
        assert_eq!(split.hw.len(), 2);
        assert!(
            split.hw.iter().all(|b| b.gain > 0.0),
            "only boosts should be offloaded, got {:?}",
            split.hw
        );
        let hw_freqs: Vec<f64> = split.hw.iter().map(|b| b.freq).collect();
        assert!(hw_freqs.contains(&1000.0) && hw_freqs.contains(&5000.0));
        assert_eq!(split.software.filters().count(), 1);
        assert_eq!(split.software.filters().next().unwrap().gain, Some(-8.0));
    }

    #[test]
    fn selected_positions_are_filter_indices_matching_split() {
        let p = HardwareProfile {
            max_filters: 2,
            ..profile()
        };
        // A preamp (not a filter) precedes three peaks; positions count only filters.
        let config = Config {
            lines: vec![
                Line::Preamp {
                    gain: -3.0,
                    channel: Channel::Both,
                },
                peak_filter(100.0, 3.0, 1.0), // filter pos 0
                peak_filter(200.0, 3.0, 1.0), // filter pos 1
                peak_filter(300.0, 3.0, 1.0), // filter pos 2
            ],
        };
        let pos = selected_filter_positions(&config, &p, OffloadMode::FirstX);
        assert_eq!(pos, vec![0, 1]); // first two filters, preamp ignored

        // The positions name exactly the bands `split` offloads (by frequency).
        let split = split(&config, &p, OffloadMode::FirstX);
        let hw_freqs: Vec<f64> = split.hw.iter().map(|b| b.freq).collect();
        assert_eq!(hw_freqs, vec![100.0, 200.0]);
    }

    #[test]
    fn selected_positions_count_skipped_filters() {
        let p = HardwareProfile {
            max_filters: 2,
            ..profile()
        };
        // A left-only band is not a candidate, but still occupies a filter position.
        let config = Config {
            lines: vec![
                peak_filter(100.0, 3.0, 1.0), // pos 0 (both)
                Line::Filter(Filter {
                    channel: Channel::Left,
                    ..Filter::peak(200.0, 3.0, 1.0)
                }), // pos 1 (left — skipped)
                peak_filter(300.0, 3.0, 1.0), // pos 2 (both)
            ],
        };
        let pos = selected_filter_positions(&config, &p, OffloadMode::FirstX);
        assert_eq!(pos, vec![0, 2]);
    }

    #[test]
    fn auto_preamp_covers_boosts_cuts_and_tone() {
        let flat = Tone::default();

        // A +8 dB boost needs ≈ -8 dB of preamp.
        let boost = Config {
            lines: vec![peak_filter(1000.0, 8.0, 1.0)],
        };
        let p = auto_preamp(&boost, &flat);
        assert!((p + 8.0).abs() < 0.3, "≈ -8 dB, was {p}");

        // A pure cut needs no preamp.
        let cut = Config {
            lines: vec![peak_filter(1000.0, -8.0, 1.0)],
        };
        assert_eq!(auto_preamp(&cut, &flat), 0.0);

        // The tone overlay counts too: a cut-only config plus a +6 dB bass tone
        // still needs headroom for the tone.
        let bass = Tone {
            bass: 6.0,
            ..Tone::default()
        };
        let with_tone = auto_preamp(&cut, &bass);
        assert!(
            with_tone < -5.0,
            "tone boost should be covered, was {with_tone}"
        );
    }

    #[test]
    fn peaking_magnitude_matches_gain_at_center() {
        // A peaking filter's magnitude at its centre frequency equals its gain.
        let band = HwBand {
            kind: HwFilterType::Peak,
            freq: 1000.0,
            gain: 6.0,
            q: 1.0,
        };
        assert!((magnitude_db(&band, 1000.0, 96000.0) - 6.0).abs() < 0.05);

        // A flat (0 dB) band is ~0 dB everywhere.
        let flat = HwBand { gain: 0.0, ..band };
        assert!(magnitude_db(&flat, 1000.0, 96000.0).abs() < 1e-9);
        assert!(magnitude_db(&flat, 50.0, 96000.0).abs() < 1e-9);
    }

    #[test]
    fn low_shelf_magnitude_approaches_gain_at_dc() {
        let band = HwBand {
            kind: HwFilterType::LowShelf,
            freq: 200.0,
            gain: 6.0,
            q: FRAC_1_SQRT_2,
        };
        // Well below the corner the shelf reaches its full gain; well above it is flat.
        assert!((magnitude_db(&band, 20.0, 96000.0) - 6.0).abs() < 0.5);
        assert!(magnitude_db(&band, 15000.0, 96000.0).abs() < 0.5);
    }
}
