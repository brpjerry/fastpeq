//! [`Config`] → text.
//!
//! Modeled lines are emitted in a canonical APO form; [`Line::Raw`] lines are
//! written back exactly as they came in. A `Channel:` directive is injected
//! whenever a preamp/filter's channel differs from the one currently in effect,
//! which (with the parser) makes `parse(serialize(config)) == config` hold even
//! for left/right-scoped presets.

use super::model::{Channel, Config, Filter, Line};
use std::fmt::Write as _;

/// Serialize a configuration to APO text. No trailing newline is added; the
/// writer that ultimately persists `config.txt` is responsible for that.
pub fn serialize(config: &Config) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut emitted = Channel::Both;

    for line in &config.lines {
        let channel = match line {
            Line::Preamp { channel, .. } => Some(channel),
            Line::Filter(filter) => Some(&filter.channel),
            Line::Raw(_) => None,
        };
        if let Some(channel) = channel {
            if *channel != emitted {
                lines.push(channel_directive(channel));
                emitted = channel.clone();
            }
        }

        match line {
            Line::Raw(raw) => lines.push(raw.clone()),
            Line::Preamp { gain, .. } => lines.push(format!("Preamp: {} dB", num(*gain))),
            Line::Filter(filter) => lines.push(serialize_filter(filter)),
        }
    }

    lines.join("\n")
}

fn channel_directive(channel: &Channel) -> String {
    match channel {
        Channel::Both => "Channel: L R".to_string(),
        Channel::Left => "Channel: L".to_string(),
        Channel::Right => "Channel: R".to_string(),
        Channel::Other(spec) => format!("Channel: {spec}"),
    }
}

fn serialize_filter(f: &Filter) -> String {
    let mut s = String::new();
    match f.index {
        Some(n) => {
            let _ = write!(s, "Filter {}: ", n);
        }
        None => s.push_str("Filter: "),
    }
    s.push_str(if f.enabled { "ON " } else { "OFF " });
    s.push_str(f.kind.as_token());
    let _ = write!(s, " Fc {} Hz", num(f.freq));
    if let Some(gain) = f.gain {
        let _ = write!(s, " Gain {} dB", num(gain));
    }
    if let Some(q) = f.q {
        let _ = write!(s, " Q {}", num(q));
    }
    s
}

/// Shortest decimal that parses back to the identical `f64`.
fn num(v: f64) -> String {
    v.to_string()
}
