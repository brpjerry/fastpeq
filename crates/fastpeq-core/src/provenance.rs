//! Provenance: a stamp in the live `config.txt` recording which preset produced
//! it, so the active preset is *known* rather than re-inferred by content.
//!
//! When a preset is applied we write a marker comment:
//!
//! ```text
//! # fastpeq:preset=<name>
//! ```
//!
//! Equalizer APO ignores comment lines and the parser keeps them as
//! [`Line::Raw`], so the stamp round-trips a parse/serialize cycle and survives a
//! restart. [`Manager::active_preset`](crate::Manager::active_preset) reads it
//! back in O(1) — and, crucially, it *disambiguates* presets that are otherwise
//! indistinguishable once Auto Preamp has rewritten the master gain (the inherent
//! limitation of the content-only [`Config::is_equivalent`](crate::Config) match).
//!
//! The stamp is **advisory**: `active_preset` trusts it only while the live base
//! EQ is still equivalent to the named preset, so a stale stamp — left by a
//! divergent edit, or pointing at a since-deleted preset — never yields a wrong
//! match; detection falls back to the content scan. Preset files never contain a
//! stamp: [`PresetStore::save`](crate::PresetStore) strips it on the way to disk.

use crate::apo::model::{Config, Line};

/// The marker comment prefix. Everything after it (to end of line) is the name.
const PREFIX: &str = "# fastpeq:preset=";

/// The preset name recorded in `config`'s provenance stamp, if present.
pub fn name(config: &Config) -> Option<String> {
    config.lines.iter().find_map(stamped_name)
}

/// `config` with any provenance stamp removed (leaving the EQ untouched).
pub fn strip(config: &Config) -> Config {
    if name(config).is_none() {
        return config.clone();
    }
    Config {
        lines: config
            .lines
            .iter()
            .filter(|l| !is_stamp(l))
            .cloned()
            .collect(),
    }
}

/// `config` stamped as having come from `preset`, replacing any existing stamp.
/// The stamp leads the file so it stays visible and survives a partial edit of
/// the lines below it.
pub fn set(config: &Config, preset: &str) -> Config {
    let mut lines = Vec::with_capacity(config.lines.len() + 1);
    lines.push(Line::Raw(format!("{PREFIX}{preset}")));
    lines.extend(config.lines.iter().filter(|l| !is_stamp(l)).cloned());
    Config { lines }
}

fn stamped_name(line: &Line) -> Option<String> {
    match line {
        Line::Raw(s) => s.trim().strip_prefix(PREFIX).map(str::to_string),
        _ => None,
    }
}

fn is_stamp(line: &Line) -> bool {
    matches!(line, Line::Raw(s) if s.trim().starts_with(PREFIX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apo::model::Filter;

    fn eq() -> Config {
        Config {
            lines: vec![Line::Filter(Filter::peak(1000.0, 3.0, 1.0))],
        }
    }

    #[test]
    fn set_then_name_round_trips() {
        let stamped = set(&eq(), "Bass Boost");
        assert_eq!(name(&stamped).as_deref(), Some("Bass Boost"));
        // The stamp leads, the EQ follows untouched.
        assert!(matches!(&stamped.lines[0], Line::Raw(s) if s == "# fastpeq:preset=Bass Boost"));
        assert_eq!(strip(&stamped), eq());
    }

    #[test]
    fn set_replaces_an_existing_stamp() {
        let once = set(&eq(), "First");
        let twice = set(&once, "Second");
        assert_eq!(name(&twice).as_deref(), Some("Second"));
        // Only one stamp ever remains.
        let stamps = twice.lines.iter().filter(|l| is_stamp(l)).count();
        assert_eq!(stamps, 1);
    }

    #[test]
    fn name_survives_a_parse_round_trip() {
        let text = crate::serialize(&set(&eq(), "HD 600"));
        assert_eq!(name(&crate::parse(&text)).as_deref(), Some("HD 600"));
    }

    #[test]
    fn unstamped_config_has_no_name_and_strip_is_a_noop() {
        assert_eq!(name(&eq()), None);
        assert_eq!(strip(&eq()), eq());
    }
}
