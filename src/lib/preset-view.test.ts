// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, vi } from "vitest";
import * as api from "./api";
import {
  initPresetView,
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
  renamePresetView,
  clearPresetView,
  getTargetOffset,
  setTargetOffset,
  getTargetAlignFreq,
  setTargetAlignFreq,
} from "./preset-view.svelte";

vi.mock("./api", () => ({
  loadUiState: vi.fn(() => Promise.resolve(null)),
  saveUiState: vi.fn(() => Promise.resolve()),
}));

const KEY = "fastpeq.presetView";

// Re-init from "no file, empty localStorage" so each test starts with an
// empty store (the module state would otherwise leak between tests).
beforeEach(async () => {
  localStorage.clear();
  vi.mocked(api.loadUiState).mockResolvedValue(null);
  await initPresetView();
  vi.mocked(api.saveUiState).mockClear();
});

describe("presetView persistence", () => {
  it("loads the blob from the backend file when one exists", async () => {
    vi.mocked(api.loadUiState).mockResolvedValue(JSON.stringify({ A: { targetId: "t9" } }));
    await initPresetView();
    expect(getTargetId("A")).toBe("t9");
    // The file is authoritative — a plain load never writes anything back.
    expect(api.saveUiState).not.toHaveBeenCalled();
  });

  it("migrates a pre-file localStorage blob into the file when none exists", async () => {
    localStorage.setItem(KEY, JSON.stringify({ B: { compensate: true } }));
    await initPresetView();
    expect(getCompensate("B")).toBe(true);
    expect(api.saveUiState).toHaveBeenCalledWith(
      "preset-view",
      JSON.stringify({ B: { compensate: true } }),
    );
  });

  it("never overwrites an unreadable file", async () => {
    vi.mocked(api.loadUiState).mockResolvedValue("{corrupt");
    await initPresetView();
    expect(getTargetId("A")).toBe("flat"); // starts empty, but the file is left alone
    expect(api.saveUiState).not.toHaveBeenCalled();
  });

  it("treats a wrong-shape document as unreadable", async () => {
    vi.mocked(api.loadUiState).mockResolvedValue("[1,2]");
    await initPresetView();
    expect(getTargetId("A")).toBe("flat");
    expect(api.saveUiState).not.toHaveBeenCalled();
  });

  it("persists every edit to the file and the localStorage backup", () => {
    setTargetId("C", "t1");
    expect(api.saveUiState).toHaveBeenCalledTimes(1);
    expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual({ C: { targetId: "t1" } });
  });
});

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

  it("defaults the target offset to 0 dB and align freq to 1 kHz, per preset", () => {
    expect(getTargetOffset("H")).toBe(0);
    expect(getTargetAlignFreq("H")).toBe(1000);
    setTargetOffset("H", -3.5);
    setTargetAlignFreq("H", 500);
    expect(getTargetOffset("H")).toBe(-3.5);
    expect(getTargetAlignFreq("H")).toBe(500);
    expect(getTargetOffset("I")).toBe(0); // independent per preset
    expect(getTargetAlignFreq("I")).toBe(1000);
  });

  it("stores and clears a per-preset measurement", () => {
    expect(getMeasurement("E")).toBeNull();
    setMeasurement("E", { name: "m.txt", points: [{ freq: 100, spl: 1 }] });
    expect(getMeasurement("E")?.name).toBe("m.txt");
    clearMeasurement("E");
    expect(getMeasurement("E")).toBeNull();
  });

  it("carries view state across a rename and frees the old key", () => {
    setTargetId("Old", "tX");
    setCompensate("Old", true);
    setMeasurement("Old", { name: "m.txt", points: [{ freq: 100, spl: 1 }] });

    renamePresetView("Old", "New");

    // New name has the settings...
    expect(getTargetId("New")).toBe("tX");
    expect(getCompensate("New")).toBe(true);
    expect(getMeasurement("New")?.name).toBe("m.txt");
    // ...and the old name is back to defaults (not orphaned).
    expect(getTargetId("Old")).toBe("flat");
    expect(getCompensate("Old")).toBe(false);
    expect(getMeasurement("Old")).toBeNull();
  });

  it("clears view state on delete", () => {
    setTargetId("Doomed", "tY");
    setMeasurement("Doomed", { name: "d.txt", points: [{ freq: 200, spl: 2 }] });

    clearPresetView("Doomed");

    expect(getTargetId("Doomed")).toBe("flat");
    expect(getMeasurement("Doomed")).toBeNull();
  });

  it("rename is a no-op when there's nothing to carry", () => {
    renamePresetView("Nonexistent", "Whatever");
    expect(getTargetId("Whatever")).toBe("flat"); // not created
  });
});
