//! Integration tests for the parse/serialize round-trip — the core correctness
//! guarantee of fastpeq. These drive the crate through its public API only.

use fastpeq_core::{Channel, Config, Filter, FilterKind, Line, parse, serialize};

fn filter(kind: FilterKind, freq: f64, gain: Option<f64>, q: Option<f64>, index: u32) -> Filter {
    Filter {
        enabled: true,
        kind,
        freq,
        gain,
        q,
        index: Some(index),
        channel: Channel::Both,
    }
}

/// The central invariant: a model survives `serialize` → `parse` unchanged.
#[test]
fn model_round_trips() {
    let config = Config {
        lines: vec![
            Line::Raw("# My headphone preset".to_string()),
            Line::Raw(String::new()),
            Line::Preamp {
                gain: -6.5,
                channel: Channel::Both,
            },
            Line::Filter(filter(FilterKind::Peak, 1000.0, Some(-3.0), Some(1.41), 1)),
            Line::Filter(filter(
                FilterKind::LowShelfQ,
                105.0,
                Some(5.5),
                Some(0.7),
                2,
            )),
            Line::Filter(Filter {
                enabled: false,
                kind: FilterKind::HighPass,
                freq: 30.0,
                gain: None,
                q: None,
                index: None,
                channel: Channel::Both,
            }),
            Line::Raw("Include: base.txt".to_string()),
        ],
    };

    assert_eq!(parse(&serialize(&config)), config);
}

/// A realistic AutoEQ / oratory1990 export (the user's 15k-preset use case).
#[test]
fn parses_autoeq_export() {
    let input = "\
Preamp: -6.0 dB
Filter 1: ON LSC Fc 105 Hz Gain 5.5 dB Q 0.70
Filter 2: ON PK Fc 28 Hz Gain 2.0 dB Q 1.0
Filter 3: ON PK Fc 3000 Hz Gain -4.5 dB Q 2.5
Filter 4: ON HSC Fc 10000 Hz Gain 1.0 dB Q 0.70";

    let config = parse(input);
    assert_eq!(config.lines.len(), 5);
    assert_eq!(config.preamp(), Some(-6.0));
    assert_eq!(config.filters().count(), 4);

    let Line::Filter(f1) = &config.lines[1] else {
        panic!("line 1 should be a filter, got {:?}", config.lines[1]);
    };
    assert_eq!(f1.kind, FilterKind::LowShelfQ);
    assert_eq!(f1.freq, 105.0);
    assert_eq!(f1.gain, Some(5.5));
    assert_eq!(f1.q, Some(0.70));
    assert!(f1.enabled);
    assert_eq!(f1.index, Some(1));
    assert_eq!(f1.channel, Channel::Both);

    // And it round-trips.
    assert_eq!(parse(&serialize(&config)), config);
}

/// Stereo `Channel:` directives map to per-band channels and round-trip exactly.
#[test]
fn channels_round_trip() {
    let input = "\
Preamp: -3 dB
Filter 1: ON PK Fc 1000 Hz Gain -2 dB Q 1
Channel: L
Filter 2: ON PK Fc 2000 Hz Gain 2 dB Q 1
Channel: R
Filter 3: ON PK Fc 3000 Hz Gain 1 dB Q 1
Channel: L R
Filter 4: ON PK Fc 4000 Hz Gain -1 dB Q 1";

    let config = parse(input);
    let filters: Vec<_> = config.filters().collect();
    assert_eq!(filters.len(), 4);
    assert_eq!(filters[0].channel, Channel::Both);
    assert_eq!(filters[1].channel, Channel::Left);
    assert_eq!(filters[2].channel, Channel::Right);
    assert_eq!(filters[3].channel, Channel::Both);
    assert_eq!(config.preamp(), Some(-3.0));

    // The directives are regenerated verbatim, and the model round-trips.
    assert_eq!(serialize(&config), input);
    assert_eq!(parse(&serialize(&config)), config);
}

