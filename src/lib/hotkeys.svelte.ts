// User-configurable global hotkeys, persisted to localStorage. Each binding is a
// modifier (Ctrl+Alt or Ctrl+Shift) plus a single key, mapped to an action. The
// list is ordered (the Hotkeys page lets the user reorder it). The backend just
// registers the accelerators and emits an event on press; App.svelte dispatches
// the action, so all the semantics live here on the frontend.

import { loadJson, saveJson } from "./storage";

export type HotkeyMod = "ctrl-alt" | "ctrl-shift";
// "device" switches the default Windows audio output (backed by the
// list_audio_devices / set_default_audio_device commands).
export type HotkeyAction = "preset" | "bypass" | "tone-up" | "tone-down" | "tone-reset" | "device";
export type ToneControl = "bass" | "mid" | "treble";

export interface Hotkey {
  id: string;
  mod: HotkeyMod;
  key: string; // a single A–Z / 0–9, normalized uppercase
  action: HotkeyAction;
  preset?: string; // principal for "preset"
  tone?: ToneControl; // principal for "tone-up" / "tone-down"
  device?: string; // principal for "device": the audio endpoint id
  deviceName?: string; // label cached at pick time, so an unplugged device still reads clearly
}

const KEY = "fastpeq.hotkeys";

function freshId(): string {
  return `h${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
}

// Seed a single Ctrl+Alt+B → Bypass binding on first run only. Once the list has
// been written (even emptied), it's respected — a deleted default stays deleted.
function initial(): Hotkey[] {
  const stored = loadJson<Hotkey[] | null>(KEY, null);
  if (stored !== null) return stored;
  const seed: Hotkey[] = [{ id: freshId(), mod: "ctrl-alt", key: "B", action: "bypass" }];
  saveJson(KEY, seed);
  return seed;
}

let hotkeys = $state<Hotkey[]>(initial());

export function getHotkeys(): Hotkey[] {
  return hotkeys;
}

/** Append a blank binding (user fills in the key + principal); returns its id. */
export function addHotkey(): string {
  const h: Hotkey = { id: freshId(), mod: "ctrl-alt", key: "", action: "preset" };
  hotkeys = [...hotkeys, h];
  saveJson(KEY, hotkeys);
  return h.id;
}

export function updateHotkey(id: string, patch: Partial<Hotkey>): void {
  hotkeys = hotkeys.map((h) => (h.id === id ? { ...h, ...patch } : h));
  saveJson(KEY, hotkeys);
}

export function removeHotkey(id: string): void {
  hotkeys = hotkeys.filter((h) => h.id !== id);
  saveJson(KEY, hotkeys);
}

/** Reorder: move the entry at `from` to index `to`. No-op for out-of-range. */
export function moveHotkey(from: number, to: number): void {
  if (from === to || from < 0 || to < 0 || from >= hotkeys.length || to >= hotkeys.length) return;
  const next = [...hotkeys];
  const [moved] = next.splice(from, 1);
  next.splice(to, 0, moved);
  hotkeys = next;
  saveJson(KEY, hotkeys);
}

/** A valid hotkey key is exactly one uppercase letter or digit. */
export function validKey(key: string): boolean {
  return /^[A-Z0-9]$/.test(key);
}

const MOD_PREFIX: Record<HotkeyMod, string> = {
  "ctrl-alt": "Ctrl+Alt+",
  "ctrl-shift": "Ctrl+Shift+",
};

/** The Tauri accelerator string for a binding, or null if its key is invalid. */
export function accelerator(h: Hotkey): string | null {
  return validKey(h.key) ? MOD_PREFIX[h.mod] + h.key : null;
}

/**
 * `{id, accelerator}` for every binding with a valid, unique combo — what gets
 * handed to the backend to register. Duplicate combos keep the first occurrence.
 */
export function accelerators(): { id: string; accelerator: string }[] {
  const seen = new Set<string>();
  const out: { id: string; accelerator: string }[] = [];
  for (const h of hotkeys) {
    const acc = accelerator(h);
    if (!acc || seen.has(acc)) continue;
    seen.add(acc);
    out.push({ id: h.id, accelerator: acc });
  }
  return out;
}

/**
 * Ids of bindings whose combo is already claimed by an *earlier* binding —
 * exactly the ones `accelerators()` drops. These never reach the backend (so
 * they're never in the failed-registration list either), and silently do
 * nothing; the Hotkeys page flags them so the dead row is visible. Invalid keys
 * are ignored here (the key input flags those on its own).
 */
export function duplicateIds(): Set<string> {
  const seen = new Set<string>();
  const dups = new Set<string>();
  for (const h of hotkeys) {
    const acc = accelerator(h);
    if (!acc) continue;
    if (seen.has(acc)) dups.add(h.id);
    else seen.add(acc);
  }
  return dups;
}
