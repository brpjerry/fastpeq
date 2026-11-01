import { describe, it, expect } from "vite-plus/test";
import {
  DEFAULT_VARIANT,
  variantCreateLabel,
  variantLabel,
  variantNoun,
  variantPlaceholder,
} from "./variants";

describe("sub-preset wording", () => {
  it("calls headphone-family variants pads and in-ear-family variants tips", () => {
    for (const c of ["headphone", "estat", "bluetooth_headphone"]) {
      expect(variantNoun(c)).toBe("pads");
      expect(variantCreateLabel(c)).toBe("Pad roll");
      expect(variantPlaceholder(c)).toBe("Pad name");
    }
    for (const c of ["iem", "earbud", "bluetooth_iem", "bluetooth_earbud"]) {
      expect(variantNoun(c)).toBe("tips");
      expect(variantCreateLabel(c)).toBe("Tip roll");
      expect(variantPlaceholder(c)).toBe("Tip name");
    }
  });

  it("falls back to plain sub-preset wording for speakers and uncategorized", () => {
    for (const c of ["speaker", undefined, "something-new"]) {
      expect(variantNoun(c)).toBeNull();
      expect(variantCreateLabel(c)).toBe("+ sub-preset");
      expect(variantPlaceholder(c)).toBe("Sub-preset name");
    }
  });

  it("labels a variant with its noun, or bare when there is none", () => {
    expect(variantLabel(DEFAULT_VARIANT, "pads")).toBe("Default pads");
    expect(variantLabel("Foam", "tips")).toBe("Foam tips");
    expect(variantLabel("Near-field", null)).toBe("Near-field");
  });
});
