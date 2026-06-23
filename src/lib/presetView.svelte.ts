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
}

const KEY = "fastpeq.presetView";

let store = $state<Record<string, PresetView>>(loadJson(KEY, {}));

function patch(name: string, p: Partial<PresetView>): void {
  store = { ...store, [name]: { ...store[name], ...p } };
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

export function getMeasurement(name: string): PresetMeasurement | null {
  return store[name]?.measurement ?? null;
}
export function setMeasurement(name: string, m: PresetMeasurement): void {
  patch(name, { measurement: m });
}
export function clearMeasurement(name: string): void {
  patch(name, { measurement: undefined });
}
