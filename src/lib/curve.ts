// Pure helpers for the curve editor's measurement/target overlays and the
// FR-to-target gap readout. Kept out of the (layout-gated, hard-to-render)
// component so the math is unit-tested, like eq.ts / measurement.ts.

import { responseCurve, type CurveFilter } from "./eq";
import { sampleAt, type MeasPoint } from "./measurement";

/** Interpolated value (dB) of a measurement/target curve at one frequency; 0 if empty. */
export function valueAt(points: MeasPoint[], freq: number): number {
  return points.length ? sampleAt(points, [freq])[0] : 0;
}

/** Combined FR — preamp + filters + measurement — at one frequency, in dB. */
export function frValueAt(
  filters: CurveFilter[],
  preamp: number,
  measurement: MeasPoint[],
  freq: number,
): number {
  return responseCurve(filters, preamp, [freq])[0] + valueAt(measurement, freq);
}

/** The target line value — preamp + target — at one frequency, in dB. */
export function targetValueAt(target: MeasPoint[], preamp: number, freq: number): number {
  return valueAt(target, freq) + preamp;
}

/** Absolute dB gap between the FR and the target at one frequency. */
export function gapDb(
  filters: CurveFilter[],
  frPreamp: number,
  measurement: MeasPoint[],
  target: MeasPoint[],
  targetPreamp: number,
  freq: number,
): number {
  return Math.abs(
    frValueAt(filters, frPreamp, measurement, freq) - targetValueAt(target, targetPreamp, freq),
  );
}

/** Subtract a (sampled) target from a response for the "compensate" view. */
export function compensateCurve(resp: number[], target: number[]): number[] {
  return resp.map((v, i) => v - target[i]);
}

/**
 * The dB offset that shifts `target` so its displayed line meets the FR at
 * `freq`. Both the FR and the target line carry the preamp, so it cancels and
 * the offset is just (filters + measurement) − target at that frequency.
 */
export function matchOffset(
  filters: CurveFilter[],
  preamp: number,
  measurement: MeasPoint[],
  target: MeasPoint[],
  freq: number,
): number {
  return frValueAt(filters, preamp, measurement, freq) - preamp - valueAt(target, freq);
}
