import { describe, it, expect } from "vitest";
import {
  balanceTrim,
  balanceFromTrim,
  kindHasGain,
  kindHasQ,
  defaultQ,
  responseCurve,
  peakGainDb,
  aWeightDb,
  loudnessDb,
  toneFilters,
  type CurveFilter,
} from "./eq";

const both = { kind: "both" } as const;
const peak = (freq: number, gain: number, q: number): CurveFilter => ({
  enabled: true,
  kind: "Peak",
  freq,
  gain,
  q,
  channel: both,
});

describe("balanceTrim", () => {
  it("centers at 0", () => expect(balanceTrim(0)).toEqual({ left: 0, right: 0 }));
  it("cuts the left when balance is positive (right louder)", () =>
    expect(balanceTrim(6)).toEqual({ left: -6, right: 0 }));
  it("cuts the right when balance is negative", () =>
    expect(balanceTrim(-6)).toEqual({ left: 0, right: -6 }));
});

describe("balanceFromTrim", () => {
  it("inverts balanceTrim", () => {
    expect(balanceFromTrim("left", -6)).toBe(6);
    expect(balanceFromTrim("right", -6)).toBe(-6);
  });
});

describe("filter metadata", () => {
  it("knows which kinds carry gain", () => {
    expect(kindHasGain("Peak")).toBe(true);
    expect(kindHasGain("LowShelfQ")).toBe(true);
    expect(kindHasGain("LowPass")).toBe(false);
  });
  it("knows which kinds carry Q", () => {
    expect(kindHasQ("Peak")).toBe(true);
    expect(kindHasQ("LowShelf")).toBe(false);
  });
  it("defaults Q per kind", () => {
    expect(defaultQ("Peak")).toBe(1);
    expect(defaultQ("LowShelfQ")).toBeCloseTo(Math.SQRT1_2, 6);
  });
});

describe("responseCurve", () => {
  it("is flat at the preamp with no filters", () => {
    expect(responseCurve([], -10, [100, 1000, 10000])).toEqual([-10, -10, -10]);
  });
  it("equals the filter's gain at its center frequency", () => {
    // A peaking filter's magnitude at its center frequency is exactly its gain.
    expect(responseCurve([peak(1000, 6, 1)], 0, [1000])[0]).toBeCloseTo(6, 5);
  });
  it("returns to baseline far from the peak", () => {
    expect(responseCurve([peak(1000, 6, 3)], 0, [20])[0]).toBeCloseTo(0, 1);
  });
});

describe("peakGainDb", () => {
  it("reflects the boost over the preamp", () => {
    expect(peakGainDb([peak(1000, 6, 1)], 0, 0, [1000])).toBeCloseTo(6, 5);
    expect(peakGainDb([peak(1000, 6, 1)], -10, 0, [1000])).toBeCloseTo(-4, 5);
  });
  it("only lets balance lower the peak (it cuts one channel)", () => {
    // The boost is on both channels; balance attenuates the left, so the louder
    // (right) channel is unchanged and still defines the peak.
    expect(peakGainDb([peak(1000, 6, 1)], 0, 8, [1000])).toBeCloseTo(6, 5);
  });
});

describe("toneFilters", () => {
  it("maps knobs to shelf/peak filters, disabling 0 dB knobs", () => {
    const [bass, mid, treble] = toneFilters(4, 0, -2);
    expect(bass).toMatchObject({ kind: "LowShelfQ", freq: 105, gain: 4, enabled: true });
    expect(mid).toMatchObject({ kind: "Peak", freq: 1000, gain: 0, enabled: false });
    expect(treble).toMatchObject({ kind: "HighShelfQ", freq: 4000, gain: -2, enabled: true });
  });
});

describe("aWeightDb / loudnessDb", () => {
  it("matches the IEC 61672 reference points", () => {
    expect(aWeightDb(1000)).toBeCloseTo(0, 6); // normalised exactly at 1 kHz
    expect(aWeightDb(100)).toBeCloseTo(-19.1, 1);
    expect(aWeightDb(10000)).toBeCloseTo(-2.5, 1);
  });

  it("tracks a preamp shift 1:1", () => {
    expect(loudnessDb([], 0) - loudnessDb([], -3)).toBeCloseTo(3, 6);
  });

  it("hears a mid boost as louder than an equal bass boost", () => {
    const flat = loudnessDb([], 0);
    const bassGain = loudnessDb([peak(60, 6, 1)], 0) - flat;
    const midGain = loudnessDb([peak(3000, 6, 1)], 0) - flat;
    expect(midGain).toBeGreaterThan(bassGain); // A-weighting discounts the bass
    expect(bassGain).toBeGreaterThanOrEqual(0);
  });
});
