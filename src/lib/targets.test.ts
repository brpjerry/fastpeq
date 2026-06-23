// @vitest-environment happy-dom
import { describe, it, expect, beforeEach } from "vitest";
import { getTargets, getTarget, addTarget, removeTarget, FLAT_TARGET } from "./targets.svelte";

beforeEach(() => localStorage.clear());

describe("targets store", () => {
  it("always offers the built-in Flat target first", () => {
    expect(getTargets()[0]).toEqual(FLAT_TARGET);
    expect(getTarget("flat")).toEqual(FLAT_TARGET);
  });

  it("adds and looks up a target", () => {
    const id = addTarget("Harman", [{ freq: 100, spl: 1 }]);
    expect(getTarget(id).name).toBe("Harman");
    expect(getTargets().some((t) => t.id === id)).toBe(true);
    removeTarget(id);
  });

  it("removes a user target but never Flat", () => {
    const id = addTarget("Temp", [{ freq: 50, spl: 0 }]);
    removeTarget(id);
    expect(getTargets().some((t) => t.id === id)).toBe(false);
    removeTarget("flat");
    expect(getTargets()[0]).toEqual(FLAT_TARGET); // still present
  });

  it("falls back to Flat for unknown ids", () => {
    expect(getTarget("nope")).toEqual(FLAT_TARGET);
  });
});
