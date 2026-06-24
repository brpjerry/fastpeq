import { describe, it, expect } from "vitest";
import { valueAt, frValueAt, targetValueAt, gapDb, compensateCurve, alignOffset } from "./curve";
import type { CurveFilter } from "./eq";
import type { MeasPoint } from "./measurement";

const pts: MeasPoint[] = [
  { freq: 100, spl: 2 },
  { freq: 1000, spl: 6 },
];

describe("curve helpers", () => {
  it("valueAt interpolates, and is 0 for an empty curve", () => {
    expect(valueAt([], 500)).toBe(0);
    expect(valueAt(pts, 100)).toBe(2);
    expect(valueAt(pts, 1000)).toBe(6);
    // Linear in log-freq between 100 and 1000 → halfway (~316 Hz) ≈ 4 dB.
    expect(valueAt(pts, 316.23)).toBeCloseTo(4, 1);
  });

  it("frValueAt is preamp + filters + measurement", () => {
    // No filters: FR is just preamp + the measurement value.
    expect(frValueAt([], -5, pts, 100)).toBeCloseTo(-3, 5); // -5 + 2
    expect(frValueAt([], 0, [], 1000)).toBe(0); // nothing → flat at preamp 0
  });

  it("targetValueAt is preamp + target (flat target stays at preamp)", () => {
    expect(targetValueAt(pts, -5, 1000)).toBeCloseTo(1, 5); // -5 + 6
    expect(targetValueAt([], 3, 1000)).toBe(3); // flat → just preamp
  });

  it("gapDb is the absolute FR-to-target distance (preamp cancels)", () => {
    // FR = preamp + meas(=2 at 100); target = preamp + 0 (flat). Gap = 2.
    expect(gapDb([], -5, pts, [], -5, 100)).toBeCloseTo(2, 5);
    // Against an identical target: gap is 0 regardless of preamp.
    expect(gapDb([], -5, pts, pts, -5, 100)).toBeCloseTo(0, 5);
  });

  it("gapDb folds a filter's gain into the FR", () => {
    const peak: CurveFilter = {
      enabled: true,
      kind: "Peak",
      freq: 1000,
      gain: 4,
      q: 1,
      channel: { kind: "both" },
    };
    // At the peak center the filter adds ~+4 dB over a flat target.
    expect(gapDb([peak], 0, [], [], 0, 1000)).toBeCloseTo(4, 0);
  });

  it("compensateCurve subtracts the target pointwise", () => {
    expect(compensateCurve([5, 3, -2], [1, 3, -2])).toEqual([4, 0, 0]);
  });

  it("alignOffset shifts a target onto the FR at a frequency (preamp cancels)", () => {
    // Flat FR (no filters/meas) at preamp -10; target reads 6 at 1 kHz → -6 to meet it.
    expect(alignOffset([], -10, [], pts, 1000)).toBeCloseTo(-6, 5);
    // A +4 dB peak over a flat target → lift the flat target by +4 at the center.
    const peak: CurveFilter = {
      enabled: true,
      kind: "Peak",
      freq: 1000,
      gain: 4,
      q: 1,
      channel: { kind: "both" },
    };
    expect(alignOffset([peak], 0, [], [], 1000)).toBeCloseTo(4, 0);
    // A measurement raises the FR; the offset follows it (meas=2 at 100 Hz).
    expect(alignOffset([], 0, pts, [], 100)).toBeCloseTo(2, 5);
  });
});
