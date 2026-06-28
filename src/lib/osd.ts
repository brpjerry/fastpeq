// Shared model for the on-screen-display (OSD) overlay. The main window builds an
// OsdPayload describing the result of a fired hotkey and emits it over the Tauri
// event bus; the overlay window (src/osd) renders it. Kept UI-free so the
// action→payload mapping is unit-testable.

import type { Hotkey, ToneControl } from "./hotkeys.svelte";

/** Tauri event the main window emits and the overlay window listens for. */
export const OSD_EVENT = "osd:show";

export interface OsdBar {
  value: number;
  min: number;
  max: number;
}

export interface OsdPayload {
  title: string;
  detail?: string;
  /** A centered level bar (used for tone), drawn relative to [min, max]. */
  bar?: OsdBar;
}

/** Resulting state the payload needs, captured after the action has run. */
export interface OsdContext {
  tone: Record<ToneControl, number>;
  bypassed: boolean;
  /** The preset name, if the binding's preset still exists (else omitted). */
  presetName?: string;
}

const TONE_LABEL: Record<ToneControl, string> = { bass: "Bass", mid: "Mids", treble: "Treble" };

/** Signed one-decimal dB, e.g. "+3.0 dB", "-1.5 dB", "0.0 dB". */
function fmtDb(v: number): string {
  return `${v > 0 ? "+" : ""}${v.toFixed(1)} dB`;
}

/**
 * Build the OSD payload describing the result of a fired hotkey, or `null` when
 * there's nothing worth showing (e.g. a stale preset/device reference that the
 * action itself no-ops on). The tone Knob range is ±12 dB.
 */
export function payloadForHotkey(h: Hotkey, ctx: OsdContext): OsdPayload | null {
  switch (h.action) {
    case "bypass":
      return { title: "Bypass", detail: ctx.bypassed ? "Filters off" : "EQ on" };
    case "preset":
      return ctx.presetName ? { title: "Preset", detail: ctx.presetName } : null;
    case "device":
      return h.device ? { title: "Output device", detail: h.deviceName ?? h.device } : null;
    case "tone-reset":
      return { title: "Tone", detail: "Reset" };
    case "tone-up":
    case "tone-down": {
      const control = h.tone ?? "bass";
      const value = ctx.tone[control];
      return { title: TONE_LABEL[control], detail: fmtDb(value), bar: { value, min: -12, max: 12 } };
    }
  }
}
