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

    /// Whether `self` would drive Equalizer APO to the same processing as
    /// `other` once the *master* `Preamp:` (the `Both` channel) is set aside.
    ///
    /// Specifically: the modeled `Filter:` lines must match in document order,
    /// the one-sided preamp *trims* (balance) must match, and the unmodeled
    /// [`Line::Raw`] lines must match as a set (order-independent). The master
    /// preamp is ignored entirely — both its value and its presence.
    ///
    /// This exists so a preset still reads as "active" after Auto Preamp has
    /// recomputed the master gain on the live config (which may *add* a preamp
    /// to a preset that had none), and after the editor has rewritten the line
    /// order. It deliberately ignores the master preamp, so two configs that
    /// differ *only* by that gain compare equal here; that ambiguity is resolved
    /// upstream by the [provenance] stamp, which names *which* preset produced the
    /// live config — this only confirms the named preset still matches.
    ///
    /// [provenance]: crate::provenance
    pub fn is_equivalent(&self, other: &Config) -> bool {
        // Modeled filters, in document order. (Order is audibly neutral for APO,
        // but the editor preserves it, so we keep the comparison strict.)
        if self.filters().ne(other.filters()) {
            return false;
        }

        // One-sided preamps are balance trims, not the master gain — keep them.
        let is_trim = |l: &&Line| {
            matches!(
                l,
                Line::Preamp {
                    channel: Channel::Left | Channel::Right | Channel::Other(_),
                    ..
                }
            )
        };
        let trims_self: Vec<_> = self.lines.iter().filter(is_trim).collect();
        let trims_other: Vec<_> = other.lines.iter().filter(is_trim).collect();
        if trims_self != trims_other {
            return false;
        }

        // Unmodeled lines (comments, Device:, Include:, …) as an order-free set.
        let mut raw_self: Vec<_> = self.raw_lines().collect();
        let mut raw_other: Vec<_> = other.raw_lines().collect();
        raw_self.sort();
        raw_other.sort();
        raw_self == raw_other
    }

    /// Iterate over the unmodeled [`Line::Raw`] payloads.
    fn raw_lines(&self) -> impl Iterator<Item = &String> {
        self.lines.iter().filter_map(|l| match l {
            Line::Raw(s) => Some(s),
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
    /// `Filter[ N]: ON|OFF <type> Fc <hz> Hz [Gain <db> dB] [Q <q>]`
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
            index: None,
            channel: Channel::Both,
        }
    }
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
                    index: Some(1),
                    channel: Channel::Left,
                }),
                Line::Filter(Filter {
                    enabled: true,
                    kind: FilterKind::HighShelf,
                    freq: 8000.0,
                    gain: Some(2.0),
                    q: None,
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

    fn both_preamp(gain: f64) -> Line {
        Line::Preamp {
            gain,
            channel: Channel::Both,
        }
    }

    #[test]
    fn is_equivalent_ignores_master_preamp() {
        // Same filters, different master gain (the Auto Preamp case).
        let a = Config {
            lines: vec![
                both_preamp(-6.0),
                Line::Filter(Filter::peak(1000.0, 3.0, 1.0)),
            ],
        };
        let b = Config {
            lines: vec![
                both_preamp(-2.0),
                Line::Filter(Filter::peak(1000.0, 3.0, 1.0)),
            ],
        };
        assert!(a.is_equivalent(&b));
        assert_ne!(a, b); // ...but they are NOT exactly equal.

        // Auto Preamp may *add* a master preamp the source preset lacked.
        let no_preamp = Config {
            lines: vec![Line::Filter(Filter::peak(1000.0, 3.0, 1.0))],
        };
        assert!(no_preamp.is_equivalent(&a));
    }

    #[test]
    fn is_equivalent_ignores_raw_line_order_but_not_content() {
        let a = Config {
            lines: vec![
                Line::Raw("Device: Speakers".into()),
                Line::Raw("# comment".into()),
                Line::Filter(Filter::peak(1000.0, 3.0, 1.0)),
            ],
        };
        let reordered = Config {
            lines: vec![
                Line::Raw("# comment".into()),
                Line::Filter(Filter::peak(1000.0, 3.0, 1.0)),
                Line::Raw("Device: Speakers".into()),
            ],
        };
        assert!(a.is_equivalent(&reordered));

        let different_raw = Config {
            lines: vec![
                Line::Raw("Device: Headphones".into()),
                Line::Filter(Filter::peak(1000.0, 3.0, 1.0)),
            ],
        };
        assert!(!a.is_equivalent(&different_raw));
    }

    #[test]
    fn is_equivalent_distinguishes_filters_and_balance_trims() {
        let base = Config {
            lines: vec![Line::Filter(Filter::peak(1000.0, 3.0, 1.0))],
        };

        // A differing filter is not equivalent.
        let other_filter = Config {
            lines: vec![Line::Filter(Filter::peak(2000.0, 3.0, 1.0))],
        };
        assert!(!base.is_equivalent(&other_filter));

        // A one-sided preamp (balance trim) is significant, unlike the master.
        let with_trim = Config {
            lines: vec![
                Line::Preamp {
                    gain: -2.0,
                    channel: Channel::Left,
                },
                Line::Filter(Filter::peak(1000.0, 3.0, 1.0)),
            ],
        };
        assert!(!base.is_equivalent(&with_trim));
    }
}
