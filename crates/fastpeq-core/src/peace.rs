//! Convert PEACE (`.peace`) preset files into our APO config model.
//!
//! PEACE stores presets as INI files: a fixed frequency/quality grid plus a
//! **sparse** `[Gains]` section — only active bands have a `Gain{i}` entry, so
//! presence of a gain is what makes a band "on". All bands are peaking filters,
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
    {
        if preamp != 0.0 {
            lines.push(Line::Preamp {
                gain: preamp,
                channel: Channel::Both,
            });
        }
    }

    // Grid "" is speaker 0 (all); "1".."8" are the per-channel grids.
    for n in 0..=8u32 {
        let suffix = if n == 0 { String::new() } else { n.to_string() };
        let Some(gains) = ini.get(&format!("Gains{suffix}")) else {
            continue;
        };
        let freqs = ini.get(&format!("Frequencies{suffix}"));
        let quals = ini.get(&format!("Qualities{suffix}"));
        let channel = speaker_channel(&ini, n);

        // Active bands, by grid index.
        let mut bands: Vec<(u32, f64)> = gains
            .iter()
            .filter_map(|(k, v)| Some((k.strip_prefix("Gain")?.parse().ok()?, v.parse().ok()?)))
            .collect();
        bands.sort_by_key(|(i, _)| *i);

        for (band, gain) in bands {
            let Some(freq) = freqs
                .and_then(|f| f.get(&format!("Frequency{band}")))
                .and_then(|v| v.parse::<f64>().ok())
            else {
                continue;
            };
            let q = quals
                .and_then(|f| f.get(&format!("Quality{band}")))
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1.0);
            index += 1;
            lines.push(Line::Filter(Filter {
                enabled: true,
                kind: FilterKind::Peak,
                freq,
                gain: Some(gain),
                q: Some(q),
                index: Some(index),
                channel: channel.clone(),
            }));
        }
    }

    Config { lines }
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
}
