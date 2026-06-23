// User-defined target curves for the curve editor's compensation view. A target
// is a frequency response the corrected sound is aimed at; the built-in "Flat"
// target (no points) means "aim for a ruler-flat response". User targets are
// imported curves (same text format as REW measurements), normalised so the
// midband sits at 0 dB, and persisted to localStorage.

import { loadJson, saveJson } from "./storage";
import type { MeasPoint } from "./measurement";

export interface Target {
  id: string;
  name: string;
  points: MeasPoint[]; // empty = flat (0 dB at every frequency)
}

/** The always-present default: a ruler-flat target. */
export const FLAT_TARGET: Target = { id: "flat", name: "Flat", points: [] };

const KEY = "fastpeq.targets";

// Only user-added targets are stored; Flat is prepended on read.
let userTargets = $state<Target[]>(loadJson<Target[]>(KEY, []));

/** All selectable targets, Flat first. */
export function getTargets(): Target[] {
  return [FLAT_TARGET, ...userTargets];
}

/** Look up a target by id, falling back to Flat for unknown/removed ids. */
export function getTarget(id: string): Target {
  return getTargets().find((t) => t.id === id) ?? FLAT_TARGET;
}

/** Add a target curve; returns its new id. */
export function addTarget(name: string, points: MeasPoint[]): string {
  const id = `t${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
  userTargets = [...userTargets, { id, name, points }];
  saveJson(KEY, userTargets);
  return id;
}

/** Remove a user target (the built-in Flat can't be removed). */
export function removeTarget(id: string): void {
  if (id === FLAT_TARGET.id) return;
  userTargets = userTargets.filter((t) => t.id !== id);
  saveJson(KEY, userTargets);
}
