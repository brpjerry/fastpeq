// User-defined target curves for the curve editor's compensation view. A target
// is a frequency response the corrected sound is aimed at; the built-in "Flat"
// target (no points) means "aim for a ruler-flat response". User targets are
// imported curves (same text format as REW measurements), normalised so the
// midband sits at 0 dB.
//
// Persistence: the source of truth is `targets.json` in the app data dir,
// written atomically by the backend (see hotkeys.svelte.ts for why WebView
// localStorage alone isn't safe — imported curves are data a user can't
// trivially recreate). localStorage is only read as a one-time migration
// source and kept as a backup copy.

import * as api from "./api";
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
const STATE_KEY = "targets";

// Only user-added targets are stored; Flat is prepended on read.
let userTargets = $state<Target[]>([]);

/** The document as a target list, or `null` when it's unusable. */
function parseTargets(json: string): Target[] | null {
  try {
    const parsed: unknown = JSON.parse(json);
    return Array.isArray(parsed) ? (parsed as Target[]) : null;
  } catch {
    return null;
  }
}

/**
 * Load the user targets from the backend's `targets.json` (App calls this once
 * on mount; the store starts with just Flat until it resolves). When no file
 * exists yet, migrate the pre-file localStorage list. An *unreadable* file is
 * deliberately NOT overwritten — the curves in it may be recoverable by hand;
 * the list just starts empty and the file is only rewritten on the next edit.
 * Same rules as initHotkeys.
 */
export async function initTargets(): Promise<void> {
  let stored: string | null = null;
  try {
    stored = await api.loadUiState(STATE_KEY);
  } catch {
    // Backend unavailable (unit tests / plain browser) — fall through to the
    // localStorage copy so the page still works.
  }
  if (stored !== null) {
    userTargets = parseTargets(stored) ?? [];
    return;
  }
  userTargets = loadJson<Target[]>(KEY, []);
  persist();
}

/** Write the list to targets.json (source of truth) + localStorage (backup). */
function persist(): void {
  saveJson(KEY, userTargets);
  // A failed file write isn't surfaced here — the localStorage copy above still
  // holds the list, and the next edit retries.
  api.saveUiState(STATE_KEY, JSON.stringify(userTargets)).catch(() => {});
}

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
  persist();
  return id;
}

/** Remove a user target (the built-in Flat can't be removed). */
export function removeTarget(id: string): void {
  if (id === FLAT_TARGET.id) return;
  userTargets = userTargets.filter((t) => t.id !== id);
  persist();
}
