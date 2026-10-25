//! Convert PEACE (`.peace`) preset files into our APO config model.
//!
//! PEACE stores presets as INI files: a fixed frequency/quality grid plus a
//! **sparse** `[Gains]` section — only active bands have a `Gain{i}` entry, so
//! presence of a gain is what makes a band "on". Two more sections are sparse
//! the same way: `[Filters]` holds `Filter{i}=<code>` only for bands that are
//! not peaking filters (see [`band_type`]), and `[Disabled]` holds
//! `Disabled{i}=1` for bands switched off while keeping their settings.
//! `[General] PreAmp` is the preamp (dB), and `[Speakers]` maps each grid to a
//! channel (grid "" = all, "1" = L, "2" = R, …). Values are in dB / Hz / Q
//! directly, matching the APO config PEACE itself generates.

use crate::apo::model::{Channel, Config, Filter, FilterKind, Line};
use std::collections::BTreeMap;

type Ini = BTreeMap<String, BTreeMap<String, String>>;

/// Build an APO [`Config`] from the text of a `.peace` file.
pub fn from_peace(text: &str) -> Config {
    let ini = parse_ini(text);
    let mut lines = Vec::new();
    let mut index = 0u32;

    if let Some(preamp) = ini
        .get("General")
        .and_then(|g| g.get("PreAmp"))
        .and_then(|v| v.parse::<f64>().ok())
        && preamp != 0.0
    {
        lines.push(Line::Preamp {
            gain: preamp,
            channel: Channel::Both,
        });
    }

    // Grid "" is speaker 0 (all); "1".."8" are the per-channel grids.
    for n in 0..=8u32 {
        let suffix = if n == 0 { String::new() } else { n.to_string() };
        let Some(gains) = ini.get(&format!("Gains{suffix}")) else {
            continue;
        };
        let section = |name: &str| ini.get(&format!("{name}{suffix}"));
        let (freqs, quals) = (section("Frequencies"), section("Qualities"));
        let (types, disabled) = (section("Filters"), section("Disabled"));
        let channel = speaker_channel(&ini, n);

        // Active bands, by grid index.
        let mut bands: Vec<(u32, f64)> = gains
            .iter()
            .filter_map(|(k, v)| Some((k.strip_prefix("Gain")?.parse().ok()?, v.parse().ok()?)))
            .collect();
        bands.sort_by_key(|(i, _)| *i);

        for (band, gain) in bands {
            let Some(freq) =
                entry(freqs, &format!("Frequency{band}")).and_then(|v| v.parse::<f64>().ok())
            else {
                continue;
            };
            let quality =
                entry(quals, &format!("Quality{band}")).and_then(|v| v.parse::<f64>().ok());
            let code = entry(types, &format!("Filter{band}")).and_then(|v| v.parse::<u32>().ok());
            let off = entry(disabled, &format!("Disabled{band}")).is_some_and(|v| v != "0");
            let BandType {
                kind,
                gain: has_gain,
                q: has_q,
                slope: has_slope,
            } = band_type(code);
            index += 1;
            lines.push(Line::Filter(Filter {
                enabled: !off,
                kind,
                freq,
                gain: has_gain.then_some(gain),
                q: has_q.then(|| quality.unwrap_or(1.0)),
                // PEACE keeps a slope shelf's dB/octave in its Quality column.
                slope: quality.filter(|_| has_slope),
                index: Some(index),
                channel: channel.clone(),
            }));
        }
    }

    Config { lines }
}

/// How a band of one PEACE filter type maps onto our model: the kind, and which
/// of PEACE's stored values (it keeps a gain and a quality for every band) the
/// type actually uses. `slope` means the quality column is a shelf slope in
/// dB/octave rather than a Q.
struct BandType {
    kind: FilterKind,
    gain: bool,
    q: bool,
    slope: bool,
}

