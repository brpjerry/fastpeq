// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, fireEvent, cleanup, waitFor } from "@testing-library/svelte";
import { emit } from "@tauri-apps/api/event";
import * as api from "./lib/api";
import { ACCENTS } from "./lib/theme";
import {
  getFilterSet,
  setFilterSet,
  getSpecialtyIcons,
  setSpecialtyIcons,
  setBluetoothIcons,
  getFilterShapes,
  setFilterShapes,
  getToneStep,
  setToneStep,
} from "./lib/prefs.svelte";
import { addHotkey, updateHotkey, removeHotkey } from "./lib/hotkeys.svelte";
import App from "./App.svelte";

// Capture event listeners (to fire "hotkey-pressed") and the window focus
// callback (to simulate the window losing focus for the OSD gate).
const { listeners, focus } = vi.hoisted(() => ({
  listeners: {} as Record<string, (e: { payload: unknown }) => void>,
  focus: { cb: null as null | ((e: { payload: boolean }) => void) },
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((event: string, cb: (e: { payload: unknown }) => void) => {
    listeners[event] = cb;
    return Promise.resolve(() => {});
  }),
  emit: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isFocused: () => Promise.resolve(true),
    onFocusChanged: (cb: (e: { payload: boolean }) => void) => {
      focus.cb = cb;
      return Promise.resolve(() => {});
    },
  }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("./lib/api", () => {
  const ok = () => vi.fn(() => Promise.resolve());
  const report = () => vi.fn(() => Promise.resolve({ imported: [], skipped: [], ignored: 0 }));
  return {
    // Data reads — configured per test below.
    apoStatus: vi.fn(),
    presetsDir: vi.fn(),
    listPresets: vi.fn(),
    presetCategories: vi.fn(),
    activePreset: vi.fn(),
    bypassed: vi.fn(),
    getTone: vi.fn(),
    getPreset: vi.fn(() => Promise.resolve({ lines: [] })),
    listAudioDevices: vi.fn(() => Promise.resolve([])),
    listHardwareDevices: vi.fn(() => Promise.resolve([])),
    hardwareStatus: vi.fn(() =>
      Promise.resolve({
        enabled: false,
        active: false,
        device: null,
        version: null,
        error: null,
        max_filters: null,
        mode: "apo-only",
      }),
    ),
    refreshHardware: vi.fn(() =>
      Promise.resolve({
        enabled: false,
        active: false,
        device: null,
        version: null,
        error: null,
        max_filters: null,
        mode: "apo-only",
      }),
    ),
    offloadSelection: vi.fn(() => Promise.resolve([])),
    // Mutations — resolve to void / a report.
    applyPreset: ok(),
    toggleBypass: ok(),
    captureCurrent: ok(),
    deletePreset: ok(),
    renamePreset: ok(),
    setCategory: ok(),
    savePreset: ok(),
    applyLive: ok(),
    setTone: ok(),
    setDefaultAudioDevice: ok(),
    setOffloadMode: ok(),
    setHotkeys: vi.fn(() => Promise.resolve([])),
    loadHotkeyBindings: vi.fn(() => Promise.resolve(null)),
    saveHotkeyBindings: ok(),
    loadUiState: vi.fn(() => Promise.resolve(null)),
    saveUiState: ok(),
    readTextFile: vi.fn(() => Promise.resolve("")),
    setPresetsDir: ok(),
    resetPresetsDir: ok(),
    openPresetsDir: ok(),
    importPeacePresets: report(),
    importPeaceFiles: report(),
  };
});

const FLAT_TONE = { bass: 0, mid: 0, treble: 0, invert: false, swap: false };
const rows = (root: ParentNode) => [...root.querySelectorAll(".presets li:not(.empty)")];
const rowFor = (root: ParentNode, name: string) =>
  rows(root).find((li) => li.querySelector(".name")?.textContent?.trim() === name)!;

beforeEach(() => {
  vi.mocked(api.apoStatus).mockResolvedValue({ installed: true, config_path: "C:/config.txt", error: null });
  vi.mocked(api.presetsDir).mockResolvedValue("C:/presets");
  vi.mocked(api.bypassed).mockResolvedValue(false);
  vi.mocked(api.getTone).mockResolvedValue(FLAT_TONE);
  vi.mocked(api.activePreset).mockResolvedValue(null); // no active preset → no Editor rendered
  vi.mocked(api.listPresets).mockResolvedValue([]);
  vi.mocked(api.presetCategories).mockResolvedValue({});
});
afterEach(cleanup);

function withLibrary() {
  vi.mocked(api.listPresets).mockResolvedValue(["64 Audio U12t", "Sennheiser HD600"]);
  vi.mocked(api.presetCategories).mockResolvedValue({
    "64 Audio U12t": "iem",
    "Sennheiser HD600": "headphone",
  });
}

describe("App preset list", () => {
  it("renders presets from the backend", async () => {
    withLibrary();
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));
    expect(container.textContent).toContain("64 Audio U12t");
    expect(container.textContent).toContain("Sennheiser HD600");
  });

  it("filters by search query", async () => {
    withLibrary();
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));
    await fireEvent.input(container.querySelector(".search")!, { target: { value: "64" } });
    expect(rows(container).length).toBe(1);
    expect(rows(container)[0].textContent).toContain("64 Audio U12t");
  });

  it("filters by device type via the icon dropdown", async () => {
    withLibrary();
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));

    await fireEvent.click(container.querySelector(".type-trigger")!);
    const menu = container.querySelector(".type-menu");
    expect(menu).toBeTruthy();

    const headphone = [...menu!.querySelectorAll(".cat-menu-item")].find(
      (b) => b.textContent!.trim() === "Headphone",
    );
    await fireEvent.click(headphone!);

    expect(rows(container).length).toBe(1);
    expect(rows(container)[0].textContent).toContain("Sennheiser HD600");
  });

  it("only offers device types that some preset uses", async () => {
    withLibrary(); // iem + headphone only
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));
    await fireEvent.click(container.querySelector(".type-trigger")!);
    const labels = [...container.querySelectorAll(".type-menu .cat-menu-item")].map((b) =>
      b.textContent!.trim(),
    );
    expect(labels).toContain("All types");
    expect(labels).toContain("Headphone");
    expect(labels).toContain("IEM");
    expect(labels).not.toContain("Speaker"); // no speaker presets exist
  });
});

