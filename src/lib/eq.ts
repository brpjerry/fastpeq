// Biquad magnitude-response math (RBJ Audio EQ Cookbook) for the live curve,
// plus metadata about each filter type. Computed on the client so dragging a
// slider redraws instantly with no IPC round-trip.
import type { Channel, FilterKind } from "./types";

export interface FilterTypeInfo {
  value: FilterKind;
  label: string;
  token: string;
}

export const FILTER_TYPES: FilterTypeInfo[] = [
  { value: "Peak", label: "Peaking", token: "PK" },
  { value: "LowShelf", label: "Low shelf", token: "LS" },
  { value: "HighShelf", label: "High shelf", token: "HS" },
  { value: "LowShelfQ", label: "Low shelf (Q)", token: "LSC" },
  { value: "HighShelfQ", label: "High shelf (Q)", token: "HSC" },
  { value: "LowPass", label: "Low pass", token: "LP" },
  { value: "HighPass", label: "High pass", token: "HP" },
  { value: "LowPassQ", label: "Low pass (Q)", token: "LPQ" },
  { value: "HighPassQ", label: "High pass (Q)", token: "HPQ" },
  { value: "BandPass", label: "Band pass", token: "BP" },
  { value: "Notch", label: "Notch", token: "NO" },
  { value: "AllPass", label: "All pass", token: "AP" },
];

// The reduced set offered when the "basic" filter mode is on: peaking + the two
// adjustable-Q shelves (PK / LSC / HSC) — the workhorses for most EQ presets.
export const BASIC_FILTER_KINDS: FilterKind[] = ["Peak", "LowShelfQ", "HighShelfQ"];

const GAIN_KINDS = new Set<FilterKind>([
  "Peak",
  "LowShelf",
  "HighShelf",
  "LowShelfQ",
  "HighShelfQ",
]);
const Q_KINDS = new Set<FilterKind>([
  "Peak",
  "LowShelfQ",
  "HighShelfQ",
  "LowPassQ",
  "HighPassQ",
  "BandPass",
  "Notch",
  "AllPass",
]);

export const kindHasGain = (k: FilterKind) => GAIN_KINDS.has(k);
export const kindHasQ = (k: FilterKind) => Q_KINDS.has(k);
export const defaultQ = (k: FilterKind) => (k === "Peak" ? 1 : Math.SQRT1_2);

export const SAMPLE_RATE = 48000;

/** Log-spaced frequencies from 20 Hz to 20 kHz for plotting. */
export const FREQS: number[] = (() => {
  const n = 240;
  const f0 = 20;
  const f1 = 20000;
  return Array.from({ length: n }, (_, i) => f0 * Math.pow(f1 / f0, i / (n - 1)));
})();

export interface CurveFilter {
  enabled: boolean;
  kind: FilterKind;
  freq: number;
  gain: number | null;
  q: number | null;
  channel: Channel;
}

/** Whether a channel contributes to the given side's response. */
export const inChannel = (c: Channel, side: "left" | "right") =>
  c.kind === "both" || c.kind === side;

/**
 * Per-channel preamp trim (dB) for a balance value that is itself in dB:
 * 0 = centered, >0 makes the right side louder (cuts the left channel by that
 * many dB), <0 makes the left side louder (cuts the right). The louder side
 * stays at 0; only the quieter side is attenuated.
 */
export function balanceTrim(balanceDb: number): { left: number; right: number } {
  if (balanceDb > 0) return { left: -balanceDb, right: 0 };
  if (balanceDb < 0) return { left: 0, right: balanceDb };
  return { left: 0, right: 0 };
}

/** Inverse of balanceTrim: recover the balance (dB) from a one-sided trim. */
export function balanceFromTrim(side: "left" | "right", trimDb: number): number {
  const cut = Math.min(0, trimDb); // attenuation only; ignore boosts
  return side === "left" ? -cut : cut;
}