/// The type of a band from its `Filter{i}` code — an index into PEACE's own
/// filter list (the "Peak filter" … labels in its `.plf` language files, keys
/// 3200 + code). No entry means a peaking filter, and so does a code we don't
/// know: that keeps the band rather than losing it. 16/17 (shelves with a
/// corner frequency) become an `LS`/`HS` that keeps its Q, which Equalizer APO
/// reads as a corner-frequency shelf ([`Filter::apo_biquad`]).
///
/// Approximations, where our model has no exact counterpart:
/// - 10–13 (Butterworth / Linkwitz-Riley passes) become one adjustable-Q
///   biquad, where PEACE renders a cascade.
fn band_type(code: Option<u32>) -> BandType {
    use FilterKind::*;
    let band = |kind, gain, q| BandType {
        kind,
        gain,
        q,
        slope: false,
    };
    let slope_shelf = |kind| BandType {
        kind,
        gain: true,
        q: false,
        slope: true,
    };
    match code {
        Some(1) => band(LowPass, false, false),
        Some(2) => band(HighPass, false, false),
        Some(3) => band(BandPass, false, true),
        Some(4) => band(LowShelf, true, false),
        Some(5) => band(HighShelf, true, false),
        Some(6) => band(Notch, false, true),
        Some(7) => band(AllPass, false, true),
        Some(8) => slope_shelf(LowShelfQ),
        Some(9) => slope_shelf(HighShelfQ),
        Some(10 | 12) => band(LowPassQ, false, true),
        Some(11 | 13) => band(HighPassQ, false, true),
        Some(14) => band(LowShelfQ, true, true),
        Some(15) => band(HighShelfQ, true, true),
        Some(16) => band(LowShelf, true, true),
        Some(17) => band(HighShelf, true, true),
        _ => band(Peak, true, true),
    }
}

/// One key's value from a section that may be absent.
fn entry<'a>(section: Option<&'a BTreeMap<String, String>>, key: &str) -> Option<&'a str> {
    section?.get(key).map(String::as_str)
}

fn speaker_channel(ini: &Ini, n: u32) -> Channel {
    let target = ini
        .get("Speakers")
        .and_then(|s| s.get(&format!("SpeakerTargets{n}")))
        .map(|s| s.trim());
    match target {
        Some("L") => Channel::Left,
        Some("R") => Channel::Right,
        Some(other) if !other.is_empty() && !other.eq_ignore_ascii_case("all") => {
            Channel::Other(other.to_string())
        }
        _ => Channel::Both,
    }
}

