// Biquad magnitude-response math (RBJ Audio EQ Cookbook) for the live curve,
// plus metadata about each filter type. Computed on the client so dragging a
// slider redraws instantly with no IPC round-trip.
import type { Channel, Config, FilterKind } from "./types";

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
const SHELF_KINDS = new Set<FilterKind>(["LowShelf", "HighShelf", "LowShelfQ", "HighShelfQ"]);
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
/** The types that always carry a Q (the editor shows and seeds one). The others
 *  — LS/HS/LP/HP — may still be given one, which Equalizer APO honours. */
export const kindHasQ = (k: FilterKind) => Q_KINDS.has(k);
/** The Q seeded for a type when it has none: 1 for a peak, otherwise 1/√2 —
 *  mirrors `FilterKind::default_q` in crates/fastpeq-core/src/apo/model.rs. */
export const defaultQ = (k: FilterKind) => (k === "Peak" ? 1 : Math.SQRT1_2);
/** The types that can take a slope in dB/octave in place of their Q. */
export const kindHasSlope = (k: FilterKind) => k === "LowShelfQ" || k === "HighShelfQ";

/** The cookbook shelf slope S Equalizer APO gives a shelf with neither a Q nor
 *  a slope: 0.9, "found out by experimentation with RoomEQWizard"
 *  (BiQuadFilterFactory.cpp) — mirrors `DEFAULT_SHELF_S` in model.rs. */
export const DEFAULT_SHELF_S = 0.9;

/**
 * The Q equivalent to the cookbook shelf slope `s` at `gain` dB, or null when
 * there is none (a slope too steep for the gain, or not positive). From the
 * cookbook's alpha = sin(w0)/2 · sqrt((A + 1/A)(1/S − 1) + 2) = sin(w0)/(2Q) —
 * mirrors `shelf_s_to_q` in crates/fastpeq-core/src/apo/model.rs.
 */
export function shelfSToQ(s: number, gain: number): number | null {
  if (!(s > 0)) return null;
  const A = Math.pow(10, gain / 40);
  const radicand = (A + 1 / A) * (1 / s - 1) + 2;
  return radicand > 0 ? 1 / Math.sqrt(radicand) : null;
}

/**
 * The Q equivalent to a shelf slope of `slope` dB/octave at `gain` dB, or null
 * when there is none. Equalizer APO reads `LSC 12 dB` as the cookbook's shelf
 * slope S = slope / 12 — mirrors `slope_to_q` in model.rs.
 */
export function slopeToQ(slope: number, gain: number): number | null {
  return shelfSToQ(slope / 12, gain);
}

/** The cookbook shelf slope equivalent to `q` at `gain` dB — the inverse of
 *  [shelfSToQ], as APO's BiQuadFilter derives it for a corner-frequency shelf
 *  given a Q. Mirrors `shelf_q_to_s` in model.rs. */
export function shelfQToS(q: number, gain: number): number {
  const A = Math.pow(10, gain / 40);
  return 1 / ((1 / (q * q) - 2) / (A + 1 / A) + 1);
}

/**
 * The centre frequency and Q of the biquad Equalizer APO runs for a filter —
 * for a shelf not always the line's Fc and Q. As APO's BiQuadFilterFactory and
 * BiQuadFilter resolve them:
 * - a slope of N dB is the cookbook shelf slope S = N / 12 ([slopeToQ]);
 * - with neither a Q nor a slope (or either written as 0), S = [DEFAULT_SHELF_S];
 * - an LS/HS given a Q or a slope reads Fc as a *corner* frequency and centres
 *   the biquad at Fc · 10^(|gain| / (80 S)) (LS) or Fc / 10^(…) (HS), deriving
 *   S from the Q when that's what it has. LSC/HSC, and an LS/HS with neither,
 *   centre on Fc itself.
 * Any other type keeps its Fc, and its Q or else [defaultQ]. Null for a slope
 * with no real equivalent Q, which is no usable filter. Mirrors
 * `Filter::apo_biquad` in model.rs; the two are pinned to the same numbers.
 */
export function apoBiquad(f: CurveFilter): { freq: number; q: number } | null {
  // APO takes a Q or slope of 0 as none given.
  const given = f.q || null;
  if (!SHELF_KINDS.has(f.kind)) return { freq: f.freq, q: given ?? defaultQ(f.kind) };
  const gain = f.gain ?? 0;
  const slope = (kindHasSlope(f.kind) && f.slope) || null;
  let s: number;
  let q: number | null;
  if (slope !== null) {
    s = slope / 12;
    q = slopeToQ(slope, gain);
  } else if (given !== null) {
    s = shelfQToS(given, gain);
    q = given;
  } else {
    q = shelfSToQ(DEFAULT_SHELF_S, gain);
    return q === null ? null : { freq: f.freq, q };
  }
  if (q === null) return null;
  // APO's frequency adjustment "for DCX2496" (a Behringer crossover).
  const shift = Math.pow(10, Math.abs(gain) / 80 / s);
  if (f.kind === "LowShelf") return { freq: f.freq * shift, q };
  if (f.kind === "HighShelf") return { freq: f.freq / shift, q };
  return { freq: f.freq, q };
}

