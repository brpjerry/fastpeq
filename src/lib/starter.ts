// Builds the starting config for a new preset: a -10 dB preamp for headroom,
// then N peaking bands at 0 dB, spaced logarithmically from 20 Hz to 20 kHz,
// with Q stepping up across equal-width
// plateaus — wide (Q 0.5) in the bass, progressively narrower up to Q 6 at the
// top. The bands are split into equal groups, so every plateau covers the same
// share of the range regardless of band count; the step size adapts so the top
// plateau always lands on Q 6.
import type { Config, Line } from "./types";
import { loadNumber, save } from "./storage";

export const BAND_COUNTS = [10, 15, 20, 30];

const F_LO = 20;
const F_HI = 20000;
const PREAMP_DB = -10; // headroom so boosted bands don't clip
const Q_LO = 0.5; // widest band (bass)
const Q_HI = 6; // narrowest band (top of treble) — every preset reaches this
const Q_PLATEAU = 3; // target bands per Q step; the plateau count scales with N

function roundNice(f: number): number {
  if (f < 100) return Math.round(f / 5) * 5;
  if (f < 1000) return Math.round(f / 10) * 10;
  if (f < 10000) return Math.round(f / 100) * 100;
  return Math.round(f / 1000) * 1000;
}

export function starterConfig(n: number): Config {
  const lines: Line[] = [{ kind: "Preamp", value: { gain: PREAMP_DB, channel: { kind: "both" } } }];
  // Distribute the bands across `plateaus` equal groups; floor((i*plateaus)/n)
  // keeps group sizes within one of each other, so every Q value lands on an
  // equal share of the range. Q spans Q_LO..Q_HI evenly so the top plateau is
  // always Q_HI (one decimal, matching the editor's 0.1 Q step).
  const plateaus = Math.max(1, Math.round(n / Q_PLATEAU));
  for (let i = 0; i < n; i++) {
    const freq = roundNice(F_LO * Math.pow(F_HI / F_LO, i / (n - 1)));
    const group = Math.floor((i * plateaus) / n);
    const t = plateaus > 1 ? group / (plateaus - 1) : 0;
    const q = Math.round((Q_LO + (Q_HI - Q_LO) * t) * 10) / 10;
    lines.push({
      kind: "Filter",
      value: {
        enabled: true,
        kind: "Peak",
        freq,
        gain: 0,
        q,
        index: i + 1,
        channel: { kind: "both" },
      },
    });
  }
  return { lines };
}

const KEY = "fastpeq.bandCount";

export function defaultBandCount(): number {
  const v = loadNumber(KEY, 10);
  return BAND_COUNTS.includes(v) ? v : 10;
}

export function setDefaultBandCount(n: number): void {
  save(KEY, n);
}
