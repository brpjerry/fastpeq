// Parse and prepare a REW (Room EQ Wizard) measurement export for overlaying on
// the curve editor. REW writes plain text: `*`-prefixed header/comment lines,
// then rows of `freq  SPL  [phase]`. We keep freq + SPL, normalise so the
// midband sits at 0 dB (the graph is a relative ±dB view), and interpolate onto
// the editor's frequency grid in log space.

export interface MeasPoint {
  freq: number;
  spl: number;
}

/** Parse REW measurement text into frequency/SPL points (header lines skipped). */
export function parseRew(text: string): MeasPoint[] {
  const out: MeasPoint[] = [];
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("*")) continue;
    const tok = line.split(/[\s,]+/);
    const freq = Number(tok[0]);
    const spl = Number(tok[1]);
    // A header row like "Freq(Hz) SPL(dB)" yields NaN and is skipped here.
    if (Number.isFinite(freq) && Number.isFinite(spl) && freq > 0) {
      out.push({ freq, spl });
    }
  }
  out.sort((a, b) => a.freq - b.freq);
  return out;
}

/**
 * Resample a (sorted) measurement onto at most `n` log-spaced points across its
 * own frequency range. REW exports can run to thousands of points; we store the
 * measurement per preset (localStorage) and the graphs resample onto the plot
 * grid anyway, so keeping ~256 points keeps storage small with no visible loss.
 */
export function downsample(points: MeasPoint[], n = 256): MeasPoint[] {
  if (points.length <= n) return points;
  const f0 = points[0].freq;
  const f1 = points[points.length - 1].freq;
  const freqs = Array.from({ length: n }, (_, i) => f0 * Math.pow(f1 / f0, i / (n - 1)));
  const spls = sampleAt(points, freqs);
  return freqs.map((freq, i) => ({ freq, spl: spls[i] }));
}

/** Shift the whole curve so the 300 Hz–3 kHz average reads 0 dB. */
export function normalize(points: MeasPoint[]): MeasPoint[] {
  if (points.length === 0) return points;
  const band = points.filter((p) => p.freq >= 300 && p.freq <= 3000);
  const ref = band.length ? band : points;
  const mean = ref.reduce((s, p) => s + p.spl, 0) / ref.length;
  return points.map((p) => ({ freq: p.freq, spl: p.spl - mean }));
}

/** Sample the (sorted) measurement at each frequency, linearly in log-freq. */
export function sampleAt(points: MeasPoint[], freqs: number[]): number[] {
  const n = points.length;
  return freqs.map((f) => {
    if (n === 0) return 0;
    if (f <= points[0].freq) return points[0].spl;
    if (f >= points[n - 1].freq) return points[n - 1].spl;
    let lo = 0;
    let hi = n - 1;
    while (hi - lo > 1) {
      const mid = (lo + hi) >> 1;
      if (points[mid].freq <= f) lo = mid;
      else hi = mid;
    }
    const a = points[lo];
    const b = points[hi];
    const t = (Math.log(f) - Math.log(a.freq)) / (Math.log(b.freq) - Math.log(a.freq));
    return a.spl + t * (b.spl - a.spl);
  });
}
