// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, vi } from "vitest";
import * as api from "./api";
import {
  initTargets,
  getTargets,
  getTarget,
  addTarget,
  removeTarget,
  FLAT_TARGET,
} from "./targets.svelte";

vi.mock("./api", () => ({
  loadUiState: vi.fn(() => Promise.resolve(null)),
  saveUiState: vi.fn(() => Promise.resolve()),
}));

const KEY = "fastpeq.targets";

// Re-init from "no file, empty localStorage" so each test starts with just
// Flat (the module state would otherwise leak between tests).
beforeEach(async () => {
  localStorage.clear();
  vi.mocked(api.loadUiState).mockResolvedValue(null);
  await initTargets();
  vi.mocked(api.saveUiState).mockClear();
});

describe("targets persistence", () => {
  it("loads the list from the backend file when one exists", async () => {
    const stored = [{ id: "t1", name: "Harman", points: [{ freq: 100, spl: 1 }] }];
    vi.mocked(api.loadUiState).mockResolvedValue(JSON.stringify(stored));
    await initTargets();
    expect(getTarget("t1").name).toBe("Harman");
    // The file is authoritative — a plain load never writes anything back.
    expect(api.saveUiState).not.toHaveBeenCalled();
  });

  it("migrates a pre-file localStorage list into the file when none exists", async () => {
    const legacy = [{ id: "t2", name: "Diffuse", points: [] }];
    localStorage.setItem(KEY, JSON.stringify(legacy));
    await initTargets();
    expect(getTarget("t2").name).toBe("Diffuse");
    expect(api.saveUiState).toHaveBeenCalledWith("targets", JSON.stringify(legacy));
  });

  it("never overwrites an unreadable file", async () => {
    vi.mocked(api.loadUiState).mockResolvedValue("{corrupt");
    await initTargets();
    expect(getTargets()).toEqual([FLAT_TARGET]); // starts empty, but the file is left alone
    expect(api.saveUiState).not.toHaveBeenCalled();
  });

  it("persists every edit to the file and the localStorage backup", () => {
    const id = addTarget("Harman", [{ freq: 100, spl: 1 }]);
    expect(api.saveUiState).toHaveBeenCalledTimes(1);
    expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual([
      { id, name: "Harman", points: [{ freq: 100, spl: 1 }] },
    ]);
    removeTarget(id);
  });
});

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