/// A channel spec we don't model is preserved verbatim and still scopes filters.
#[test]
fn unknown_channel_preserved() {
    let input = "\
Channel: C
Filter 1: ON PK Fc 100 Hz Gain 2 dB Q 1
Channel: L R
Filter 2: ON PK Fc 200 Hz Gain 1 dB Q 1";

    let config = parse(input);
    let filters: Vec<_> = config.filters().collect();
    assert_eq!(filters[0].channel, Channel::Other("C".to_string()));
    assert_eq!(filters[1].channel, Channel::Both);
    assert_eq!(serialize(&config), input);
    assert_eq!(parse(&serialize(&config)), config);
}

/// Lines we don't model must be preserved exactly, including odd spacing.
#[test]
fn preserves_unknown_lines_verbatim() {
    let input = "\
#   comment with   weird spacing
Device: Benchmark DAC
Include: other.txt
GraphicEQ: 25 -1; 40 -2; 63 -3
Convolution: impulse.wav
Filter: ON LS 6dB Fc 100 Hz Gain 3 dB";

    let config = parse(input);
    assert!(
        config.lines.iter().all(|l| matches!(l, Line::Raw(_))),
        "every line here is unsupported and should be Raw: {config:?}"
    );
    // Raw passthrough must be byte-identical.
    assert_eq!(serialize(&config), input);
}

/// `Filter:` (no index) and `OFF` state both round-trip.
#[test]
fn handles_bare_and_disabled_filters() {
    let input = "Filter: OFF PK Fc 500 Hz Gain 2 dB Q 1";
    let config = parse(input);

    let Line::Filter(f) = &config.lines[0] else {
        panic!("expected a filter, got {:?}", config.lines[0]);
    };
    assert!(!f.enabled);
    assert_eq!(f.index, None);
    assert_eq!(serialize(&config), input);
}

/// Preamp accepts the common textual variants and rejects garbage.
#[test]
fn parses_preamp_variants() {
    assert_eq!(parse("Preamp: -6 dB").preamp(), Some(-6.0));
    assert_eq!(parse("Preamp: -6.0 dB").preamp(), Some(-6.0));
    assert_eq!(parse("preamp: 0 dB").preamp(), Some(0.0)); // case-insensitive
    assert_eq!(parse("Preamp: 3.25").preamp(), Some(3.25)); // unit optional
    assert!(matches!(parse("Preamp: loud").lines[0], Line::Raw(_))); // garbage -> raw
}

/// Filter types are case-insensitive and unknown types fall back to Raw.
#[test]
fn filter_type_tokens() {
    assert!(matches!(
        parse("filter 1: on pk Fc 1000 Hz Gain -3 dB Q 1.41").lines[0],
        Line::Filter(_)
    ));
    // Unsupported type token -> preserved as Raw.
    assert!(matches!(
        parse("Filter 1: ON XYZ Fc 1000 Hz").lines[0],
        Line::Raw(_)
    ));
}

/// A non-trivial fractional Q survives the float round-trip exactly.
#[test]
fn fractional_values_round_trip_exactly() {
    let original = "Filter 1: ON PK Fc 1234.5 Hz Gain -2.7 dB Q 4.318";
    assert_eq!(serialize(&parse(original)), original);
}

/// Non-ASCII text must never panic the parser (config.txt is user-editable and
/// comments in any language are legitimate). Each line here puts a multi-byte
/// character across one of the prefix-check byte offsets (6 for "Filter",
/// 7 for "Preamp:", 8 for "Channel:"), which a byte-index slice would split
/// mid-character. They aren't valid directives, so they round-trip as Raw.
#[test]
fn non_ascii_lines_are_preserved_not_panicked_on() {
    for line in [
        "#中文注释",
        "Prea😀 x",
        "Chan😀 y",
        "Filtre… métal",
        "Préamp: -3 dB",
    ] {
        let config = parse(line);
        assert!(
            matches!(&config.lines[..], [Line::Raw(raw)] if raw == line),
            "{line:?} should be preserved verbatim as Raw: {config:?}"
        );
        assert_eq!(serialize(&config), line);
    }
}
