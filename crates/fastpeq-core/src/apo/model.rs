//! The structured representation of an Equalizer APO configuration.
//!
//! A [`Config`] is an ordered list of [`Line`]s. We only model the two things
//! the editor needs to manipulate — `Preamp:` and `Filter:` — and keep
//! everything else as [`Line::Raw`] so it survives a parse/serialize cycle byte
//! for byte. This keeps the MVP small while guaranteeing we never mangle a
//! user's `Include:`, `Device:`, `GraphicEQ:` or `Convolution:` lines.

use serde::{Deserialize, Serialize};

/// Which channel(s) a preamp or filter applies to — Equalizer APO's stateful
/// `Channel:` directive. `Both` is the default (no scoping). `Other` preserves
/// channel specs we don't model (e.g. surround) so they round-trip untouched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", content = "spec", rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Both,
    Left,
    Right,
    Other(String),
}

/// A whole `config.txt` (or a preset file), in document order.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Config {
    pub lines: Vec<Line>,
}

impl Config {
    /// An empty configuration (APO passthrough — no processing).
    pub fn new() -> Self {
        Config::default()
    }

    /// Iterate over only the modeled `Filter:` lines.
    pub fn filters(&self) -> impl Iterator<Item = &Filter> {
        self.lines.iter().filter_map(|l| match l {
            Line::Filter(f) => Some(f),
            _ => None,
        })
    }

    /// The `Preamp:` value, if one is present.
    pub fn preamp(&self) -> Option<f64> {
        self.lines.iter().find_map(|l| match l {
            Line::Preamp { gain, .. } => Some(*gain),
            _ => None,
        })
    }
}

/// A single line of an APO configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum Line {
    /// `Preamp: <value> dB`, scoped to `channel`.
    Preamp { gain: f64, channel: Channel },
    /// `Filter[ N]: ON|OFF <type> [<slope> dB] Fc <hz> Hz [Gain <db> dB] [Q <q>]`
    Filter(Filter),
    /// Any line we do not model — comments, blank lines, `Include:`,
    /// `Device:`, `Channel:`, `GraphicEQ:`, `Convolution:`, unsupported filter
    /// variants, etc. Preserved exactly as written.
    Raw(String),
}

/// A parametric filter band.
///
/// `gain` and `q` are optional because not every filter type carries them
/// (a `LP`/`HP` has only `Fc`). Representing presence with [`Option`] is what
/// makes `parse(serialize(filter)) == filter` hold regardless of type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub enabled: bool,
    pub kind: FilterKind,
    /// Centre/corner frequency `Fc`, in Hz. Always present.
    pub freq: f64,
    /// Gain in dB (peaking and shelving filters).
    pub gain: Option<f64>,
    /// Quality factor `Q`.
    pub q: Option<f64>,
    /// Shelf slope in dB per octave — `LSC 12 dB …` — given instead of a `Q`.
    /// Only for the types where [`FilterKind::takes_slope`]; never alongside
    /// `q`. Defaulted so JSON written before this field existed still loads.
    #[serde(default)]
    pub slope: Option<f64>,
    /// The label from a numbered `Filter N:` line, or `None` for bare `Filter:`.
    pub index: Option<u32>,
    /// Which channel(s) this band applies to.
    pub channel: Channel,
}

impl Filter {
    /// A peaking (parametric) band — the workhorse used by AutoEQ/oratory presets.
    pub fn peak(freq: f64, gain: f64, q: f64) -> Self {
        Filter {
            enabled: true,
            kind: FilterKind::Peak,
            freq,
            gain: Some(gain),
            q: Some(q),
            slope: None,
            index: None,
            channel: Channel::Both,
        }
    }

