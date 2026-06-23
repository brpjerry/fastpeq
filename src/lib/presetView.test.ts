// @vitest-environment happy-dom
import { describe, it, expect, beforeEach } from "vitest";
import {
  getTargetId,
  setTargetId,
  getCompensate,
  setCompensate,
  getShowMeasRef,
  setShowMeasRef,
  getShowTargetRef,
  setShowTargetRef,
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

  it("defaults both reference toggles on and tracks them independently per preset", () => {
    expect(getShowMeasRef("F")).toBe(true);
    expect(getShowTargetRef("F")).toBe(true);
    setShowMeasRef("F", false);
    expect(getShowMeasRef("F")).toBe(false);
    expect(getShowTargetRef("F")).toBe(true); // independent of the measurement ref
    setShowTargetRef("F", false);
    expect(getShowTargetRef("F")).toBe(false);
    expect(getShowMeasRef("G")).toBe(true); // independent per preset
  });

  it("stores and clears a per-preset measurement", () => {
    expect(getMeasurement("E")).toBeNull();
    setMeasurement("E", { name: "m.txt", points: [{ freq: 100, spl: 1 }] });
    expect(getMeasurement("E")?.name).toBe("m.txt");
    clearMeasurement("E");
    expect(getMeasurement("E")).toBeNull();
  });
});
