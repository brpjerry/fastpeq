// @vitest-environment happy-dom
import { describe, it, expect, beforeEach } from "vitest";
import {
  getTargetId,
  setTargetId,
  getCompensate,
  setCompensate,
  getMeasurement,
  setMeasurement,
  clearMeasurement,
} from "./presetView.svelte";

beforeEach(() => localStorage.clear());

describe("presetView store", () => {
  it("defaults targetId to flat and remembers it per preset", () => {
    expect(getTargetId("A")).toBe("flat");
    setTargetId("A", "x1");
    expect(getTargetId("A")).toBe("x1");
    expect(getTargetId("B")).toBe("flat"); // independent per preset
  });

  it("defaults compensate to off and toggles per preset", () => {
    expect(getCompensate("C")).toBe(false);
    setCompensate("C", true);
    expect(getCompensate("C")).toBe(true);
    expect(getCompensate("D")).toBe(false);
  });

  it("stores and clears a per-preset measurement", () => {
    expect(getMeasurement("E")).toBeNull();
    setMeasurement("E", { name: "m.txt", points: [{ freq: 100, spl: 1 }] });
    expect(getMeasurement("E")?.name).toBe("m.txt");
    clearMeasurement("E");
    expect(getMeasurement("E")).toBeNull();
  });
});
