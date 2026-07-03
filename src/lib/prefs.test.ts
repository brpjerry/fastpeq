// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, vi } from "vitest";
import * as api from "./api";
import {
  initPrefs,
  getFilterSet,
  setFilterSet,
  getToneVolumeCap,
  setToneVolumeCap,
  getToneStep,
  setToneStep,
  getToneHeadroom,
  setToneHeadroom,
  getSpecialtyIcons,
  getBluetoothIcons,
  getFilterShapes,
  getAutoPreamp,
  setAutoPreamp,
  defaultBandCount,
  setDefaultBandCount,
} from "./prefs.svelte";

vi.mock("./api", () => ({
  loadUiState: vi.fn(() => Promise.resolve(null)),
  saveUiState: vi.fn(() => Promise.resolve()),
}));

const KEY = "fastpeq.prefs";

// Re-init from "no file, empty localStorage" so each test starts from defaults
// (the module state would otherwise leak between tests).
beforeEach(async () => {
  localStorage.clear();
  vi.mocked(api.loadUiState).mockResolvedValue(null);
  await initPrefs();
  vi.mocked(api.saveUiState).mockClear();
});

describe("prefs persistence", () => {
  it("loads the document from the backend file when one exists", async () => {
    vi.mocked(api.loadUiState).mockResolvedValue(
      JSON.stringify({ filterSet: "basic", toneStep: 1, autoPreamp: true, bandCount: 20 }),
    );
    await initPrefs();
    expect(getFilterSet()).toBe("basic");
    expect(getToneStep()).toBe(1);
    expect(getAutoPreamp()).toBe(true);
    expect(defaultBandCount()).toBe(20);
    // The file is authoritative — a plain load never writes anything back.
    expect(api.saveUiState).not.toHaveBeenCalled();
  });

  it("migrates only the pre-file localStorage keys that were actually set", async () => {
    localStorage.clear(); // a true pre-file profile has no whole-document backup copy
    localStorage.setItem("fastpeq.toneStep", "1");
    localStorage.setItem("fastpeq:autoPreamp", "true"); // the pre-"fastpeq." key
    localStorage.setItem("fastpeq.bandCount", "30");
    await initPrefs();
    expect(getToneStep()).toBe(1);
    expect(getAutoPreamp()).toBe(true);
    expect(defaultBandCount()).toBe(30);
    // Unset prefs stay out of the document, so later default changes reach
    // migrated users.
    const written = JSON.parse(vi.mocked(api.saveUiState).mock.calls[0][1]);
    expect(written).toEqual({ toneStep: 1, autoPreamp: true, bandCount: 30 });
  });

  it("prefers the whole-document backup copy over the per-pref legacy keys", async () => {
    localStorage.setItem(KEY, JSON.stringify({ toneHeadroom: 6 }));
    localStorage.setItem("fastpeq.toneHeadroom", "12"); // stale legacy key loses
    await initPrefs();
    expect(getToneHeadroom()).toBe(6);
  });

  it("never overwrites an unreadable file", async () => {
    vi.mocked(api.loadUiState).mockResolvedValue("{corrupt");
    await initPrefs();
    expect(getFilterSet()).toBe("full"); // defaults, but the file is left alone
    expect(api.saveUiState).not.toHaveBeenCalled();
  });

  it("persists every change to the file and the localStorage backup", () => {
    setFilterSet("basic");
    expect(api.saveUiState).toHaveBeenCalledTimes(1);
    expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual({ filterSet: "basic" });
  });

  it("falls back per field when the file holds out-of-range values", async () => {
    vi.mocked(api.loadUiState).mockResolvedValue(
      JSON.stringify({ toneStep: 99, toneVolumeCap: "loud", bandCount: 11, filterSet: "huh" }),
    );
    await initPrefs();
    expect(getToneStep()).toBe(0.5);
    expect(getToneVolumeCap()).toBe(0.2);
    expect(defaultBandCount()).toBe(10);
    expect(getFilterSet()).toBe("full");
  });
});

describe("prefs defaults and clamping", () => {
  it("serves the documented defaults on a fresh profile", () => {
    expect(getFilterSet()).toBe("full");
    expect(getToneVolumeCap()).toBe(0.2);
    expect(getToneStep()).toBe(0.5);
    expect(getToneHeadroom()).toBe(0);
    expect(getSpecialtyIcons()).toBe(false);
    expect(getBluetoothIcons()).toBe(false);
    expect(getFilterShapes()).toBe(true);
    expect(getAutoPreamp()).toBe(false);
    expect(defaultBandCount()).toBe(10);
  });

  it("clamps writes to each pref's range", () => {
    setToneVolumeCap(2);
    expect(getToneVolumeCap()).toBe(1);
    setToneVolumeCap(0.001);
    expect(getToneVolumeCap()).toBe(0.05);
    setToneHeadroom(-5);
    expect(getToneHeadroom()).toBe(0);
    setToneHeadroom(99);
    expect(getToneHeadroom()).toBe(30);
    setToneStep(9); // out of range resets to the default
    expect(getToneStep()).toBe(0.5);
  });

  it("round-trips the auto-preamp toggle and the band count", () => {
    setAutoPreamp(true);
    expect(getAutoPreamp()).toBe(true);
    setAutoPreamp(false);
    expect(getAutoPreamp()).toBe(false);
    setDefaultBandCount(15);
    expect(defaultBandCount()).toBe(15);
  });
});
