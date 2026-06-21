import { describe, it, expect } from "vitest";
import { freqToX, xToFreq, dbToY, yToGain, dbGridLines, F_MIN, F_MAX, type PlotBox } from "./graph";

const box: PlotBox = { padL: 40, padT: 10, plotW: 600, plotH: 200 };

describe("frequency axis", () => {
  it("maps F_MIN/F_MAX to the plot edges", () => {
    expect(freqToX(F_MIN, box)).toBeCloseTo(box.padL, 6);
    expect(freqToX(F_MAX, box)).toBeCloseTo(box.padL + box.plotW, 6);
  });
  it("round-trips through xToFreq", () => {
    expect(xToFreq(freqToX(1000, box), box)).toBeCloseTo(1000, 6);
  });
});

describe("dB axis (preamp-centred)", () => {
  it("puts the preamp on the centre line", () => {
    expect(dbToY(-10, -10, 30, box)).toBeCloseTo(box.padT + box.plotH / 2, 6);
  });
  it("yToGain inverts dbToY for a handle at preamp + gain", () => {
    const y = dbToY(-10 + 4, -10, 30, box); // preamp -10, gain +4
    expect(yToGain(y, 30, box)).toBeCloseTo(4, 6);
  });
});

describe("dbGridLines", () => {
  it("returns step multiples spanning the visible range", () => {
    expect(dbGridLines(0, 30, 10)).toEqual([-30, -20, -10, 0, 10, 20, 30]);
  });
  it("recentres around a non-zero preamp", () => {
    expect(dbGridLines(-10, 30, 10)).toEqual([-40, -30, -20, -10, 0, 10, 20]);
  });
});
