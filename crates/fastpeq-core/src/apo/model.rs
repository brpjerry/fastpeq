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

    /// The Q to use in biquad math: the band's own `q`, or for a shelf given as
    /// a slope, the Q that draws the same curve at this gain. `None` when the
    /// band has neither, or its slope has no real equivalent (see
    /// [`slope_to_q`]).
    pub fn effective_q(&self) -> Option<f64> {
        match self.slope {
            Some(slope) => slope_to_q(slope, self.gain.unwrap_or(0.0)),
            None => self.q,
        }
    }
}

/// The Q equivalent to a shelf slope of `slope` dB/octave at `gain` dB.
///
/// Equalizer APO reads `LSC 12 dB` as the RBJ cookbook's shelf slope
/// S = slope / 12 (so 12 dB/oct is S = 1, the steepest shelf with no overshoot)
/// and sets alpha = sin(w0)/2 · sqrt((A + 1/A)(1/S − 1) + 2). In Q form alpha is
/// sin(w0)/(2Q), so Q = 1/sqrt(…) — which, unlike S, depends on the gain.
/// `None` when the root isn't real (a slope too steep for the gain) or the slope
/// isn't positive; APO has no usable filter there either. Mirrors `slopeToQ` in
/// `src/lib/eq.ts`.
pub fn slope_to_q(slope: f64, gain: f64) -> Option<f64> {
    let s = slope / 12.0;
    if s <= 0.0 {
        return None;
    }
    let a = 10f64.powf(gain / 40.0);
    let radicand = (a + 1.0 / a) * (1.0 / s - 1.0) + 2.0;
    (radicand > 0.0).then(|| 1.0 / radicand.sqrt())
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
    /// `LS` — low shelf (fixed slope).
    LowShelf,
    /// `HS` — high shelf (fixed slope).
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
    /// (`LSC 12 dB` / `HSC 6 dB`). APO also accepts `LS 6dB`, but that is a
    /// corner-frequency shelf we don't model, so it stays a raw line.
    pub fn takes_slope(self) -> bool {
        matches!(self, FilterKind::LowShelfQ | FilterKind::HighShelfQ)
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
    fn effective_q_prefers_the_slope() {
        let shelf = Filter {
            kind: FilterKind::LowShelfQ,
            q: None,
            slope: Some(12.0),
            ..Filter::peak(100.0, 6.0, 1.0)
        };
        assert_eq!(shelf.effective_q(), slope_to_q(12.0, 6.0));
        assert_eq!(Filter::peak(100.0, 6.0, 2.0).effective_q(), Some(2.0));
    }
}
