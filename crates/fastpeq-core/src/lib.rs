//! `fastpeq-core` — the UI-agnostic heart of fastpeq.
//!
//! Equalizer APO is driven entirely by a plain-text `config.txt` that the APO
//! engine watches and live-reloads. This crate owns the model of that file:
//! detecting where it lives ([`apo::env`]), turning text into a structured
//! [`Config`] ([`apo::parse`]), and turning a [`Config`] back into APO text
//! ([`apo::serialize`]).
//!
//! The guiding invariant is a **lossless model round-trip**:
//! `parse(serialize(config)) == config`. Anything the model does not understand
//! is preserved verbatim as [`Line::Raw`], so a user's `Include:`, `Device:`,
//! `Convolution:` and comment lines always survive an edit untouched.

pub mod apo;
pub mod category;
pub mod history;
pub mod manager;
pub mod offload;
pub mod peace;
pub mod provenance;
pub mod store;
pub mod tone;

pub use apo::env;
pub use apo::{Channel, Config, Filter, FilterKind, Line, parse, serialize};
pub use category::Category;
pub use history::{PresetHistory, Revision, RevisionOp};
pub use manager::{ImportReport, Manager};
pub use offload::{
    HardwareProfile, HwBand, HwFilterType, OffloadMode, Split, biquad_coeffs, peak_gain_db, split,
};
pub use store::PresetStore;
pub use tone::Tone;