/** Magnitude (dB) of one biquad filter at `freq`. */
function magnitudeDb(f: CurveFilter, freq: number, fs: number): number {
  if (f.kind === "AllPass") return 0; // flat magnitude — phase only

  const w0 = (2 * Math.PI * f.freq) / fs;
  const cw = Math.cos(w0);
  const sw = Math.sin(w0);
  const q = (kindHasQ(f.kind) ? f.q ?? defaultQ(f.kind) : defaultQ(f.kind)) || defaultQ(f.kind);
  const alpha = sw / (2 * q);
  const A = Math.pow(10, (f.gain ?? 0) / 40);

  let b0 = 1;
  let b1 = 0;
  let b2 = 0;
  let a0 = 1;
  let a1 = 0;
  let a2 = 0;

  switch (f.kind) {
    case "Peak":
      b0 = 1 + alpha * A;
      b1 = -2 * cw;
      b2 = 1 - alpha * A;
      a0 = 1 + alpha / A;
      a1 = -2 * cw;
      a2 = 1 - alpha / A;
      break;
    case "LowShelf":
    case "LowShelfQ": {
      const s = 2 * Math.sqrt(A) * alpha;
      b0 = A * (A + 1 - (A - 1) * cw + s);
      b1 = 2 * A * (A - 1 - (A + 1) * cw);
      b2 = A * (A + 1 - (A - 1) * cw - s);
      a0 = A + 1 + (A - 1) * cw + s;
      a1 = -2 * (A - 1 + (A + 1) * cw);
      a2 = A + 1 + (A - 1) * cw - s;
      break;
    }
    case "HighShelf":
    case "HighShelfQ": {
      const s = 2 * Math.sqrt(A) * alpha;
      b0 = A * (A + 1 + (A - 1) * cw + s);
      b1 = -2 * A * (A - 1 + (A + 1) * cw);
      b2 = A * (A + 1 + (A - 1) * cw - s);
      a0 = A + 1 - (A - 1) * cw + s;
      a1 = 2 * (A - 1 - (A + 1) * cw);
      a2 = A + 1 - (A - 1) * cw - s;
      break;
    }
    case "LowPass":
    case "LowPassQ":
      b0 = (1 - cw) / 2;
      b1 = 1 - cw;
      b2 = (1 - cw) / 2;
      a0 = 1 + alpha;
      a1 = -2 * cw;
      a2 = 1 - alpha;
      break;
    case "HighPass":
    case "HighPassQ":
      b0 = (1 + cw) / 2;
      b1 = -(1 + cw);
      b2 = (1 + cw) / 2;
      a0 = 1 + alpha;
      a1 = -2 * cw;
      a2 = 1 - alpha;
      break;
    case "BandPass":
      b0 = alpha;
      b1 = 0;
      b2 = -alpha;
      a0 = 1 + alpha;
      a1 = -2 * cw;
      a2 = 1 - alpha;
      break;
    case "Notch":
      b0 = 1;
      b1 = -2 * cw;
      b2 = 1;
      a0 = 1 + alpha;
      a1 = -2 * cw;
      a2 = 1 - alpha;
      break;
    default:
      return 0;
  }

  // |H(e^jw)| = |numerator| / |denominator| (the a0 stays in the denominator).
  const w = (2 * Math.PI * freq) / fs;
  const c1 = Math.cos(-w);
  const s1 = Math.sin(-w);
  const c2 = Math.cos(-2 * w);
  const s2 = Math.sin(-2 * w);
  const nr = b0 + b1 * c1 + b2 * c2;
  const ni = b1 * s1 + b2 * s2;
  const dr = a0 + a1 * c1 + a2 * c2;
  const di = a1 * s1 + a2 * s2;
  const den = Math.hypot(dr, di);
  if (den === 0) return 0;
  return 20 * Math.log10(Math.hypot(nr, ni) / den);
}

/** Combined response (dB) across `freqs`: preamp + sum of enabled filters. */
export function responseCurve(
  filters: CurveFilter[],
  preamp: number,
  freqs: number[] = FREQS,
  fs: number = SAMPLE_RATE,
): number[] {
  return freqs.map((freq) => {
    let db = preamp;
    for (const f of filters) {
      if (f.enabled) db += magnitudeDb(f, freq, fs);
    }
    return db;
  });
}

// Tone-overlay filter shapes — must mirror the Rust source of truth in
// crates/fastpeq-core/src/tone.rs (bass low-shelf, mid peak, treble high-shelf).
const TONE_SHAPE = [
  { kind: "LowShelfQ" as FilterKind, freq: 105, q: 0.71 },
  { kind: "Peak" as FilterKind, freq: 1000, q: 0.7 },
  { kind: "HighShelfQ" as FilterKind, freq: 4000, q: 0.71 },
] as const;

/** The global tone knobs (bass/mid/treble dB) as response-curve filters. */
export function toneFilters(bass: number, mid: number, treble: number): CurveFilter[] {
  const gains = [bass, mid, treble];
  return TONE_SHAPE.map((s, i) => ({
    enabled: gains[i] !== 0,
    kind: s.kind,
    freq: s.freq,
    gain: gains[i],
    q: s.q,
    channel: { kind: "both" } as Channel,
  }));
}

/**
 * Peak combined gain (dB) over the audible band, taken as the louder of the two
 * channels. Above 0 dB the summed boost can exceed full scale, so Equalizer APO
 * may clip unless the preamp pulls it back down. Balance only ever cuts a
 * channel, so it can lower this peak but never raise it.
 */
export function peakGainDb(
  filters: CurveFilter[],
  preamp: number,
  balance = 0,
  freqs: number[] = FREQS,
): number {
  const trim = balanceTrim(balance);
  const left = responseCurve(filters.filter((f) => inChannel(f.channel, "left")), preamp + trim.left, freqs);
  const right = responseCurve(filters.filter((f) => inChannel(f.channel, "right")), preamp + trim.right, freqs);
  return Math.max(...left, ...right);
}
