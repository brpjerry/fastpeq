//! Preset classification, stored as sidecar metadata alongside the preset
//! library (see [`crate::Manager::categories`]). Kept out of the preset `.txt`
//! files so the EQ config stays pure APO and category edits don't tangle with
//! the live/save flow.

/// What kind of device a preset is tuned for, e.g. `"speaker"`, `"headphone"`,
/// `"iem"`, plus any extended types the UI offers (`"estat"`, `"earbud"`, …).
/// A free-form string so new categories — each with its own icon — can be added
/// in the UI without a core change.
pub type Category = String;
