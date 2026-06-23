import { describe, it, expect } from "vitest";
import { parseRew, normalize, sampleAt, downsample, type MeasPoint } from "./measurement";

describe("parseRew", () => {
  it("parses freq/SPL rows, skipping comments and headers, sorted by freq", () => {
    const text = "* REW measurement\nFreq(Hz) SPL(dB)\n1000 0.0\n20 3.5\n100, -2.0\n";
    expect(parseRew(text)).toEqual([
      { freq: 20, spl: 3.5 },
      { freq: 100, spl: -2 },
      { freq: 1000, spl: 0 },
    ]);
  });
});

describe("normalize", () => {
  it("shifts the 300-3000 Hz mean to 0 dB", () => {
    const out = normalize([
      { freq: 1000, spl: 5 },
      { freq: 2000, spl: 7 },
    ]);
    const mean = out.reduce((s, p) => s + p.spl, 0) / out.length;
    expect(mean).toBeCloseTo(0, 10);
  });
  it("returns empty input unchanged", () => expect(normalize([])).toEqual([]));
});

describe("sampleAt", () => {
  const pts = [
    { freq: 100, spl: 0 },
    { freq: 1000, spl: 10 },
  ];
  it("clamps below and above the measured range", () => {
    expect(sampleAt(pts, [50])[0]).toBe(0);
    expect(sampleAt(pts, [2000])[0]).toBe(10);
  });
  it("interpolates linearly in log frequency", () => {
    // The geometric mean (~316 Hz) is the log-midpoint, so half the SPL span.
    expect(sampleAt(pts, [Math.sqrt(100 * 1000)])[0]).toBeCloseTo(5, 6);
  });
  it("returns 0 for an empty measurement", () => expect(sampleAt([], [1000])[0]).toBe(0));
});

describe("downsample", () => {
  it("leaves a small measurement untouched", () => {
    const small: MeasPoint[] = [
      { freq: 100, spl: 1 },
      { freq: 1000, spl: 2 },
    ];
    expect(downsample(small, 256)).toBe(small);
  });

  it("caps a large measurement at n points, preserving the range and shape", () => {
    // 5000 dense points from 20 Hz to 20 kHz, SPL = log-freq ramp 0..40.
    const dense: MeasPoint[] = Array.from({ length: 5000 }, (_, i) => {
      const f = 20 * Math.pow(1000, i / 4999);
      return { freq: f, spl: (Math.log10(f / 20) / 3) * 40 };
    });
    const ds = downsample(dense, 256);
    expect(ds.length).toBe(256);
    expect(ds[0].freq).toBeCloseTo(20, 1);
    expect(ds[ds.length - 1].freq).toBeCloseTo(20000, 0);
    // The resampled curve still tracks the original ramp at the midband.
    expect(sampleAt(ds, [1000])[0]).toBeCloseTo((Math.log10(1000 / 20) / 3) * 40, 1);
  });
});
