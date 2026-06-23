// Shared, persisted UI preferences. Using a `.svelte.ts` module gives a single
// reactive value that both the settings page and every TypeSelect read from.

import { loadBool, loadNumber, loadString, save } from "./storage";

export type FilterSet = "basic" | "full";

const KEY = "fastpeq.filterSet";

// Default to the full list so existing behaviour is unchanged.
let filterSet = $state<FilterSet>(loadString(KEY, "full") === "basic" ? "basic" : "full");

export function getFilterSet(): FilterSet {
  return filterSet;
}

export function setFilterSet(v: FilterSet): void {
  filterSet = v;
  save(KEY, v);
}

// Maximum volume (linear amplitude, 0..1) the curve-editor tone generator can
// reach. Default 0.2 (20%); clamped to a sane 5%–100% range.
const CAP_KEY = "fastpeq.toneVolumeCap";

function loadCap(): number {
  const v = loadNumber(CAP_KEY, 0.2);
  return v >= 0.05 && v <= 1 ? v : 0.2;
}

let toneVolumeCap = $state<number>(loadCap());

export function getToneVolumeCap(): number {
  return toneVolumeCap;
}

export function setToneVolumeCap(v: number): void {
  toneVolumeCap = Math.max(0.05, Math.min(1, v));
  save(CAP_KEY, toneVolumeCap);
}

// Which optional preset-category groups can be assigned. Off by default; their
// icons still *display* on presets that use them, regardless of these.
let specialtyIcons = $state<boolean>(loadBool("fastpeq.specialtyIcons"));
export function getSpecialtyIcons(): boolean {
  return specialtyIcons;
}
export function setSpecialtyIcons(v: boolean): void {
  specialtyIcons = v;
  save("fastpeq.specialtyIcons", v);
}

let bluetoothIcons = $state<boolean>(loadBool("fastpeq.bluetoothIcons"));
export function getBluetoothIcons(): boolean {
  return bluetoothIcons;
}
export function setBluetoothIcons(v: boolean): void {
  bluetoothIcons = v;
  save("fastpeq.bluetoothIcons", v);
}

// Curve-editor handle style: draw each band's actual filter shape (default) or
// the older dashed stem from the handle down to the preamp line.
let filterShapes = $state<boolean>(loadBool("fastpeq.filterShapes", true));
export function getFilterShapes(): boolean {
  return filterShapes;
}
export function setFilterShapes(v: boolean): void {
  filterShapes = v;
  save("fastpeq.filterShapes", v);
}
