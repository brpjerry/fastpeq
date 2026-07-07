import { describe, it, expect } from "vitest";
import { timeAgo } from "./time";

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
const NOW = 1_783_400_000_000;

describe("timeAgo", () => {
  it("labels each coarse bucket", () => {
    expect(timeAgo(NOW - 5_000, NOW)).toBe("just now");
    expect(timeAgo(NOW - 5 * MIN, NOW)).toBe("5 min ago");
    expect(timeAgo(NOW - 3 * HOUR, NOW)).toBe("3 h ago");
    expect(timeAgo(NOW - 1 * DAY, NOW)).toBe("yesterday");
    expect(timeAgo(NOW - 6 * DAY, NOW)).toBe("6 days ago");
  });

  it("falls back to a date past a month, and never reads the future", () => {
    expect(timeAgo(NOW - 45 * DAY, NOW)).toMatch(/\d/); // locale date
    expect(timeAgo(NOW + 10 * MIN, NOW)).toBe("just now"); // clock skew clamps
  });
});