// How far a single filter's gain may be pushed, in dB either way. The default
// keeps a band inside the range most presets (and most hardware EQ engines)
// stay in; Settings can widen or narrow it. The limit is an *editing* rule, not
// a storage one: a preset loaded with a bigger gain keeps it (see BandRow), and
// only an edit of that gain pulls it inside the limit.
export const DEFAULT_GAIN_LIMIT = 12;
export const GAIN_LIMIT_MIN = 1;
/** The curve editor's vertical range — a limit past it couldn't be dragged. */
export const GAIN_LIMIT_MAX = 30;

/** A filter gain pulled inside ±`limit` dB. */
export const clampGainTo = (gain: number, limit: number) =>
  Math.max(-limit, Math.min(limit, gain));

/** The band-list channel views. */
export type BandView = "both" | "left" | "right";

/** An additional display filter over a band list while a hybrid offload mode is
 * on: only the bands running in Equalizer APO, only those on the device, or all. */
export type EngineFilter = "all" | "apo" | "hw";

/** Whether a band belongs to a band-list view under an engine filter.
 * `offloaded` is whether the band currently runs on the hardware device —
 * callers pass the backend's selection verbatim (the UI never derives
 * membership itself: today it's mode-driven, but it may become user-assigned,
 * non-contiguous, or otherwise arbitrary). */
export function bandInView(
  c: Channel,
  offloaded: boolean,
  v: BandView,
  engine: EngineFilter = "all",
): boolean {
  if (engine === "apo" && offloaded) return false;
  if (engine === "hw" && !offloaded) return false;
  if (v === "left") return c.kind === "left";
  if (v === "right") return c.kind === "right";
  return c.kind === "both" || c.kind === "other"; // unmodeled specs ride along
}

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
  /** Null for none. On an LS/HS a Q makes it a corner-frequency shelf (see
   *  [apoBiquad]), so a plain one must leave it null. */
  q: number | null;
  /** A shelf's slope in dB/octave; when set it stands in for `q`. */
  slope?: number | null;
  channel: Channel;
}

/** An editable band as the editor, its graphs, and the undo history hold it:
 *  a [CurveFilter] whose gain is always a plain number (defaulted on load,
 *  nulled back out per type on save) plus a stable id for keyed lists, drag
 *  targets, and history snapshots. The one definition — Editor, CurveEditor,
 *  and history all alias this. */
export type EditorBand = {
  id: number;
  enabled: boolean;
  kind: FilterKind;
  freq: number;
  gain: number;
  /** Always set on the Q types (seeded on load — see [parseConfigEq]). The
   *  other types (LS/HS/LP/HP) keep only a Q their line came with, which
   *  Equalizer APO honours; the editor never adds one there, and it saves
   *  whatever is set. */
  q: number | null;
  /** Set only on a shelf given as a slope (see [kindHasSlope]); the editor then
   *  edits it in place of `q`, which stays at its default underneath. */
  slope: number | null;
  channel: Channel;
};

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

/** A Config split into the editor's shape (see [parseConfigEq]). */
export interface ParsedConfigEq {
  /** The `Filter:` lines, gain defaulted and q seeded as an [EditorBand]'s. */
  filters: (CurveFilter & { gain: number; slope: number | null })[];
  /** The master (both-channel) preamp gain, 0 when absent. */
  preamp: number;
  /** Balance recovered from a one-sided preamp trim (see balanceFromTrim). */
  balance: number;
  /** Whether the config carried an explicit master preamp line. */
  hadPreamp: boolean;
  /** Unmodeled lines (comments, Device:, …), preserved verbatim in order. */
  raw: string[];
}

/** The Q a Q type is seeded with when its line has none: for a shelf the one
 *  Equalizer APO runs (S = [DEFAULT_SHELF_S] at its gain), so writing it out
 *  leaves the sound as it was; otherwise [defaultQ]. */
function missingQ(kind: FilterKind, gain: number): number {
  const shelf = SHELF_KINDS.has(kind) ? shelfSToQ(DEFAULT_SHELF_S, gain) : null;
  return shelf ?? defaultQ(kind);
}

/**
 * Split a Config's lines into the parts the editor works with: master preamp,
 * balance, graph-ready filters, and the raw lines carried along untouched.
 * Shared by the editor's load and its A/B-compare ghost, so the two can never
 * drift in how they read a preset.
 */
