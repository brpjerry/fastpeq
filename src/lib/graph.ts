// Shared plotting math for the response graphs (ResponseCurve + CurveEditor): a
// log frequency axis and a preamp-centred dB axis. Centring on the preamp keeps
// a flat filter on the middle line and makes the 0-line / scale shift as the
// preamp changes, instead of the whole curve drifting toward an edge.

export const F_MIN = 20;
export const F_MAX = 20000;
const LOG_SPAN = Math.log10(F_MAX / F_MIN);

/** The drawable area of a plot, in pixels. */
export interface PlotBox {
  padL: number;
  padT: number;
  plotW: number;
  plotH: number;
}

/** X pixel for a frequency (log-spaced F_MIN..F_MAX). */
export function freqToX(f: number, box: PlotBox): number {
  return box.padL + (Math.log10(f / F_MIN) / LOG_SPAN) * box.plotW;
}

/** Frequency at an X pixel — inverse of {@link freqToX}. */
export function xToFreq(x: number, box: PlotBox): number {
  return F_MIN * Math.pow(F_MAX / F_MIN, (x - box.padL) / box.plotW);
}

/** Y pixel for a dB value, with the view centred on `preamp`; `dbMax` is the half-range. */
export function dbToY(db: number, preamp: number, dbMax: number, box: PlotBox): number {
  return box.padT + (1 - (db - preamp + dbMax) / (2 * dbMax)) * box.plotH;
}

/** Gain relative to preamp at a Y pixel — inverse of `dbToY(preamp + gain)`. */
export function yToGain(y: number, dbMax: number, box: PlotBox): number {
  return dbMax - ((y - box.padT) / box.plotH) * 2 * dbMax;
}

/** Round-dB gridlines (multiples of `step`) across the visible, preamp-centred range. */
export function dbGridLines(preamp: number, dbMax: number, step: number): number[] {
  const out: number[] = [];
  for (let v = Math.ceil((preamp - dbMax) / step) * step; v <= preamp + dbMax + 0.01; v += step) {
    out.push(v);
  }
  return out;
}

/** An SVG path for `curve` (sampled at `freqs`), clamped to the plot box. */
export function pathFrom(
  curve: number[],
  freqs: number[],
  preamp: number,
  dbMax: number,
  box: PlotBox,
): string {
  const yTop = box.padT;
  const yBot = box.padT + box.plotH;
  let d = "";
  for (let i = 0; i < freqs.length; i++) {
    const y = Math.max(yTop, Math.min(yBot, dbToY(curve[i], preamp, dbMax, box)));
    d += (i === 0 ? "M" : "L") + freqToX(freqs[i], box).toFixed(1) + "," + y.toFixed(1) + " ";
  }
  return d.trim();
}
