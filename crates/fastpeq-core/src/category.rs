//! Preset classification, stored as sidecar metadata alongside the preset
//! library (see [`crate::Manager::categories`]). Kept out of the preset `.txt`
//! files so the EQ config stays pure APO and category edits don't tangle with
//! the live/save flow.

/// What kind of device a preset is tuned for, e.g. `"speaker"`, `"headphone"`,
/// `"iem"`, plus any extended types the UI offers (`"estat"`, `"earbud"`, …).
/// A free-form string so new categories — each with its own icon — can be added
/// in the UI without a core change.
pub type Category = String;

/// The word a preset's sub-presets go by in the UI — "pads" for the headphone
/// types, "tips" for the in-ear types, none for speakers and uncategorised
/// presets (whose variants are just "sub-presets"). Mirrors `variantNoun` in
/// `src/lib/variants.ts`; the backend needs it for the tray label.
pub fn variant_noun(category: Option<&str>) -> Option<&'static str> {
    match category? {
        "headphone" | "estat" | "bluetooth_headphone" => Some("pads"),
        "iem" | "earbud" | "bluetooth_iem" | "bluetooth_earbud" => Some("tips"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_noun_follows_the_device_type() {
        for c in ["headphone", "estat", "bluetooth_headphone"] {
            assert_eq!(variant_noun(Some(c)), Some("pads"), "{c}");
        }
        for c in ["iem", "earbud", "bluetooth_iem", "bluetooth_earbud"] {
            assert_eq!(variant_noun(Some(c)), Some("tips"), "{c}");
        }
        assert_eq!(variant_noun(Some("speaker")), None);
        assert_eq!(variant_noun(Some("")), None);
        assert_eq!(variant_noun(None), None);
    }
}