describe("App category assignment", () => {
  it("cycles a preset's category on left-click", async () => {
    withLibrary();
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));

    await fireEvent.click(rowFor(container, "Sennheiser HD600").querySelector(".cat")!);
    await waitFor(() => expect(api.setCategory).toHaveBeenCalled());
    const calls = vi.mocked(api.setCategory).mock.calls;
    expect(calls[calls.length - 1][0]).toBe("Sennheiser HD600");
  });

  it("assigns a category from the right-click menu", async () => {
    withLibrary();
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));

    await fireEvent.contextMenu(rowFor(container, "Sennheiser HD600").querySelector(".cat")!, {
      clientX: 20,
      clientY: 20,
    });
    const menu = container.querySelector(".cat-menu:not(.type-menu)");
    expect(menu).toBeTruthy();

    const iem = [...menu!.querySelectorAll(".cat-menu-item")].find(
      (b) => b.textContent!.trim() === "IEM",
    );
    await fireEvent.click(iem!);
    await waitFor(() => expect(api.setCategory).toHaveBeenCalledWith("Sennheiser HD600", "iem"));
  });
});

describe("App settings", () => {
  beforeEach(() => {
    localStorage.clear();
    setFilterSet("full");
    setSpecialtyIcons(false);
    setBluetoothIcons(false);
    setFilterShapes(true);
    setToneStep(0.5);
  });

  it("applies an accent color to the document", async () => {
    const { container } = render(App);
    await fireEvent.click(container.querySelector(".gear")!);
    const swatches = [...container.querySelectorAll<HTMLButtonElement>(".swatch")];

    await fireEvent.click(swatches[1]);
    expect(document.documentElement.style.getPropertyValue("--accent")).toBe(ACCENTS[1].accent);
    expect(swatches[1].classList.contains("sel")).toBe(true);
  });

  it("sets the tone control step", async () => {
    const { container } = render(App);
    await fireEvent.click(container.querySelector(".gear")!);
    const oneDb = [...container.querySelectorAll(".seg-btn")].find((b) =>
      b.textContent!.includes("1 dB"),
    )!;
    await fireEvent.click(oneDb);
    expect(getToneStep()).toBe(1);
  });

  it("switches the editor's filter set", async () => {
    const { container } = render(App);
    await fireEvent.click(container.querySelector(".gear")!);
    const basic = [...container.querySelectorAll(".seg-btn")].find((b) =>
      b.textContent!.includes("Basic"),
    )!;
    await fireEvent.click(basic);
    expect(getFilterSet()).toBe("basic");
  });

  it("toggles a specialty category group", async () => {
    const { container } = render(App);
    await fireEvent.click(container.querySelector(".gear")!);
    const label = [...container.querySelectorAll(".switch")].find((l) =>
      l.textContent!.includes("Specialty"),
    )!;
    const sw = label.querySelector<HTMLInputElement>("input[type='checkbox']")!;

    const before = getSpecialtyIcons();
    sw.checked = !before;
    await fireEvent.change(sw);
    expect(getSpecialtyIcons()).toBe(!before);
  });

  it("toggles the filter-shapes handle style", async () => {
    const { container } = render(App);
    await fireEvent.click(container.querySelector(".gear")!);
    const label = [...container.querySelectorAll(".switch")].find((l) =>
      l.textContent!.includes("filter shape"),
    )!;
    const cb = label.querySelector<HTMLInputElement>("input[type='checkbox']")!;

    const before = getFilterShapes();
    cb.checked = !before;
    await fireEvent.change(cb);
    expect(getFilterShapes()).toBe(!before);
  });
});

