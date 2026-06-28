export const CATEGORY_NONE = "__none";

export const CATEGORIES: { value: string; label: string; group: "base" | "specialty" | "bluetooth" }[] = [
  { value: "headphone", label: "Headphone", group: "base" },
  { value: "iem", label: "IEM", group: "base" },
  { value: "estat", label: "Electrostatic", group: "specialty" },
  { value: "earbud", label: "Earbud", group: "specialty" },
  { value: "bluetooth_headphone", label: "BT Headphone", group: "bluetooth" },
  { value: "bluetooth_iem", label: "BT IEM", group: "bluetooth" },
  { value: "bluetooth_earbud", label: "BT Earbud", group: "bluetooth" },
  { value: "speaker", label: "Speaker", group: "base" },
];

export const CATEGORY_LABELS: Record<string, string> = Object.fromEntries(
  CATEGORIES.map((c) => [c.value, c.label]),
);
