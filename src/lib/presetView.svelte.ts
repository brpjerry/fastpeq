// Per-preset curve-editor view state, keyed by preset name: which target curve
// to compare against, whether to compensate to it, and an imported FR
// measurement. Kept here (localStorage) rather than in the preset .txt so it
// doesn't tangle with the pure-APO EQ config. One reactive blob so the editor
// re-derives when any of it changes.

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
  targetMatchFreq?: number; // Hz the "Match" action aligns the target to the FR at
}

const KEY = "fastpeq.presetView";

let store = $state<Record<string, PresetView>>(loadJson(KEY, {}));

function patch(name: string, p: Partial<PresetView>): void {
  store = { ...store, [name]: { ...store[name], ...p } };
  saveJson(KEY, store);
}

// Keep the per-preset view state in step with rename/delete of the preset
// itself (it's keyed by name), so settings follow a rename and don't orphan on
// delete.
export function renamePresetView(from: string, to: string): void {
  if (from === to || !store[from]) return;
  const { [from]: entry, ...rest } = store;
  store = { ...rest, [to]: entry };
  saveJson(KEY, store);
}
export function clearPresetView(name: string): void {
  if (!(name in store)) return;
  const { [name]: _removed, ...rest } = store;
  store = rest;
  saveJson(KEY, store);
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

// Target trace adjustments: a manual dB offset and the frequency the "Match"
// action pins the target to the response at. Both per preset.
export function getTargetOffset(name: string): number {
  return store[name]?.targetOffset ?? 0;
}
export function setTargetOffset(name: string, db: number): void {
  patch(name, { targetOffset: db });
}
export function getTargetMatchFreq(name: string): number {
  return store[name]?.targetMatchFreq ?? 1000;
}
export function setTargetMatchFreq(name: string, hz: number): void {
  patch(name, { targetMatchFreq: hz });
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