export function parseConfigEq(cfg: Config): ParsedConfigEq {
  const out: ParsedConfigEq = { filters: [], preamp: 0, balance: 0, hadPreamp: false, raw: [] };
  for (const line of cfg.lines) {
    if (line.kind === "Preamp") {
      // A both-channel preamp is the master gain; a one-sided preamp is a
      // balance trim, folded back into the balance slider.
      const ch = line.value.channel;
      if (ch.kind === "left" || ch.kind === "right") {
        out.balance = balanceFromTrim(ch.kind, line.value.gain);
      } else {
        out.preamp = line.value.gain;
        out.hadPreamp = true;
      }
    } else if (line.kind === "Filter") {
      const f = line.value;
      out.filters.push({
        enabled: f.enabled,
        kind: f.kind,
        freq: f.freq,
        gain: f.gain ?? 0,
        q: f.q ?? (kindHasQ(f.kind) ? missingQ(f.kind, f.gain ?? 0) : null),
        slope: f.slope ?? null,
        channel: f.channel,
      });
    } else {
      out.raw.push(line.value);
    }
  }
  return out;
}

/** Biquad coefficients for one filter (`a0` kept un-normalized; the evaluation
 *  divides it out), computed once per filter — they don't depend on the probe
 *  frequency. `null` for types with a flat magnitude (AllPass / unmodeled),
 *  which contribute nothing to the response. */
interface BiquadCoeffs {
  b0: number;
  b1: number;
  b2: number;
  a0: number;
  a1: number;
  a2: number;
}

function biquadCoeffs(f: CurveFilter, fs: number): BiquadCoeffs | null {
  if (f.kind === "AllPass") return null; // flat magnitude — phase only

  const biquad = apoBiquad(f);
  if (biquad === null) return null; // no real filter for this slope and gain
  const w0 = (2 * Math.PI * biquad.freq) / fs;
  const cw = Math.cos(w0);
  const sw = Math.sin(w0);
  const alpha = sw / (2 * biquad.q);
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
      return null;
  }

  return { b0, b1, b2, a0, a1, a2 };
}

/** Combined response (dB) across `freqs`: preamp + sum of enabled filters.
 *  Coefficients are computed once per filter, and the per-frequency trig once
 *  per point (shared by every filter) — this runs per drag frame over up to
 *  ~1600 points, so no per-(filter, freq) transcendentals. */
export function responseCurve(
  filters: CurveFilter[],
  preamp: number,
  freqs: number[] = FREQS,
  fs: number = SAMPLE_RATE,
): number[] {
  const coeffs: BiquadCoeffs[] = [];
  for (const f of filters) {
    if (!f.enabled) continue;
    const c = biquadCoeffs(f, fs);
    if (c) coeffs.push(c);
  }
  return freqs.map((freq) => {
    const w = (2 * Math.PI * freq) / fs;
    const c1 = Math.cos(-w);
    const s1 = Math.sin(-w);
    const c2 = Math.cos(-2 * w);
    const s2 = Math.sin(-2 * w);
    let db = preamp;
    for (const c of coeffs) {
      // |H(e^jw)| = |numerator| / |denominator| (the a0 stays in the denominator).
      const nr = c.b0 + c.b1 * c1 + c.b2 * c2;
      const ni = c.b1 * s1 + c.b2 * s2;
      const dr = c.a0 + c.a1 * c1 + c.a2 * c2;
      const di = c.a1 * s1 + c.a2 * s2;
      const den = Math.hypot(dr, di);
      if (den !== 0) db += 20 * Math.log10(Math.hypot(nr, ni) / den);
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

// IEC 61672 A-weighting, normalised so A(1 kHz) = 0 dB exactly.
function aWeightRaw(f: number): number {
  const f2 = f * f;
  const ra =
    (12194 ** 2 * f2 * f2) /
    ((f2 + 20.6 ** 2) * Math.sqrt((f2 + 107.7 ** 2) * (f2 + 737.9 ** 2)) * (f2 + 12194 ** 2));
  return 20 * Math.log10(ra);
}
const A_WEIGHT_1K = aWeightRaw(1000);

/** A-weighting (dB) at `f` — how much quieter a tone there *sounds* than its
 *  level suggests (≈ −19.1 dB at 100 Hz, 0 at 1 kHz, ≈ −2.5 dB at 10 kHz). */
export function aWeightDb(f: number): number {
  return aWeightRaw(f) - A_WEIGHT_1K;
}

/**
 * Perceived level (dB) of a response: the A-weighted power mean over the probe
 * grid — the loudness proxy compare-mode volume matching uses. Only the
 * *difference* between two values is meaningful (a flat response reads
 * slightly negative because A-weighting attenuates the spectrum edges); a
 * preamp shift moves it 1:1.
 */
export function loudnessDb(
  filters: CurveFilter[],
  preamp: number,
  freqs: number[] = FREQS,
): number {
  const resp = responseCurve(filters, preamp, freqs);
  let sum = 0;
  for (let i = 0; i < resp.length; i++) {
    sum += Math.pow(10, (resp[i] + aWeightDb(freqs[i])) / 10);
  }
  return 10 * Math.log10(sum / resp.length);
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
