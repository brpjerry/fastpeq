import { describe, it, expect } from "vitest";
import { starterConfig } from "./starter";

describe("starterConfig", () => {
  it("starts with a -10 dB preamp followed by N peaking bands", () => {
    const cfg = starterConfig(10);
    expect(cfg.lines[0]).toEqual({
      kind: "Preamp",
      value: { gain: -10, channel: { kind: "both" } },
    });
    const filters = cfg.lines.flatMap((l) => (l.kind === "Filter" ? [l.value] : []));
    expect(filters).toHaveLength(10);
    expect(filters.every((f) => f.kind === "Peak" && f.gain === 0)).toBe(true);
  });

  it("spans Q 0.5 (bass) up to 6 (treble) across the audible range", () => {
    const filters = starterConfig(10).lines.flatMap((l) => (l.kind === "Filter" ? [l.value] : []));
    const last = filters[filters.length - 1];
    expect(filters[0].q).toBe(0.5);
    expect(last.q).toBe(6);
    expect(filters[0].freq).toBeLessThanOrEqual(25);
    expect(last.freq).toBeGreaterThanOrEqual(19000);
    // Indices are 1-based and contiguous.
    expect(filters.map((f) => f.index)).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
  });

  it("keeps every Q within the 0.5..6 range for all band counts", () => {
    for (const n of [10, 15, 20, 30]) {
      const filters = starterConfig(n).lines.flatMap((l) => (l.kind === "Filter" ? [l.value] : []));
      expect(filters).toHaveLength(n);
      expect(filters.every((f) => f.q! >= 0.5 && f.q! <= 6)).toBe(true);
    }
  });
});