describe("App global hotkeys", () => {
  it("switches to the bound preset when a hotkey fires", async () => {
    withLibrary(); // includes "Sennheiser HD600"
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));

    const id = addHotkey();
    updateHotkey(id, { key: "1", action: "preset", preset: "Sennheiser HD600" });
    listeners["hotkey-pressed"]({ payload: id });

    await waitFor(() => expect(api.applyPreset).toHaveBeenCalledWith("Sennheiser HD600"));
    removeHotkey(id);
  });

  it("nudges the tone by the configured step on a tone hotkey", async () => {
    withLibrary();
    setToneStep(0.5);
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));

    const id = addHotkey();
    updateHotkey(id, { key: "2", action: "tone-up", tone: "bass" });
    vi.mocked(api.setTone).mockClear();
    listeners["hotkey-pressed"]({ payload: id });

    await waitFor(() => expect(api.setTone).toHaveBeenCalled());
    const last = vi.mocked(api.setTone).mock.calls.at(-1)![0];
    expect(last.bass).toBeCloseTo(0.5);
    removeHotkey(id);
  });

  it("resets the tone on a reset-tone hotkey", async () => {
    withLibrary();
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));

    const id = addHotkey();
    updateHotkey(id, { key: "0", action: "tone-reset" });
    vi.mocked(api.setTone).mockClear();
    listeners["hotkey-pressed"]({ payload: id });

    await waitFor(() => expect(api.setTone).toHaveBeenCalled());
    expect(vi.mocked(api.setTone).mock.calls.at(-1)![0]).toEqual({
      bass: 0,
      mid: 0,
      treble: 0,
      invert: false,
      swap: false,
    });
    removeHotkey(id);
  });

  it("switches the default output device on a device hotkey", async () => {
    withLibrary();
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));

    const id = addHotkey();
    updateHotkey(id, { key: "D", action: "device", device: "{0.0.0}.{dac}" });
    listeners["hotkey-pressed"]({ payload: id });

    await waitFor(() => expect(api.setDefaultAudioDevice).toHaveBeenCalledWith("{0.0.0}.{dac}"));
    removeHotkey(id);
  });

  it("emits an OSD payload for a hotkey fired while the window is unfocused", async () => {
    withLibrary();
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));

    focus.cb?.({ payload: false }); // window lost focus
    vi.mocked(emit).mockClear();
    const id = addHotkey();
    updateHotkey(id, { key: "1", action: "preset", preset: "Sennheiser HD600" });
    listeners["hotkey-pressed"]({ payload: id });

    await waitFor(() =>
      expect(emit).toHaveBeenCalledWith("osd:show", { title: "Preset", detail: "Sennheiser HD600" }),
    );
    removeHotkey(id);
  });

  it("does not emit an OSD payload while the window is focused", async () => {
    withLibrary();
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));

    focus.cb?.({ payload: true }); // window focused (also the default)
    vi.mocked(emit).mockClear();
    const id = addHotkey();
    updateHotkey(id, { key: "2", action: "tone-up", tone: "bass" });
    listeners["hotkey-pressed"]({ payload: id });

    await waitFor(() => expect(api.setTone).toHaveBeenCalled()); // the action still ran
    expect(emit).not.toHaveBeenCalledWith("osd:show", expect.anything());
    removeHotkey(id);
  });
});

