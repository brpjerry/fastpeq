// Per-preset curve-editor view state, keyed by preset name: which target curve
// to compare against, whether to compensate to it, and an imported FR
// measurement. Kept in its own store rather than in the preset .txt so it
// doesn't tangle with the pure-APO EQ config. One reactive blob so the editor
// re-derives when any of it changes.
//
// Persistence: the source of truth is `preset-view.json` in the app data dir,
// written atomically by the backend (see hotkeys.svelte.ts for the history —
// WebView localStorage can be silently discarded, and imported measurements
// are exactly the kind of data a user can't trivially recreate). localStorage
// is only read as a one-time migration source and kept as a backup copy.

import * as api from "./api";
import { loadJson, saveJson } from "./storage";
import type { MeasPoint } from "./measurement";

export interface PresetMeasurement {
  name: string;
  points: MeasPoint[];
}

interface PresetView {
  targetId?: string;
  compensate?: boolean;
  showMeasRef?: boolean;
  showTargetRef?: boolean;
  measurement?: PresetMeasurement;
  targetOffset?: number; // dB the target trace is shifted by
  targetAlignFreq?: number; // Hz the "Align" action aligns the target to the FR at
}

const KEY = "fastpeq.presetView";
const STATE_KEY = "preset-view";

let store = $state<Record<string, PresetView>>({});

/** The document as a view-state record, or `null` when it's unusable. */
function parseStore(json: string): Record<string, PresetView> | null {
  try {
    const parsed: unknown = JSON.parse(json);
    return parsed !== null && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, PresetView>)
      : null;
  } catch {
    return null;
  }
}

/**
 * Load the view state from the backend's `preset-view.json` (App calls this
 * once on mount; the store starts empty until it resolves). When no file exists
 * yet, migrate the pre-file localStorage blob. An *unreadable* file is
 * deliberately NOT overwritten — the measurements in it may be recoverable by
 * hand; the store just starts empty and the file is only rewritten on the next
 * edit. Same rules as initHotkeys.
 */
export async function initPresetView(): Promise<void> {
  let stored: string | null = null;
  try {
    stored = await api.loadUiState(STATE_KEY);
  } catch {
    // Backend unavailable (unit tests / plain browser) — fall through to the
    // localStorage copy so the page still works.
  }
  if (stored !== null) {
    store = parseStore(stored) ?? {};
    return;
  }
  store = loadJson(KEY, {});
  persist();
}

/** Write the blob to preset-view.json (source of truth) + localStorage (backup). */
function persist(): void {
  saveJson(KEY, store);
  // A failed file write isn't surfaced here — the localStorage copy above still
  // holds the data, and the next edit retries.
  api.saveUiState(STATE_KEY, JSON.stringify(store)).catch(() => {});
}

function patch(name: string, p: Partial<PresetView>): void {
  store = { ...store, [name]: { ...store[name], ...p } };
  persist();
}

// Keep the per-preset view state in step with rename/delete of the preset
// itself (it's keyed by name), so settings follow a rename and don't orphan on
// delete.
export function renamePresetView(from: string, to: string): void {
  if (from === to || !store[from]) return;
  const { [from]: entry, ...rest } = store;
  store = { ...rest, [to]: entry };
  persist();
}
export function clearPresetView(name: string): void {
  if (!(name in store)) return;
  const { [name]: _removed, ...rest } = store;
  store = rest;
  persist();
}

export function getTargetId(name: string): string {
  return store[name]?.targetId ?? "flat";
}
export function setTargetId(name: string, id: string): void {
  patch(name, { targetId: id });
}

export function getCompensate(name: string): boolean {
  return store[name]?.compensate ?? false;
}
export function setCompensate(name: string, on: boolean): void {
  patch(name, { compensate: on });
}

// Whether the raw-measurement and target dashed reference lines are drawn
// (independently). The FR trace always keeps the measurement data regardless.
// Both shown by default.
export function getShowMeasRef(name: string): boolean {
  return store[name]?.showMeasRef ?? true;
}
export function setShowMeasRef(name: string, on: boolean): void {
  patch(name, { showMeasRef: on });
}
export function getShowTargetRef(name: string): boolean {
  return store[name]?.showTargetRef ?? true;
}
export function setShowTargetRef(name: string, on: boolean): void {
  patch(name, { showTargetRef: on });
}

// Target trace adjustments: a manual dB offset and the frequency the "Align"
// action pins the target to the response at. Both per preset.
export function getTargetOffset(name: string): number {
  return store[name]?.targetOffset ?? 0;
}
export function setTargetOffset(name: string, db: number): void {
  patch(name, { targetOffset: db });
}
export function getTargetAlignFreq(name: string): number {
  return store[name]?.targetAlignFreq ?? 1000;
}
export function setTargetAlignFreq(name: string, hz: number): void {
  patch(name, { targetAlignFreq: hz });
}

export function getMeasurement(name: string): PresetMeasurement | null {
  return store[name]?.measurement ?? null;
}
export function setMeasurement(name: string, m: PresetMeasurement): void {
  patch(name, { measurement: m });
}
export function clearMeasurement(name: string): void {
  patch(name, { measurement: undefined });
}