    /// The centre frequency and Q of the biquad Equalizer APO runs for this
    /// band — what the response curve and a hardware device need. For a shelf
    /// these aren't always the line's `Fc` and `Q`; APO's `BiQuadFilterFactory`
    /// and `BiQuadFilter` resolve them as:
    /// - a slope of N dB is the cookbook shelf slope S = N / 12 ([`slope_to_q`]);
    /// - with neither a Q nor a slope (or either written as 0), S is
    ///   [`DEFAULT_SHELF_S`];
    /// - an `LS`/`HS` given a Q or a slope reads `Fc` as a *corner* frequency
    ///   and centres the biquad at Fc · 10^(|gain| / (80 S)) (`LS`) or
    ///   Fc / 10^(…) (`HS`), deriving S from the Q when that's what it has.
    ///   `LSC`/`HSC`, and an `LS`/`HS` with neither, centre on `Fc` itself.
    ///
    /// Any other type keeps its `Fc`, and its `Q` or else
    /// [`FilterKind::default_q`]. `None` for a slope with no real equivalent Q,
    /// which is no usable filter. Mirrors `apoBiquad` in `src/lib/eq.ts`, and
    /// the two are pinned to the same numbers in their tests.
    pub fn apo_biquad(&self) -> Option<(f64, f64)> {
        // APO takes a Q or slope of 0 as none given.
        let given = |v: Option<f64>| v.filter(|&v| v != 0.0);
        let q = given(self.q);
        if !self.kind.is_shelf() {
            return Some((self.freq, q.unwrap_or(self.kind.default_q())));
        }
        let gain = self.gain.unwrap_or(0.0);
        let slope = given(self.slope.filter(|_| self.kind.takes_slope()));
        let (s, q) = match (slope, q) {
            (Some(slope), _) => (slope / 12.0, slope_to_q(slope, gain)?),
            (None, Some(q)) => (shelf_q_to_s(q, gain), q),
            (None, None) => return Some((self.freq, shelf_s_to_q(DEFAULT_SHELF_S, gain)?)),
        };
        // APO's frequency adjustment "for DCX2496" (a Behringer crossover).
        let shift = 10f64.powf(gain.abs() / 80.0 / s);
        let freq = match self.kind {
            FilterKind::LowShelf => self.freq * shift,
            FilterKind::HighShelf => self.freq / shift,
            _ => self.freq,
        };
        Some((freq, q))
    }
}

/// The cookbook shelf slope S Equalizer APO gives a shelf with neither a `Q`
/// nor a slope: 0.9, "found out by experimentation with RoomEQWizard"
/// (`BiQuadFilterFactory.cpp`). Mirrors `DEFAULT_SHELF_S` in `src/lib/eq.ts`.
pub const DEFAULT_SHELF_S: f64 = 0.9;

/// The Q equivalent to a shelf slope of `slope` dB/octave at `gain` dB.
///
/// Equalizer APO reads `LSC 12 dB` as the RBJ cookbook's shelf slope
/// S = slope / 12 (so 12 dB/oct is S = 1, the steepest shelf with no overshoot);
/// see [`shelf_s_to_q`]. `None` when there's no real equivalent. Mirrors
/// `slopeToQ` in `src/lib/eq.ts`.
pub fn slope_to_q(slope: f64, gain: f64) -> Option<f64> {
    shelf_s_to_q(slope / 12.0, gain)
}

/// The Q equivalent to the cookbook shelf slope `s` at `gain` dB.
///
/// The cookbook sets alpha = sin(w0)/2 · sqrt((A + 1/A)(1/S − 1) + 2). In Q form
/// alpha is sin(w0)/(2Q), so Q = 1/sqrt(…) — which, unlike S, depends on the
/// gain. `None` when the root isn't real (a slope too steep for the gain) or `s`
/// isn't positive; APO has no usable filter there either. Mirrors `shelfSToQ`
/// in `src/lib/eq.ts`.
pub fn shelf_s_to_q(s: f64, gain: f64) -> Option<f64> {
    if s <= 0.0 {
        return None;
    }
    let a = 10f64.powf(gain / 40.0);
    let radicand = (a + 1.0 / a) * (1.0 / s - 1.0) + 2.0;
    (radicand > 0.0).then(|| 1.0 / radicand.sqrt())
}

/// The cookbook shelf slope equivalent to `q` at `gain` dB — the inverse of
/// [`shelf_s_to_q`], as APO's `BiQuadFilter` derives it for a corner-frequency
/// shelf given a Q. Mirrors `shelfQToS` in `src/lib/eq.ts`.
fn shelf_q_to_s(q: f64, gain: f64) -> f64 {
    let a = 10f64.powf(gain / 40.0);
    1.0 / ((1.0 / (q * q) - 2.0) / (a + 1.0 / a) + 1.0)
}

/// The supported filter types, mapped to their APO tokens.
///
/// Unknown tokens are not represented here — a filter line whose type we do not
/// recognise (or that carries parameters we cannot model) is kept as
/// [`Line::Raw`] rather than being lossily coerced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterKind {
    /// `PK` — peaking / parametric.
    Peak,
    /// `LS` — low shelf: fixed slope, or with a `Q` a corner-frequency shelf
    /// (see [`Filter::apo_biquad`]).
    LowShelf,
    /// `HS` — high shelf: fixed slope, or with a `Q` a corner-frequency shelf.
    HighShelf,
    /// `LSC` — low shelf with adjustable Q/corner.
    LowShelfQ,
    /// `HSC` — high shelf with adjustable Q/corner.
    HighShelfQ,
    /// `LP` — low pass (12 dB/oct).
    LowPass,
    /// `HP` — high pass (12 dB/oct).
    HighPass,
    /// `LPQ` — low pass with adjustable Q.
    LowPassQ,
    /// `HPQ` — high pass with adjustable Q.
    HighPassQ,
    /// `BP` — band pass.
    BandPass,
    /// `NO` — notch.
    Notch,
    /// `AP` — all pass.
    AllPass,
}

