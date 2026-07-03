// Accent color theming. The whole UI's highlights come from the --accent /
// --accent-2 CSS variables, so switching theme is just overriding those on
// :root.
//
// Persistence: the source of truth is `theme.json` in the app data dir, written
// atomically by the backend (see hotkeys.svelte.ts for why WebView localStorage
// alone isn't safe). localStorage keeps a copy under the pre-file key so both
// windows can still apply the accent synchronously before mount (no flash of
// the default blue) and as a backup; initTheme() then re-applies from the file.

import * as api from "./api";
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

const KEY = "fastpeq.accent"; // localStorage cache/backup + one-time migration source
const STATE_KEY = "theme";

/** The cached accent id (synchronous, for the pre-mount apply). */
export function currentAccentId(): string {
  return loadString(KEY, "blue");
}

/** Recolor the UI. CSS variables only — persisting the choice is setAccent's
 *  job, so the startup re-apply of a cached id can never overwrite the file. */
export function applyAccent(id: string): void {
  const a = ACCENTS.find((x) => x.id === id) ?? ACCENTS[0];
  const root = document.documentElement;
  root.style.setProperty("--accent", a.accent);
  root.style.setProperty("--accent-2", a.accent2);
}

/** The user picked an accent: apply it and persist to theme.json (source of
 *  truth) + localStorage (cache/backup). */
export function setAccent(id: string): void {
  const a = ACCENTS.find((x) => x.id === id) ?? ACCENTS[0];
  applyAccent(a.id);
  save(KEY, a.id);
  // A failed file write isn't surfaced here — the localStorage copy above still
  // holds the choice, and the next change retries.
  api.saveUiState(STATE_KEY, JSON.stringify({ accent: a.id })).catch(() => {});
}

/** The document's accent id, or `null` when it's unusable. */
function parseTheme(json: string): string | null {
  try {
    const parsed: unknown = JSON.parse(json);
    if (parsed !== null && typeof parsed === "object" && !Array.isArray(parsed)) {
      const accent = (parsed as { accent?: unknown }).accent;
      if (typeof accent === "string") return accent;
    }
    return null;
  } catch {
    return null;
  }
}

/**
 * Load the accent from the backend's `theme.json` and re-apply it (each window
 * calls this once on startup, after the synchronous cached apply). When no file
 * exists yet, migrate the cached localStorage accent into it. An *unreadable*
 * file is deliberately NOT overwritten — the cached accent stays applied and
 * the file is only rewritten when the user next picks one. Same rules as
 * initHotkeys.
 */
export async function initTheme(): Promise<void> {
  let stored: string | null = null;
  try {
    stored = await api.loadUiState(STATE_KEY);
  } catch {
    // Backend unavailable (unit tests / plain browser) — the cached accent is
    // already applied, so there's nothing to do.
    return;
  }
  if (stored !== null) {
    const accent = parseTheme(stored);
    if (accent !== null) {
      save(KEY, accent);
      applyAccent(accent);
    }
    return;
  }
  api.saveUiState(STATE_KEY, JSON.stringify({ accent: currentAccentId() })).catch(() => {});
}
