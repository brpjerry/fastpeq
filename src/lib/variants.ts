// Wording for sub-presets (a preset's pad / tip variants). Pure helpers so the
// category → label mapping is unit-testable and shared by the list, the create
// form, the editor header and the OSD.
//
// The storage model lives in the backend (`docs/PERSISTENCE.md`): a preset's
// file always mirrors its *selected* variant, so the UI only ever decides which
// variant is selected and how to name it.

/** The name the original config gets when a preset grows its first variant —
 *  mirrors the backend's `DEFAULT_VARIANT`. Only special at that moment; after
 *  that it is an ordinary variant the user may rename or delete. */
export const DEFAULT_VARIANT = "Default";

/** What a variant is called for a device type: ear pads, IEM tips, or nothing
 *  specific (speakers, uncategorized). */
export type VariantNoun = "pads" | "tips" | null;

const PADS = new Set(["headphone", "estat", "bluetooth_headphone"]);
const TIPS = new Set(["iem", "earbud", "bluetooth_iem", "bluetooth_earbud"]);

export function variantNoun(category: string | undefined): VariantNoun {
  if (category && PADS.has(category)) return "pads";
  if (category && TIPS.has(category)) return "tips";
  return null;
}

/** "Dekoni pads" / "Default tips" / plain "Dekoni" when there is no noun. */
export function variantLabel(variant: string, noun: VariantNoun): string {
  return noun ? `${variant} ${noun}` : variant;
}

/** The create button next to "+ New preset". */
export function variantCreateLabel(category: string | undefined): string {
  switch (variantNoun(category)) {
    case "pads":
      return "Pad roll";
    case "tips":
      return "Tip roll";
    default:
      return "+ sub-preset";
  }
}

/** The create form's name placeholder. */
export function variantPlaceholder(category: string | undefined): string {
  switch (variantNoun(category)) {
    case "pads":
      return "Pad name";
    case "tips":
      return "Tip name";
    default:
      return "Sub-preset name";
  }
}
