import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createDebounce, createTrailingThrottle } from "./throttle";

describe("createTrailingThrottle", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("fires immediately when the window is clear", () => {
    const fn = vi.fn();
    const t = createTrailingThrottle(fn, 75);
    t.schedule();
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it("coalesces a burst into one trailing call", () => {
    const fn = vi.fn();
    const t = createTrailingThrottle(fn, 75);
    t.schedule(); // leading call
    t.schedule();
    t.schedule();
    expect(fn).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(75);
    expect(fn).toHaveBeenCalledTimes(2); // exactly one trailing call
    vi.advanceTimersByTime(200);
    expect(fn).toHaveBeenCalledTimes(2); // nothing left pending
  });

  it("respects the window across separate schedules", () => {
    const fn = vi.fn();
    const t = createTrailingThrottle(fn, 75);
    t.schedule(); // fires at t=0
    vi.advanceTimersByTime(50);
    t.schedule(); // inside the window -> trails at t=75
    expect(fn).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(25);
    expect(fn).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(100);
    t.schedule(); // window long clear -> immediate again
    expect(fn).toHaveBeenCalledTimes(3);
  });

  it("cancel drops a pending trailing call", () => {
    const fn = vi.fn();
    const t = createTrailingThrottle(fn, 75);
    t.schedule();
    t.schedule(); // pending trail
    t.cancel();
    vi.advanceTimersByTime(500);
    expect(fn).toHaveBeenCalledTimes(1); // only the leading call ran
  });

  it("flush runs immediately, drops the pending call, and resets the window", () => {
    const fn = vi.fn();
    const t = createTrailingThrottle(fn, 75);
    t.schedule();
    t.schedule(); // pending trail
    t.flush();
    expect(fn).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(500);
    expect(fn).toHaveBeenCalledTimes(2); // pending call was absorbed
    t.schedule(); // window reset by flush at its call time
    expect(fn).toHaveBeenCalledTimes(3);
  });
});

describe("createDebounce", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("has no leading call and fires once the burst settles", () => {
    const fn = vi.fn();
    const d = createDebounce(fn, 300);
    d.schedule();
    expect(fn).not.toHaveBeenCalled();
    d.schedule();
    d.schedule();
    vi.advanceTimersByTime(299);
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(fn).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(500);
    expect(fn).toHaveBeenCalledTimes(1); // nothing left pending
  });

  it("each schedule restarts the timer", () => {
    const fn = vi.fn();
    const d = createDebounce(fn, 300);
    d.schedule();
    vi.advanceTimersByTime(200);
    d.schedule(); // restart at t=200
    vi.advanceTimersByTime(200);
    expect(fn).not.toHaveBeenCalled(); // t=400 < 200+300
    vi.advanceTimersByTime(100);
    expect(fn).toHaveBeenCalledTimes(1); // t=500
  });

  it("cancel drops the pending run", () => {
    const fn = vi.fn();
    const d = createDebounce(fn, 300);
    d.schedule();
    d.cancel();
    vi.advanceTimersByTime(1000);
    expect(fn).not.toHaveBeenCalled();
  });
});