describe("App name-conflict guards", () => {
  it("rejects a new preset whose name collides case-insensitively", async () => {
    withLibrary(); // includes "Sennheiser HD600"
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));

    await fireEvent.click(container.querySelector(".new-btn")!);
    await fireEvent.input(container.querySelector(".create input")!, {
      target: { value: "sennheiser hd600" },
    });
    const fromScratch = [...container.querySelectorAll(".create-actions button")].find((b) =>
      b.textContent!.includes("From scratch"),
    )!;
    await fireEvent.click(fromScratch);

    expect(api.savePreset).not.toHaveBeenCalled();
    await waitFor(() => expect(container.textContent).toContain("already exists"));
  });
});

describe("App scroll-to-active", () => {
  it("centers the active preset in the list on open", async () => {
    const scrollSpy = vi.fn();
    Element.prototype.scrollIntoView = scrollSpy;
    vi.mocked(api.listPresets).mockResolvedValue(["64 Audio U12t", "Sennheiser HD600"]);
    vi.mocked(api.activePreset).mockResolvedValue("Sennheiser HD600");

    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));
    // The active preset is centered (not left at the top of the list).
    await waitFor(() => expect(scrollSpy).toHaveBeenCalledWith({ block: "center" }));
  });

  it("centers the opened preset after pressing Enter in the search box", async () => {
    const scrollSpy = vi.fn();
    Element.prototype.scrollIntoView = scrollSpy;
    vi.mocked(api.listPresets).mockResolvedValue(["64 Audio U12t", "Sennheiser HD600"]);

    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));
    scrollSpy.mockClear(); // ignore any on-open scroll

    const search = container.querySelector(".search")!;
    await fireEvent.input(search, { target: { value: "Senn" } });
    await fireEvent.keyDown(search, { key: "Enter" });

    // Enter opens the top match, clears the query, and scrolls it into view.
    await waitFor(() => expect(api.applyPreset).toHaveBeenCalledWith("Sennheiser HD600"));
    await waitFor(() => expect(scrollSpy).toHaveBeenCalledWith({ block: "center" }));
  });

  it("re-centers the active preset when returning from settings", async () => {
    const scrollSpy = vi.fn();
    Element.prototype.scrollIntoView = scrollSpy;
    vi.mocked(api.listPresets).mockResolvedValue(["64 Audio U12t", "Sennheiser HD600"]);
    vi.mocked(api.activePreset).mockResolvedValue("Sennheiser HD600");

    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));
    scrollSpy.mockClear(); // ignore the on-open scroll

    await fireEvent.click(container.querySelector(".gear")!); // enter settings
    await fireEvent.click(container.querySelector(".gear")!); // exit settings
    await waitFor(() => expect(scrollSpy).toHaveBeenCalledWith({ block: "center" }));
  });
});
