// Accent color theming. The whole UI's highlights come from the --accent /
// --accent-2 CSS variables, so switching theme is just overriding those on
// :root. The choice is persisted in localStorage.

import { loadString, save } from "./storage";

export interface Accent {
  id: string;
  name: string;
  accent: string;
  accent2: string;
}

export const ACCENTS: Accent[] = [
  { id: "blue", name: "Blue", accent: "#4f8cff", accent2: "#3a6fd8" },
  { id: "teal", name: "Teal", accent: "#25c2ad", accent2: "#1aa491" },
  { id: "green", name: "Green", accent: "#4cc66a", accent2: "#3aa755" },
  { id: "purple", name: "Purple", accent: "#9a6bff", accent2: "#7d4ee0" },
  { id: "pink", name: "Pink", accent: "#ef6fb3", accent2: "#db4f9c" },
  { id: "orange", name: "Orange", accent: "#f5973a", accent2: "#df7c18" },
  { id: "rose", name: "Rose", accent: "#f76b6b", accent2: "#e54b4b" },
];

const KEY = "fastpeq.accent";

export function currentAccentId(): string {
  return loadString(KEY, "blue");
}

export function applyAccent(id: string): void {
  const a = ACCENTS.find((x) => x.id === id) ?? ACCENTS[0];
  const root = document.documentElement;
  root.style.setProperty("--accent", a.accent);
  root.style.setProperty("--accent-2", a.accent2);
  save(KEY, a.id);
}