impl FilterKind {
    /// Parse an APO type token (case-insensitive), or `None` if unsupported.
    pub fn from_token(token: &str) -> Option<Self> {
        use FilterKind::*;
        Some(match token.to_ascii_uppercase().as_str() {
            "PK" => Peak,
            "LS" => LowShelf,
            "HS" => HighShelf,
            "LSC" => LowShelfQ,
            "HSC" => HighShelfQ,
            "LP" => LowPass,
            "HP" => HighPass,
            "LPQ" => LowPassQ,
            "HPQ" => HighPassQ,
            "BP" => BandPass,
            "NO" => Notch,
            "AP" => AllPass,
            _ => return None,
        })
    }

    /// The canonical APO token this filter type serializes to.
    pub fn as_token(self) -> &'static str {
        use FilterKind::*;
        match self {
            Peak => "PK",
            LowShelf => "LS",
            HighShelf => "HS",
            LowShelfQ => "LSC",
            HighShelfQ => "HSC",
            LowPass => "LP",
            HighPass => "HP",
            LowPassQ => "LPQ",
            HighPassQ => "HPQ",
            BandPass => "BP",
            Notch => "NO",
            AllPass => "AP",
        }
    }

    /// Whether this type carries a `Gain` parameter (for editor UI hints).
    pub fn has_gain(self) -> bool {
        use FilterKind::*;
        matches!(self, Peak | LowShelf | HighShelf | LowShelfQ | HighShelfQ)
    }

    /// Whether this type carries a `Q` parameter (for editor UI hints).
    pub fn has_q(self) -> bool {
        use FilterKind::*;
        matches!(
            self,
            Peak | LowShelfQ | HighShelfQ | LowPassQ | HighPassQ | BandPass | Notch | AllPass
        )
    }

    /// Whether this type can take a slope in dB/octave in place of its `Q`
    /// (`LSC 12 dB` / `HSC 6 dB`). APO also accepts `LS 6dB`, a slope shelf
    /// with a corner frequency, but that isn't modeled: it stays a raw line.
    pub fn takes_slope(self) -> bool {
        matches!(self, FilterKind::LowShelfQ | FilterKind::HighShelfQ)
    }

    /// Whether this is one of the four shelf types.
    pub fn is_shelf(self) -> bool {
        use FilterKind::*;
        matches!(self, LowShelf | HighShelf | LowShelfQ | HighShelfQ)
    }

    /// The Q fastpeq gives a non-shelf band whose line has none (a shelf's
    /// comes from [`DEFAULT_SHELF_S`] instead): 1 for a peak, which APO rejects
    /// without a Q, and otherwise 1/√2 — APO's own default, except that APO
    /// gives a notch Q 30. Mirrors `defaultQ` in `src/lib/eq.ts`.
    pub fn default_q(self) -> f64 {
        match self {
            FilterKind::Peak => 1.0,
            _ => std::f64::consts::FRAC_1_SQRT_2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The JSON shape the Svelte frontend depends on over IPC.
    #[test]
    fn channel_json_contract() {
        let config = Config {
            lines: vec![
                Line::Preamp {
                    gain: -3.0,
                    channel: Channel::Both,
                },
                Line::Filter(Filter {
                    enabled: true,
                    kind: FilterKind::Peak,
                    freq: 1000.0,
                    gain: Some(-2.0),
                    q: Some(1.0),
                    slope: None,
                    index: Some(1),
                    channel: Channel::Left,
                }),
                Line::Filter(Filter {
                    enabled: true,
                    kind: FilterKind::HighShelf,
                    freq: 8000.0,
                    gain: Some(2.0),
                    q: None,
                    slope: None,
                    index: Some(2),
                    channel: Channel::Other("C SUB".into()),
                }),
            ],
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(
            json.contains(r#"{"kind":"Preamp","value":{"gain":-3.0,"channel":{"kind":"both"}}}"#),
            "{json}"
        );
        assert!(json.contains(r#""channel":{"kind":"left"}"#), "{json}");
        assert!(
            json.contains(r#""channel":{"kind":"other","spec":"C SUB"}"#),
            "{json}"
        );

        // And it survives a JSON round-trip unchanged.
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back, config);
    }

    /// JSON from before the slope existed (no `slope` key) still deserializes.
    #[test]
    fn filter_json_without_slope_loads() {
        let json = r#"{"enabled":true,"kind":"Peak","freq":100.0,"gain":1.0,"q":1.0,"index":null,"channel":{"kind":"both"}}"#;
        let f: Filter = serde_json::from_str(json).unwrap();
        assert_eq!(f.slope, None);
    }

    #[test]
    fn slope_to_q_matches_the_cookbook() {
        let close = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-12);
        // S = 1 (12 dB/oct) is Q = 1/√2 at every gain.
        for gain in [-12.0, 0.0, 3.0, 12.0] {
            assert!(close(
                slope_to_q(12.0, gain),
                std::f64::consts::FRAC_1_SQRT_2
            ));
        }
        // At 0 dB the gain terms vanish: Q = sqrt(S / 2).
        assert!(close(slope_to_q(6.0, 0.0), 0.5));
        // Pinned in src/lib/eq.test.ts as well, so the two stay in step.
        assert!(slope_to_q(10.0, 6.0).is_some_and(|q| (q - 0.642281).abs() < 5e-7));
        // Shallower slopes give lower Qs, steeper ones higher (the overshoot).
        assert!(slope_to_q(6.0, 6.0).unwrap() < slope_to_q(15.0, 6.0).unwrap());
        // No real equivalent: a non-positive slope, or one too steep for the gain.
        assert_eq!(slope_to_q(0.0, 6.0), None);
        assert_eq!(slope_to_q(60.0, 24.0), None);
    }

    #[test]
    fn shelf_q_to_s_inverts_shelf_s_to_q() {
        for gain in [-12.0, -3.0, 0.0, 6.0, 15.0] {
            for s in [0.3, 0.9, 1.0, 1.5] {
                let q = shelf_s_to_q(s, gain).unwrap();
                assert!(
                    (shelf_q_to_s(q, gain) - s).abs() < 1e-12,
                    "S {s} at {gain} dB"
                );
            }
        }
    }

    /// A `kind` band with the given parameters.
    fn band(kind: FilterKind, freq: f64, gain: f64, q: Option<f64>, slope: Option<f64>) -> Filter {
        Filter {
            kind,
            q,
            slope,
            ..Filter::peak(freq, gain, 1.0)
        }
    }

    #[test]
    fn apo_biquad_matches_equalizer_apo() {
        use FilterKind::*;
        use std::f64::consts::FRAC_1_SQRT_2 as SQRT1_2;
        let close = |got: Option<(f64, f64)>, (freq, q): (f64, f64)| {
            got.is_some_and(|(f, g)| (f - freq).abs() < 5e-4 && (g - q).abs() < 5e-7)
        };
        // Pinned in src/lib/eq.test.ts as well, so the two stay in step.
        // No Q or slope: S = 0.9 at Fc, whether or not the type ends in C.
        for kind in [LowShelf, HighShelf, LowShelfQ, HighShelfQ] {
            assert!(close(
                band(kind, 100.0, 6.0, None, None).apo_biquad(),
                (100.0, 0.668809)
            ));
        }
        assert!(close(
            band(LowShelf, 100.0, 0.0, None, None).apo_biquad(),
            (100.0, 0.670820) // sqrt(0.9 / 2)
        ));
        // An LS/HS given a Q reads Fc as a corner, so the biquad moves inward.
        assert!(close(
            band(LowShelf, 100.0, 6.0, Some(0.7), None).apo_biquad(),
            (119.24595, 0.7)
        ));
        assert!(close(
            band(HighShelf, 8000.0, -4.0, Some(1.0), None).apo_biquad(),
            (7541.2195, 1.0)
        ));
        // At Q 1/√2 (S = 1) that's the textbook corner-to-centre factor √A.
        let a = 10f64.powf(6.0 / 40.0);
        assert!(close(
            band(LowShelf, 100.0, 6.0, Some(SQRT1_2), None).apo_biquad(),
            (100.0 * a.sqrt(), SQRT1_2)
        ));
        // LSC/HSC keep Fc as the centre, with a Q or a slope.
        assert!(close(
            band(LowShelfQ, 100.0, 6.0, Some(0.7), None).apo_biquad(),
            (100.0, 0.7)
        ));
        assert!(close(
            band(HighShelfQ, 100.0, 6.0, None, Some(10.0)).apo_biquad(),
            (100.0, 0.642281)
        ));
        // A 0 reads as none given; a slope with no real Q is no filter.
        assert!(close(
            band(LowShelf, 100.0, 6.0, Some(0.0), None).apo_biquad(),
            (100.0, 0.668809)
        ));
        assert_eq!(
            band(LowShelfQ, 100.0, 24.0, None, Some(60.0)).apo_biquad(),
            None
        );
        // Other types keep their Fc and Q, or take the type's default.
        assert_eq!(
            Filter::peak(100.0, 6.0, 2.0).apo_biquad(),
            Some((100.0, 2.0))
        );
        assert_eq!(
            band(Peak, 100.0, 6.0, None, None).apo_biquad(),
            Some((100.0, 1.0))
        );
    }
}