fn parse_ini(text: &str) -> Ini {
    let mut ini: Ini = BTreeMap::new();
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.to_string();
            ini.entry(section.clone()).or_default();
        } else if let Some(eq) = line.find('=') {
            let key = line[..eq].trim().to_string();
            let val = line[eq + 1..].trim().to_string();
            ini.entry(section.clone()).or_default().insert(key, val);
        }
    }
    ini
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apo::serialize;

    #[test]
    fn converts_basic_preset() {
        let text = "[General]\nPreAmp=-5\n\
                    [Frequencies]\nFrequency1=20\nFrequency2=150\nFrequency3=800\n\
                    [Gains]\nGain1=4\nGain3=-3\n\
                    [Qualities]\nQuality1=1\nQuality2=1\nQuality3=2\n\
                    [Speakers]\nSpeakerId0=0\nSpeakerTargets0=all\n";
        let cfg = from_peace(text);
        assert_eq!(cfg.preamp(), Some(-5.0));
        let f: Vec<_> = cfg.filters().collect();
        assert_eq!(f.len(), 2); // bands 1 and 3 active; band 2 has no gain
        assert_eq!(f[0].freq, 20.0);
        assert_eq!(f[0].gain, Some(4.0));
        assert_eq!(f[0].q, Some(1.0));
        assert_eq!(f[0].kind, FilterKind::Peak);
        assert_eq!(f[0].channel, Channel::Both);
        assert_eq!(f[1].freq, 800.0);
        assert_eq!(f[1].gain, Some(-3.0));
        assert_eq!(f[1].q, Some(2.0));
    }

    #[test]
    fn per_channel_grid_maps_to_left_right() {
        let text = "[Gains1]\nGain1=2\n[Frequencies1]\nFrequency1=1000\n[Qualities1]\nQuality1=1\n\
                    [Gains2]\nGain1=2\n[Frequencies2]\nFrequency1=1000\n[Qualities2]\nQuality1=1\n\
                    [Speakers]\nSpeakerTargets1=L\nSpeakerTargets2=R\n";
        let cfg = from_peace(text);
        let f: Vec<_> = cfg.filters().collect();
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].channel, Channel::Left);
        assert_eq!(f[1].channel, Channel::Right);
    }

    #[test]
    fn empty_template_has_no_eq() {
        let text = "[Frequencies]\nFrequency1=100\n[Qualities]\nQuality1=1\n";
        let cfg = from_peace(text);
        assert!(cfg.preamp().is_none());
        assert_eq!(cfg.filters().count(), 0);
    }

    /// A one-band preset whose band 1 has type `code` (`None`: no entry).
    fn one_band(code: Option<u32>) -> String {
        let filters = code.map_or(String::new(), |c| format!("[Filters]\nFilter1={c}\n"));
        format!(
            "[Frequencies]\nFrequency1=100\n[Gains]\nGain1=6\n[Qualities]\nQuality1=12\n{filters}"
        )
    }

    fn apo_line(code: Option<u32>) -> String {
        serialize(&from_peace(&one_band(code)))
    }

    #[test]
    fn filter_codes_map_to_apo_types() {
        // Only the values a type uses survive: no Gain on a pass, notch or
        // all-pass, no Q on a plain shelf, and the quality as a slope on 8/9.
        let cases = [
            (None, "PK Fc 100 Hz Gain 6 dB Q 12"),
            (Some(0), "PK Fc 100 Hz Gain 6 dB Q 12"),
            (Some(1), "LP Fc 100 Hz"),
            (Some(2), "HP Fc 100 Hz"),
            (Some(3), "BP Fc 100 Hz Q 12"),
            (Some(4), "LS Fc 100 Hz Gain 6 dB"),
            (Some(5), "HS Fc 100 Hz Gain 6 dB"),
            (Some(6), "NO Fc 100 Hz Q 12"),
            (Some(7), "AP Fc 100 Hz Q 12"),
            (Some(8), "LSC 12 dB Fc 100 Hz Gain 6 dB"),
            (Some(9), "HSC 12 dB Fc 100 Hz Gain 6 dB"),
            (Some(10), "LPQ Fc 100 Hz Q 12"),
            (Some(11), "HPQ Fc 100 Hz Q 12"),
            (Some(12), "LPQ Fc 100 Hz Q 12"),
            (Some(13), "HPQ Fc 100 Hz Q 12"),
            (Some(14), "LSC Fc 100 Hz Gain 6 dB Q 12"),
            (Some(15), "HSC Fc 100 Hz Gain 6 dB Q 12"),
            (Some(16), "LS Fc 100 Hz Gain 6 dB Q 12"),
            (Some(17), "HS Fc 100 Hz Gain 6 dB Q 12"),
            (Some(99), "PK Fc 100 Hz Gain 6 dB Q 12"), // unknown: keep the band
        ];
        for (code, want) in cases {
            assert_eq!(
                apo_line(code),
                format!("Filter 1: ON {want}"),
                "code {code:?}"
            );
        }
    }

    /// The shapes found in real PEACE presets: shelves alongside the default
    /// peaks, a disabled band, and a type on an inactive band (no gain).
    #[test]
    fn real_world_shelves_and_disabled_bands() {
        let text = "[Frequencies]\nFrequency1=100\nFrequency2=950\nFrequency3=2750\n\
                    Frequency4=8000\nFrequency5=10000\n\
                    [Gains]\nGain1=5.5\nGain2=-3.6\nGain3=6\nGain4=-2\n\
                    [Qualities]\nQuality1=0.71\nQuality2=2.8\nQuality3=0.71\nQuality4=6\n\
                    Quality5=15\n\
                    [Filters]\nFilter1=14\nFilter3=15\nFilter5=8\n\
                    [Disabled]\nDisabled4=1\n";
        assert_eq!(
            serialize(&from_peace(text)),
            "Filter 1: ON LSC Fc 100 Hz Gain 5.5 dB Q 0.71\n\
             Filter 2: ON PK Fc 950 Hz Gain -3.6 dB Q 2.8\n\
             Filter 3: ON HSC Fc 2750 Hz Gain 6 dB Q 0.71\n\
             Filter 4: OFF PK Fc 8000 Hz Gain -2 dB Q 6"
        );
    }

    #[test]
    fn per_channel_grids_read_their_own_types_and_disabled_bands() {
        let text = "[Frequencies]\nFrequency1=100\n[Gains]\nGain1=1\n[Qualities]\nQuality1=1\n\
                    [Frequencies1]\nFrequency1=100\n[Gains1]\nGain1=2\n[Qualities1]\nQuality1=1\n\
                    [Filters1]\nFilter1=15\n[Disabled1]\nDisabled1=1\n\
                    [Speakers]\nSpeakerTargets1=L\n";
        let cfg = from_peace(text);
        let f: Vec<_> = cfg.filters().collect();
        assert_eq!((f[0].kind, f[0].enabled), (FilterKind::Peak, true));
        assert_eq!((f[1].kind, f[1].enabled), (FilterKind::HighShelfQ, false));
        assert_eq!(f[1].channel, Channel::Left);
    }

    #[test]
    fn slope_shelf_without_a_quality_falls_back_to_apo_default() {
        let text = "[Frequencies]\nFrequency1=100\n[Gains]\nGain1=6\n[Filters]\nFilter1=8\n";
        assert_eq!(
            serialize(&from_peace(text)),
            "Filter 1: ON LSC Fc 100 Hz Gain 6 dB"
        );
    }
}
