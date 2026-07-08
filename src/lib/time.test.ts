import { describe, it, expect } from "vitest";
import { longDate, timeAgo } from "./time";

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

describe("longDate", () => {
  const at = (y: number, m: number, d: number) => new Date(y, m - 1, d, 12).getTime();

  it("formats as 'Month Nth, Year' with correct ordinals", () => {
    expect(longDate(at(2026, 7, 3))).toBe("July 3rd, 2026");
    expect(longDate(at(2026, 1, 1))).toBe("January 1st, 2026");
    expect(longDate(at(2026, 2, 2))).toBe("February 2nd, 2026");
    expect(longDate(at(2026, 3, 4))).toBe("March 4th, 2026");
    expect(longDate(at(2026, 8, 21))).toBe("August 21st, 2026");
  });

  it("keeps the teens on 'th'", () => {
    expect(longDate(at(2026, 5, 11))).toBe("May 11th, 2026");
    expect(longDate(at(2026, 5, 12))).toBe("May 12th, 2026");
    expect(longDate(at(2026, 5, 13))).toBe("May 13th, 2026");
  });
});
