import { describe, it, expect } from "vitest";
import {
  balanceTrim,
  balanceFromTrim,
  kindHasGain,
  kindHasQ,
  kindHasSlope,
  defaultQ,
  slopeToQ,
  parseConfigEq,
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
  it("lets only the Q shelves take a slope", () => {
    expect(kindHasSlope("LowShelfQ")).toBe(true);
    expect(kindHasSlope("HighShelfQ")).toBe(true);
    expect(kindHasSlope("LowShelf")).toBe(false);
    expect(kindHasSlope("Peak")).toBe(false);
  });
});

describe("slopeToQ", () => {
  it("maps 12 dB/oct (S = 1) to Q 1/√2 at any gain", () => {
    for (const gain of [-12, 0, 6]) expect(slopeToQ(12, gain)).toBeCloseTo(Math.SQRT1_2, 12);
  });
  it("matches the Rust conversion", () => {
    // Pinned in crates/fastpeq-core/src/apo/model.rs as well.
    expect(slopeToQ(10, 6)).toBeCloseTo(0.642281, 6);
  });
  it("has no equivalent for a slope too steep for the gain, or a non-positive one", () => {
    expect(slopeToQ(60, 24)).toBeNull();
    expect(slopeToQ(0, 6)).toBeNull();
  });
});

describe("slope shelves", () => {
  const shelf = (over: Partial<CurveFilter>): CurveFilter => ({
    enabled: true,
    kind: "LowShelfQ",
    freq: 100,
    gain: 6,
    q: Math.SQRT1_2,
    channel: both,
    ...over,
  });
  const probe = [20, 50, 100, 200, 1000];

  it("draw the same curve as a Q shelf at the equivalent Q", () => {
    const bySlope = responseCurve([shelf({ slope: 6, q: 3 })], 0, probe); // slope wins over q
    const byQ = responseCurve([shelf({ q: slopeToQ(6, 6) })], 0, probe);
    bySlope.forEach((db, i) => expect(db).toBeCloseTo(byQ[i], 9));
  });
  it("ignore the slope on a type that can't take one", () => {
    const peakWithSlope = responseCurve([{ ...peak(1000, 6, 1), slope: 6 }], 0, probe);
    expect(peakWithSlope).toEqual(responseCurve([peak(1000, 6, 1)], 0, probe));
  });
  it("contribute nothing when the slope has no real filter", () => {
    expect(responseCurve([shelf({ slope: 600 })], 0, probe)).toEqual([0, 0, 0, 0, 0]);
  });
  it("carry their slope through parseConfigEq", () => {
    const filter = { enabled: true, kind: "HighShelfQ", freq: 8000, gain: -3, q: null, index: 1, channel: both } as const;
    const parsed = parseConfigEq({
      lines: [
        { kind: "Filter", value: { ...filter, slope: 9 } },
        { kind: "Filter", value: filter }, // no slope key at all
      ],
    });
    expect(parsed.filters.map((f) => f.slope)).toEqual([9, null]);
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
