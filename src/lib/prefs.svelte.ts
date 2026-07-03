// Shared, persisted UI preferences. Using a `.svelte.ts` module gives a single
// reactive value that both the settings page and every consumer read from.
//
// Persistence: the source of truth is `prefs.json` in the app data dir, written
// atomically by the backend (see hotkeys.svelte.ts for why WebView localStorage
// alone isn't safe). The prefs used to live as one localStorage key each; those
// keys are only read as a one-time migration source. The whole document is kept
// as a single localStorage backup copy under `fastpeq.prefs`.
//
// Every read validates its field (the file can be hand-edited), so a bad value
// falls back to the default instead of leaking into the UI.

import * as api from "./api";
import { loadBool, loadJson, loadNumber, loadRaw, loadString, saveJson } from "./storage";
import { BAND_COUNTS } from "./starter";

export type FilterSet = "basic" | "full";

interface Prefs {
  filterSet?: FilterSet;
  toneVolumeCap?: number;
  toneStep?: number;
  toneHeadroom?: number;
  specialtyIcons?: boolean;
  bluetoothIcons?: boolean;
  filterShapes?: boolean;
  autoPreamp?: boolean;
  bandCount?: number;
}

const KEY = "fastpeq.prefs";
const STATE_KEY = "prefs";

let prefs = $state<Prefs>({});

/** The document as a prefs record, or `null` when it's unusable. */
function parsePrefs(json: string): Prefs | null {
  try {
    const parsed: unknown = JSON.parse(json);
    return parsed !== null && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Prefs)
      : null;
  } catch {
    return null;
  }
}

// The pre-file storage: one localStorage key per pref. Only keys that were
// actually set are carried over, so a later change to a default still reaches
// migrated users. (fastpeq:autoPreamp predates the "fastpeq." naming; kept so
// existing settings survive.)
function legacyPrefs(): Prefs {
  const p: Prefs = {};
  if (loadRaw("fastpeq.filterSet") !== null)
    p.filterSet = loadString("fastpeq.filterSet", "full") === "basic" ? "basic" : "full";
  if (loadRaw("fastpeq.toneVolumeCap") !== null)
    p.toneVolumeCap = loadNumber("fastpeq.toneVolumeCap", 0.2);
  if (loadRaw("fastpeq.toneStep") !== null) p.toneStep = loadNumber("fastpeq.toneStep", 0.5);
  if (loadRaw("fastpeq.toneHeadroom") !== null)
    p.toneHeadroom = loadNumber("fastpeq.toneHeadroom", 0);
  if (loadRaw("fastpeq.specialtyIcons") !== null)
    p.specialtyIcons = loadBool("fastpeq.specialtyIcons");
  if (loadRaw("fastpeq.bluetoothIcons") !== null)
    p.bluetoothIcons = loadBool("fastpeq.bluetoothIcons");
  if (loadRaw("fastpeq.filterShapes") !== null)
    p.filterShapes = loadBool("fastpeq.filterShapes", true);
  if (loadRaw("fastpeq:autoPreamp") !== null) p.autoPreamp = loadBool("fastpeq:autoPreamp");
  if (loadRaw("fastpeq.bandCount") !== null) p.bandCount = loadNumber("fastpeq.bandCount", 10);
  return p;
}

/**
 * Load the prefs from the backend's `prefs.json` (App calls this once on mount;
 * reads return defaults until it resolves). When no file exists yet, migrate
 * the localStorage backup copy — or, before one of those exists, the pre-file
 * per-pref localStorage keys. An *unreadable* file is deliberately NOT
 * overwritten; reads fall back to defaults and the file is only rewritten on
 * the next change. Same rules as initHotkeys.
 */
export async function initPrefs(): Promise<void> {
  let stored: string | null = null;
  try {
    stored = await api.loadUiState(STATE_KEY);
  } catch {
    // Backend unavailable (unit tests / plain browser) — fall through to the
    // localStorage copy so the page still works.
  }
  if (stored !== null) {
    prefs = parsePrefs(stored) ?? {};
    return;
  }
  prefs = loadJson<Prefs | null>(KEY, null) ?? legacyPrefs();
  persist();
}

/** Write the document to prefs.json (source of truth) + localStorage (backup). */
function persist(): void {
  saveJson(KEY, prefs);
  // A failed file write isn't surfaced here — the localStorage copy above still
  // holds the document, and the next change retries.
  api.saveUiState(STATE_KEY, JSON.stringify(prefs)).catch(() => {});
}

function set(patch: Partial<Prefs>): void {
  prefs = { ...prefs, ...patch };
  persist();
}

// Default to the full list so existing behaviour is unchanged.
export function getFilterSet(): FilterSet {
  return prefs.filterSet === "basic" ? "basic" : "full";
}

export function setFilterSet(v: FilterSet): void {
  set({ filterSet: v });
}

// Maximum volume (linear amplitude, 0..1) the curve-editor tone generator can
// reach. Default 0.2 (20%); clamped to a sane 5%–100% range.
export function getToneVolumeCap(): number {
  const v = prefs.toneVolumeCap;
  return typeof v === "number" && v >= 0.05 && v <= 1 ? v : 0.2;
}

export function setToneVolumeCap(v: number): void {
  set({ toneVolumeCap: Math.max(0.05, Math.min(1, v)) });
}

// Step size (dB) for adjusting a tone control — by keyboard (arrow keys / scroll
// on a tone knob) and by a tone-bound global hotkey. Default 0.5 dB.
export function getToneStep(): number {
  const v = prefs.toneStep;
  return typeof v === "number" && v >= 0.1 && v <= 5 ? v : 0.5;
}

export function setToneStep(v: number): void {
  set({ toneStep: v >= 0.1 && v <= 5 ? v : 0.5 });
}

// Tone headroom (dB) reserved when Auto Preamp is active. This allows tone
// controls to be boosted up to this limit before the auto preamp pulls down
// to compensate. Default 0 dB.
export function getToneHeadroom(): number {
  const v = prefs.toneHeadroom;
  return typeof v === "number" && v >= 0 && v <= 30 ? v : 0;
}

export function setToneHeadroom(v: number): void {
  set({ toneHeadroom: Math.max(0, Math.min(30, v)) });
}

// Which optional preset-category groups can be assigned. Off by default; their
// icons still *display* on presets that use them, regardless of these.
export function getSpecialtyIcons(): boolean {
  return prefs.specialtyIcons === true;
}
export function setSpecialtyIcons(v: boolean): void {
  set({ specialtyIcons: v });
}

export function getBluetoothIcons(): boolean {
  return prefs.bluetoothIcons === true;
}
export function setBluetoothIcons(v: boolean): void {
  set({ bluetoothIcons: v });
}

// Curve-editor handle style: draw each band's actual filter shape (default) or
// the older dashed stem from the handle down to the preamp line.
export function getFilterShapes(): boolean {
  return prefs.filterShapes !== false;
}
export function setFilterShapes(v: boolean): void {
  set({ filterShapes: v });
}

// The editor's Auto Preamp toggle: hold the preamp at the lowest value that
// keeps the peak boost from clipping. Off by default.
export function getAutoPreamp(): boolean {
  return prefs.autoPreamp === true;
}
export function setAutoPreamp(v: boolean): void {
  set({ autoPreamp: v });
}

// How many bands a new preset starts with (the Settings "New presets" choice).
export function defaultBandCount(): number {
  const v = prefs.bandCount;
  return typeof v === "number" && BAND_COUNTS.includes(v) ? v : 10;
}
export function setDefaultBandCount(n: number): void {
  set({ bandCount: n });
}
