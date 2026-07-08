// @vitest-environment happy-dom
import { describe, it, expect, vi, afterEach } from "vitest";
import { tick } from "svelte";
import { render, fireEvent, cleanup, waitFor } from "@testing-library/svelte";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { Config } from "./types";
import * as api from "./api";
import { setAutoPreamp } from "./prefs.svelte";
import { addTarget, removeTarget } from "./targets.svelte";
import {
  getTargetId,
  setTargetId,
  getMeasurement,
  setMeasurement,
  getCompensate,
  setCompensate,
  getShowMeasRef,
  getTargetOffset,
} from "./preset-view.svelte";
import { longDate } from "./time";
import Editor from "./Editor.svelte";

// The IPC calls the Editor (and the stores it persists through — prefs,
// preset-view, targets) touches; an explicit mock is clearer than auto-mock
// and resolves the mutations to void by default.
vi.mock("./api", () => ({
  getPreset: vi.fn(),
  applyLive: vi.fn(() => Promise.resolve()),
  savePreset: vi.fn(() => Promise.resolve()),
  readTextFile: vi.fn(() => Promise.resolve("")),
  offloadSelection: vi.fn(() => Promise.resolve([])),
  loadUiState: vi.fn(() => Promise.resolve(null)),
  saveUiState: vi.fn(() => Promise.resolve()),
  presetHistory: vi.fn(() => Promise.resolve([])),
  getRevision: vi.fn(),
  restoreRevision: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const FLAT_TONE = { bass: 0, mid: 0, treble: 0, invert: false, swap: false };

function cfg(preamp: number, peaks: Array<[number, number, number]>): Config {
  const lines: Config["lines"] = [];
  if (preamp !== 0) {
    lines.push({ kind: "Preamp", value: { gain: preamp, channel: { kind: "both" } } });
  }
  peaks.forEach(([freq, gain, q], i) =>
    lines.push({
      kind: "Filter",
      value: { enabled: true, kind: "Peak", freq, gain, q, index: i + 1, channel: { kind: "both" } },
    }),
  );
  return { lines };
}

function renderEditor(config: Config, props: Record<string, unknown> = {}) {
  vi.mocked(api.getPreset).mockResolvedValue(config);
  return render(Editor, {
    props: { name: "Test", tone: FLAT_TONE, bypassed: false, reloadToken: 0, onApplied: vi.fn(), ...props },
  });
}

const bandCount = (root: ParentNode) => root.querySelectorAll(".band").length;

afterEach(cleanup);

describe("Editor", () => {
  it("loads a preset's filters as bands", async () => {
    const { container } = renderEditor(cfg(-10, [[100, 3, 1], [1000, -2, 2]]));
    await waitFor(() => expect(bandCount(container)).toBe(2));
  });

  it("adds a band", async () => {
    const { container } = renderEditor(cfg(-10, [[100, 0, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));
    await fireEvent.click(container.querySelector(".band-actions .add")!);
    expect(bandCount(container)).toBe(2);
  });

  it("filters the full L+R list with the APO/HW-only buttons in a hybrid offload mode", async () => {
    vi.mocked(api.offloadSelection).mockResolvedValue([0]); // only the first band
    const { container } = renderEditor(cfg(0, [[100, 3, 1], [1000, 3, 1]]), {
      offloadActive: true,
    });
    // The tabs stay the plain channel lists; the full L+R list shows both bands
    // with their engine statuses once the backend selection lands.
    await waitFor(() => expect(bandCount(container)).toBe(2));
    await waitFor(() => expect(container.querySelector(".status.hw")).toBeTruthy());
    const tabLabels = [...container.querySelectorAll(".view-seg button")].map((b) =>
      b.textContent!.trim(),
    );
    expect(tabLabels).toEqual(["L+R · 2", "L", "R"]);
    const statuses = () =>
      [...container.querySelectorAll(".band .status")].map((s) => s.textContent!.trim());
    expect(statuses()).toEqual(["HW", "APO"]);

    // "HW only" narrows the list to the offloaded band; clicking it again
    // restores the full list; "APO only" shows the software remainder.
    const engineBtns = () => [...container.querySelectorAll<HTMLButtonElement>(".engine-seg button")];
    expect(engineBtns().map((b) => b.textContent!.trim())).toEqual(["APO only", "HW only"]);
    await fireEvent.click(engineBtns()[1]);
    expect(statuses()).toEqual(["HW"]);
    expect(engineBtns()[1].classList.contains("sel")).toBe(true);
    await fireEvent.click(engineBtns()[1]);
    expect(statuses()).toEqual(["HW", "APO"]);
    await fireEvent.click(engineBtns()[0]);
    expect(statuses()).toEqual(["APO"]);
  });

  it("mutes enabled bands that don't fit the device in Hardware Only mode", async () => {
    vi.mocked(api.offloadSelection).mockResolvedValue([0]); // only the first band fits
    const { container } = renderEditor(cfg(0, [[100, 3, 1], [1000, 3, 1]]), {
      offloadActive: true,
      hardwareOnly: true,
      forceAutoPreamp: true,
    });
    // Hardware Only keeps binary ON/OFF statuses and offers no engine filter —
    // there is only one engine.
    await waitFor(() => expect(bandCount(container)).toBe(2));
    // Wait for the debounced backend selection to land (until then every band
    // transiently reads as muted, since nothing is on the device yet).
    await waitFor(() =>
      expect(container.querySelectorAll(".band")[0].classList.contains("muted")).toBe(false),
    );

    // The fitting band runs on the device; the other is enabled but silent —
    // APO stays flat, so it runs nowhere and reads as muted (hollow ON).
    const bandRows = container.querySelectorAll(".band");
    expect(bandRows[0].querySelector(".status")!.textContent!.trim()).toBe("ON");
    expect(bandRows[0].querySelector(".status.silent")).toBeNull();
    expect(bandRows[1].querySelector(".status")!.textContent!.trim()).toBe("ON");
    expect(bandRows[1].querySelector(".status.silent")).toBeTruthy();
    expect(bandRows[1].classList.contains("muted")).toBe(true);
    expect(container.querySelector(".engine-seg")).toBeNull();
  });

  it("pins the APO preamp to 0 in Hardware Only mode", async () => {
    vi.mocked(api.offloadSelection).mockResolvedValue([0]);
    // The preset carries a -6 dB preamp, but the APO stage doesn't exist in this
    // mode — only the device pregain (covering the +6 boost) attenuates.
    const { container } = renderEditor(cfg(-6, [[1000, 6, 1]]), {
      offloadActive: true,
      hardwareOnly: true,
      forceAutoPreamp: true,
    });
    await waitFor(() => expect(bandCount(container)).toBe(1));
    // Selection landed once the band stops reading as (transiently) muted.
    await waitFor(() =>
      expect(container.querySelector(".band")!.classList.contains("muted")).toBe(false),
    );

    const vals = [...container.querySelectorAll<HTMLInputElement>(".pval input")].map(
      (e) => e.value,
    );
    expect(parseFloat(vals[0])).toBe(0); // APO — flat
    expect(parseFloat(vals[1])).toBeLessThan(0); // Device — carries the headroom
  });

  it("shows plain ON/OFF statuses and no engine filter when offload is inactive", async () => {
    vi.mocked(api.offloadSelection).mockResolvedValue([0]);
    const { container } = renderEditor(cfg(0, [[100, 3, 1]]), { offloadActive: false });
    await waitFor(() => expect(bandCount(container)).toBe(1));
    // Offload is off, so the effect short-circuits and no band is marked.
    expect(container.querySelector(".status.hw")).toBeNull();
    expect(container.querySelector(".band .status")!.textContent!.trim()).toBe("ON");
    expect(container.querySelector(".engine-seg")).toBeNull();
    const tabLabels = [...container.querySelectorAll(".view-seg button")].map((b) =>
      b.textContent!.trim(),
    );
    expect(tabLabels).toEqual(["L+R · 1", "L", "R"]);
  });

  it("splits the preamp into APO + Device sliders when offload is active", async () => {
    const { container } = renderEditor(cfg(0, [[1000, 6, 1]]), { offloadActive: true });
    await waitFor(() => expect(bandCount(container)).toBe(1));
    const labels = [...container.querySelectorAll(".plabel")].map((e) => e.textContent?.trim());
    expect(labels).toContain("APO");
    expect(labels).toContain("Device");
    expect(container.querySelectorAll(".preamp").length).toBe(2);
  });

  it("hides the Device slider when the device's pregain isn't user-adjustable", async () => {
    const { container } = renderEditor(cfg(0, [[1000, 6, 1]]), {
      offloadActive: true,
      hwUserPregain: false,
    });
    await waitFor(() => expect(bandCount(container)).toBe(1));
    const labels = [...container.querySelectorAll(".plabel")].map((e) => e.textContent?.trim());
    expect(labels).toEqual(["APO"]); // the device headrooms itself — no Device row
    expect(container.querySelectorAll(".preamp").length).toBe(1);
  });

  it("shows a single preamp slider when offload is off", async () => {
    const { container } = renderEditor(cfg(-6, [[1000, 6, 1]]), { offloadActive: false });
    await waitFor(() => expect(bandCount(container)).toBe(1));
    const labels = [...container.querySelectorAll(".plabel")].map((e) => e.textContent?.trim());
    expect(labels).toEqual(["Preamp"]);
  });

  it("forces Auto Preamp on and locks the toggle in Min. APO preamp offload mode", async () => {
    // A +8 dB boost with no preset preamp would clip without auto preamp.
    const { container } = renderEditor(cfg(0, [[1000, 8, 1]]), { forceAutoPreamp: true });
    await waitFor(() => expect(bandCount(container)).toBe(1));

    // The manual preamp slider is driven by auto, so it's disabled...
    const slider = container.querySelector(".preamp input[type='range']") as HTMLInputElement;
    expect(slider.disabled).toBe(true);
    // ...and auto pulled the preamp negative to clear the +8 dB boost.
    expect(Number(slider.value)).toBeLessThan(0);

    // The Auto toggle reads on and is locked (disabled).
    const autoSwitch = [...container.querySelectorAll(".switch")].find((s) =>
      s.textContent?.includes("Auto"),
    );
    const autoInput = autoSwitch?.querySelector("input") as HTMLInputElement;
    expect(autoInput.checked).toBe(true);
    expect(autoInput.disabled).toBe(true);

    // No clip warning, since auto preamp prevents clipping.
    expect(container.querySelector(".clip")).toBeNull();
  });

  it("removes a band", async () => {
    const { container } = renderEditor(cfg(-10, [[100, 0, 1], [1000, 0, 2]]));
    await waitFor(() => expect(bandCount(container)).toBe(2));
    await fireEvent.click(container.querySelector(".band .remove")!);
    expect(bandCount(container)).toBe(1);
  });

  it("removes all gain filters sitting at 0 dB", async () => {
    const { container } = renderEditor(cfg(-10, [[100, 3, 1], [1000, 0, 1], [5000, 0, 2]]));
    await waitFor(() => expect(bandCount(container)).toBe(3));
    await fireEvent.click(container.querySelector(".clear-flat")!);
    expect(bandCount(container)).toBe(1); // only the +3 dB band survives
  });

  it("disables the Remove 0 dB button when nothing is flat", async () => {
    const { container } = renderEditor(cfg(-10, [[100, 3, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));
    expect(container.querySelector<HTMLButtonElement>(".clear-flat")!.disabled).toBe(true);
  });

  it("shows the live indicator when not bypassed", async () => {
    const { container } = renderEditor(cfg(-10, [[100, 0, 1]]));
    await waitFor(() => expect(container.querySelector(".live")).toBeTruthy());
    expect(container.querySelector(".live")!.textContent).toContain("live");
    expect(container.querySelector(".live.bypassed")).toBeNull();
  });

  it("shows the bypassed indicator when the prop is set", async () => {
    const { container } = renderEditor(cfg(-10, [[100, 0, 1]]), { bypassed: true });
    await waitFor(() => expect(container.querySelector(".live.bypassed")).toBeTruthy());
    expect(container.querySelector(".live")!.textContent).toContain("bypassed");
  });

  it("flags clipping when the summed boost tops 0 dB", async () => {
    const { container } = renderEditor(cfg(0, [[1000, 10, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));
    expect(container.querySelector(".clip")).toBeTruthy();
  });

  it("does not flag clipping when the preamp keeps it under 0 dB", async () => {
    const { container } = renderEditor(cfg(-10, [[1000, 6, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));
    expect(container.querySelector(".clip")).toBeNull();
  });

  it("pushes a gain edit to the live config", async () => {
    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));
    vi.mocked(api.applyLive).mockClear(); // load itself doesn't apply; isolate the edit

    const gain = container.querySelector<HTMLInputElement>(".field.gain input[type='range']")!;
    await fireEvent.input(gain, { target: { value: "5" } });

    await waitFor(() => expect(api.applyLive).toHaveBeenCalled());
    const calls = vi.mocked(api.applyLive).mock.calls;
    const applied = calls[calls.length - 1][0] as Config;
    expect(applied.lines.some((l) => l.kind === "Filter" && l.value.gain === 5)).toBe(true);
  });

  it("opens the balance popover and dismisses it on an outside click", async () => {
    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));
    expect(container.querySelector(".bal-pop")).toBeNull();

    await fireEvent.click(container.querySelector(".chan")!);
    expect(container.querySelector(".bal-pop")).toBeTruthy();
    // Centered by default, so the reset control is disabled.
    expect(container.querySelector<HTMLButtonElement>(".bal-center")!.disabled).toBe(true);

    document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    await tick();
    expect(container.querySelector(".bal-pop")).toBeNull();
  });

  it("imports a REW measurement: disabled switch beforehand, then shows, persists, and enables it", async () => {
    vi.mocked(openDialog).mockResolvedValue("C:/curves/harman.txt");
    vi.mocked(api.readTextFile).mockResolvedValue("20 -3\n500 0\n1000 0.5\n10000 -2");

    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]), { name: "ImportMeas" });
    await waitFor(() => expect(bandCount(container)).toBe(1));
    await fireEvent.click(container.querySelector(".expand-btn")!);

    // With no measurement yet, the reference switch is disabled and off.
    const before = container.querySelector<HTMLInputElement>(".meas-group .switch input[type='checkbox']")!;
    expect(before.disabled).toBe(true);
    expect(before.checked).toBe(false);

    const importBtn = await waitFor(() => {
      const b = [...container.querySelectorAll("button")].find((x) =>
        x.textContent!.includes("Import REW"),
      );
      if (!b) throw new Error("import button not rendered yet");
      return b;
    });
    await fireEvent.click(importBtn);

    // After import: the name shows, it's persisted per preset, and the switch auto-enables.
    await waitFor(() => expect(container.querySelector(".meas-name")?.textContent).toContain("harman.txt"));
    expect(getMeasurement("ImportMeas")?.name).toBe("harman.txt");
    await waitFor(() => {
      const sw = container.querySelector<HTMLInputElement>(".meas-group .switch input[type='checkbox']")!;
      expect(sw.disabled).toBe(false);
      expect(sw.checked).toBe(true);
    });
    expect(getShowMeasRef("ImportMeas")).toBe(true);
  });

  it("auto-loads a saved measurement for the preset", async () => {
    setMeasurement("AutoLoad", {
      name: "saved.txt",
      points: [{ freq: 100, spl: 2 }, { freq: 1000, spl: 0 }],
    });
    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]), { name: "AutoLoad" });
    await waitFor(() => expect(bandCount(container)).toBe(1));
    // The small graph shows the measurement reference with no import action.
    expect(container.querySelector(".graph-wrap .resp.reference")).toBeTruthy();
    // The expanded meas-tools reflect the saved name.
    await fireEvent.click(container.querySelector(".expand-btn")!);
    await waitFor(() => expect(container.querySelector(".meas-name")?.textContent).toContain("saved.txt"));
  });

  it("wraps the expanded graph in the fixed-aspect container", async () => {
    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));
    await fireEvent.click(container.querySelector(".expand-btn")!);
    await waitFor(() => expect(container.querySelector(".graph-fit")).toBeTruthy());
    expect(container.querySelector(".graph-fit .ce-wrap")).toBeTruthy();
  });

  it("offers the target dropdown and persists the selection per preset", async () => {
    const id = addTarget("Harman", [{ freq: 100, spl: 1 }, { freq: 1000, spl: 0 }]);
    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]), { name: "TgtPreset" });
    await waitFor(() => expect(bandCount(container)).toBe(1));
    await fireEvent.click(container.querySelector(".expand-btn")!);

    const trigger = await waitFor(() => {
      const b = container.querySelector<HTMLButtonElement>(".target-select .sm-btn");
      if (!b) throw new Error("target dropdown not rendered");
      return b;
    });
    await fireEvent.click(trigger);

    const items = await waitFor(() => {
      const list = [...container.querySelectorAll<HTMLButtonElement>(".sm-menu .sm-item")];
      if (!list.length) throw new Error("target menu not open");
      return list;
    });
    expect(items.map((i) => i.textContent!.trim())).toContain("Flat");

    await fireEvent.click(items.find((i) => i.textContent!.trim() === "Harman")!);
    expect(getTargetId("TgtPreset")).toBe(id);
    removeTarget(id);
  });

  it("toggles compensate per preset (with a non-flat target)", async () => {
    const id = addTarget("Tc", [{ freq: 100, spl: 1 }, { freq: 1000, spl: 0 }]);
    setTargetId("CompPreset", id);
    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]), { name: "CompPreset" });
    await waitFor(() => expect(bandCount(container)).toBe(1));
    await fireEvent.click(container.querySelector(".expand-btn")!);

    const toggle = await waitFor(() => {
      const label = [...container.querySelectorAll(".switch")].find((l) =>
        l.textContent!.includes("Compensate"),
      );
      const t = label?.querySelector<HTMLInputElement>("input[type='checkbox']");
      if (!t) throw new Error("compensate toggle not rendered");
      return t;
    });
    expect(toggle.disabled).toBe(false); // enabled with a non-flat target shown
    expect(getCompensate("CompPreset")).toBe(false);
    toggle.checked = true;
    await fireEvent.change(toggle);
    expect(getCompensate("CompPreset")).toBe(true);
    removeTarget(id);
  });

  it("disables the compensate switch on a flat target", async () => {
    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]), { name: "FlatComp" });
    await waitFor(() => expect(bandCount(container)).toBe(1));
    await fireEvent.click(container.querySelector(".expand-btn")!);
    const toggle = await waitFor(() => {
      const label = [...container.querySelectorAll(".switch")].find((l) =>
        l.textContent!.includes("Compensate"),
      );
      const t = label?.querySelector<HTMLInputElement>("input[type='checkbox']");
      if (!t) throw new Error("compensate switch not rendered");
      return t;
    });
    expect(toggle.disabled).toBe(true); // nothing to compensate against
    // The target trace controls are hidden for the Flat target.
    expect(container.querySelector(".target-adjust")).toBeNull();
  });

  it("aligns the target offset to the response at the align frequency", async () => {
    const id = addTarget("Tm", [{ freq: 100, spl: 2 }, { freq: 1000, spl: 6 }]);
    setTargetId("AlignPreset", id);
    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]), { name: "AlignPreset" });
    await waitFor(() => expect(bandCount(container)).toBe(1));
    await fireEvent.click(container.querySelector(".expand-btn")!);

    const alignBtn = await waitFor(() => {
      const b = [...container.querySelectorAll(".target-adjust button")].find((x) =>
        x.textContent!.includes("Align"),
      );
      if (!b) throw new Error("align button not rendered");
      return b;
    });
    // Default align freq 1 kHz, target=6 there, flat 0 dB bands → offset -6.
    expect(getTargetOffset("AlignPreset")).toBe(0);
    await fireEvent.click(alignBtn);
    expect(getTargetOffset("AlignPreset")).toBeCloseTo(-6, 1);
    removeTarget(id);
  });

  it("toggles the measurement reference per preset", async () => {
    setMeasurement("RefPreset", { name: "m.txt", points: [{ freq: 100, spl: 1 }, { freq: 1000, spl: 0 }] });
    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]), { name: "RefPreset" });
    await waitFor(() => expect(bandCount(container)).toBe(1));
    await fireEvent.click(container.querySelector(".expand-btn")!);

    const toggle = await waitFor(() => {
      const t = container.querySelector<HTMLInputElement>(
        ".meas-group .switch input[type='checkbox']",
      );
      if (!t) throw new Error("measurement-reference toggle not rendered");
      return t;
    });
    expect(toggle.disabled).toBe(false); // enabled because a measurement exists
    expect(getShowMeasRef("RefPreset")).toBe(true);
    toggle.checked = false;
    await fireEvent.change(toggle);
    expect(getShowMeasRef("RefPreset")).toBe(false);
  });

  it("forces the target switch on and disabled while compensating", async () => {
    const id = addTarget("Tf", [{ freq: 100, spl: 1 }, { freq: 1000, spl: 0 }]);
    setTargetId("CompTgt", id);
    setCompensate("CompTgt", true);
    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]), { name: "CompTgt" });
    await waitFor(() => expect(bandCount(container)).toBe(1));
    await fireEvent.click(container.querySelector(".expand-btn")!);

    const sw = await waitFor(() => {
      const t = container.querySelector<HTMLInputElement>(
        ".target-group .switch input[type='checkbox']",
      );
      if (!t) throw new Error("target switch not rendered");
      return t;
    });
    expect(sw.disabled).toBe(true);
    expect(sw.checked).toBe(true);
    removeTarget(id);
  });

  it("undoes and redoes a band edit", async () => {
    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));

    await fireEvent.click(container.querySelector(".band-actions .add")!);
    expect(bandCount(container)).toBe(2);

    // The edit records into history once the coalesce window passes, enabling undo.
    const undoBtn = container.querySelector<HTMLButtonElement>(".undo-btn")!;
    await waitFor(() => expect(undoBtn.disabled).toBe(false), { timeout: 1500 });
    await fireEvent.click(undoBtn);
    expect(bandCount(container)).toBe(1);

    const redoBtn = container.querySelector<HTMLButtonElement>(".redo-btn")!;
    expect(redoBtn.disabled).toBe(false);
    await fireEvent.click(redoBtn);
    expect(bandCount(container)).toBe(2);
  });

  it("undoes with Ctrl+Z immediately, without waiting for the coalesce window", async () => {
    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));

    await fireEvent.click(container.querySelector(".band-actions .add")!);
    expect(bandCount(container)).toBe(2);

    // No wait: Ctrl+Z flushes the pending edit and undoes it right away.
    await fireEvent.keyDown(window, { key: "z", ctrlKey: true });
    expect(bandCount(container)).toBe(1);
    // Ctrl+Y redoes.
    await fireEvent.keyDown(window, { key: "y", ctrlKey: true });
    expect(bandCount(container)).toBe(2);
  });

  it("auto-preamp drops the preamp to stop clipping", async () => {
    const { container } = renderEditor(cfg(0, [[1000, 10, 1]])); // +10 dB at preamp 0 → clips
    await waitFor(() => expect(bandCount(container)).toBe(1));
    expect(container.querySelector(".clip")).toBeTruthy();

    const auto = [...container.querySelectorAll(".switch")].find((l) =>
      l.textContent!.includes("Auto"),
    )!;
    const cb = auto.querySelector<HTMLInputElement>("input[type='checkbox']")!;
    cb.checked = true;
    await fireEvent.change(cb);

    // Preamp pulled to ~-10 dB, the clip warning clears, and the slider locks.
    await waitFor(() => expect(container.querySelector(".clip")).toBeNull());
    expect(container.querySelector<HTMLInputElement>(".pval input")!.value).toContain("-10");
    expect(container.querySelector<HTMLInputElement>(".preamp input[type='range']")!.disabled).toBe(true);
  });

  it("applies a preamp typed into the dB textbox", async () => {
    setAutoPreamp(false); // the textbox is editable only with Auto off
    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));
    vi.mocked(api.applyLive).mockClear();

    const box = container.querySelector<HTMLInputElement>(".pval input")!;
    expect(box.disabled).toBe(false);
    await fireEvent.change(box, { target: { value: "-12.5" } });

    await waitFor(() => expect(api.applyLive).toHaveBeenCalled());
    const applied = vi.mocked(api.applyLive).mock.calls.at(-1)![0] as Config;
    const preamp = applied.lines.find(
      (l) => l.kind === "Preamp" && l.value.channel.kind === "both",
    );
    expect((preamp!.value as { gain: number }).gain).toBe(-12.5);
  });

  it("auto-preamp accounts for the global tone overlay", async () => {
    // Flat bands, but a +6 dB bass tone overlay → auto-preamp must still pull down.
    const { container } = renderEditor(cfg(0, [[1000, 0, 1]]), {
      tone: { bass: 6, mid: 0, treble: 0, invert: false, swap: false },
    });
    await waitFor(() => expect(bandCount(container)).toBe(1));

    const auto = [...container.querySelectorAll(".switch")].find((l) =>
      l.textContent!.includes("Auto"),
    )!;
    const cb = auto.querySelector<HTMLInputElement>("input[type='checkbox']")!;
    cb.checked = true;
    await fireEvent.change(cb);

    // Bands-only would leave preamp at 0; counting the tone pulls it negative.
    await waitFor(() =>
      expect(
        parseFloat(container.querySelector<HTMLInputElement>(".pval input")!.value),
      ).toBeLessThan(0),
    );
  });

  it("re-applies the auto preamp to config.txt when the global tone overlay changes", async () => {
    // Auto on with flat tone → preamp sits at 0; a louder tone arriving from the
    // global controls (a prop change, not an editor edit) must push a fresh config
    // so config.txt's preamp tracks it instead of going stale until the next edit.
    setAutoPreamp(false); // start from a known state, then toggle on
    const props = { name: "Test", bypassed: false, reloadToken: 0, onApplied: vi.fn() };
    const { container, rerender } = renderEditor(cfg(0, [[1000, 0, 1]]), { ...props, tone: FLAT_TONE });
    await waitFor(() => expect(bandCount(container)).toBe(1));

    const auto = [...container.querySelectorAll(".switch")].find((l) => l.textContent!.includes("Auto"))!;
    const cb = auto.querySelector<HTMLInputElement>("input[type='checkbox']")!;
    cb.checked = true;
    await fireEvent.change(cb);
    await waitFor(() => expect(api.applyLive).toHaveBeenCalled());
    vi.mocked(api.applyLive).mockClear();

    await rerender({ ...props, tone: { bass: 8, mid: 0, treble: 0, invert: false, swap: false } });

    await waitFor(() => expect(api.applyLive).toHaveBeenCalled());
    const lastCfg = vi.mocked(api.applyLive).mock.calls.at(-1)![0] as Config;
    const preamp = lastCfg.lines.find(
      (l) => l.kind === "Preamp" && l.value.channel.kind === "both",
    );
    expect(preamp).toBeTruthy();
    expect((preamp!.value as { gain: number }).gain).toBeLessThan(0);
  });

  it("does not re-apply on tone changes while Auto Preamp is off", async () => {
    setAutoPreamp(false); // the persisted switch can leak between tests
    const props = { name: "Test", bypassed: false, reloadToken: 0, onApplied: vi.fn() };
    const { container, rerender } = renderEditor(cfg(-10, [[1000, 0, 1]]), { ...props, tone: FLAT_TONE });
    await waitFor(() => expect(bandCount(container)).toBe(1));
    vi.mocked(api.applyLive).mockClear();

    await rerender({ ...props, tone: { bass: 8, mid: 0, treble: 0, invert: false, swap: false } });
    await tick();

    // Manual preamp is static; the backend re-lays tone itself, so the editor stays out of it.
    expect(api.applyLive).not.toHaveBeenCalled();
  });

  it("A/B compares the working edit against the saved version", async () => {
    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));

    const compareBtn = container.querySelector<HTMLButtonElement>(".compare-btn")!;
    expect(compareBtn.disabled).toBe(true); // nothing unsaved to compare yet

    await fireEvent.click(container.querySelector(".band-actions .add")!);
    await waitFor(() => expect(compareBtn.disabled).toBe(false));

    // Compare → hear the saved version (1 band), editing locked.
    vi.mocked(api.applyLive).mockClear();
    await fireEvent.click(compareBtn);
    await waitFor(() => expect(api.applyLive).toHaveBeenCalled());
    const onB = vi.mocked(api.applyLive).mock.calls.at(-1)![0] as Config;
    expect(onB.lines.filter((l) => l.kind === "Filter").length).toBe(1);
    expect(container.querySelector(".comparing")).toBeTruthy();
    expect(container.querySelector<HTMLButtonElement>(".undo-btn")!.disabled).toBe(true);
    expect(container.querySelector(".live")!.textContent).toContain("saved");

    // Toggle off → the working edit (2 bands) is restored live.
    vi.mocked(api.applyLive).mockClear();
    await fireEvent.click(compareBtn);
    await waitFor(() => expect(api.applyLive).toHaveBeenCalled());
    const onA = vi.mocked(api.applyLive).mock.calls.at(-1)![0] as Config;
    expect(onA.lines.filter((l) => l.kind === "Filter").length).toBe(2);
    expect(container.querySelector(".comparing")).toBeNull();
  });
});

describe("Editor loudness-matched compare", () => {
  it("volume-matches the sides, shows the offset, and lets the switch opt out", async () => {
    // Saved: a +6 dB peak at 3 kHz. The edit deletes it, so the working side
    // (flat) is audibly LOUDER than the saved side on its anti-clip preamp.
    const { container } = renderEditor(cfg(0, [[3000, 6, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));
    await fireEvent.click(container.querySelector(".band .remove")!);
    const compareBtn = container.querySelector<HTMLButtonElement>(".compare-btn")!;
    await waitFor(() => expect(compareBtn.disabled).toBe(false));

    // Enter compare: the saved side auditions on an injected anti-clip preamp
    // (the file had none), and the session arms — red Auto switch, offset label.
    vi.mocked(api.applyLive).mockClear();
    await fireEvent.click(compareBtn);
    await waitFor(() => expect(api.applyLive).toHaveBeenCalled());
    const onB = vi.mocked(api.applyLive).mock.calls.at(-1)![0] as Config;
    const bPre = onB.lines.find(
      (l) => l.kind === "Preamp" && l.value.channel.kind === "both",
    );
    expect(bPre && bPre.kind === "Preamp" && bPre.value.gain).toBeLessThan(0);
    // The saved side is the quieter one — no extra offset on it.
    expect(container.querySelector(".pside .sw-label")!.textContent).toBe("Auto (−0.0 dB)");

    // Flip back to the edit: it is the louder side, so it carries the extra
    // attenuation — a negative master preamp even though the edit's is 0.
    vi.mocked(api.applyLive).mockClear();
    await fireEvent.click(compareBtn);
    await waitFor(() => expect(api.applyLive).toHaveBeenCalled());
    const onA = vi.mocked(api.applyLive).mock.calls.at(-1)![0] as Config;
    const aPre = onA.lines.find(
      (l) => l.kind === "Preamp" && l.value.channel.kind === "both",
    );
    expect(aPre && aPre.kind === "Preamp" && aPre.value.gain).toBeLessThan(0);
    const label = container.querySelector(".pside .sw-label")!.textContent!;
    expect(label).toMatch(/^Auto \(−\d+\.\d dB\)$/);
    expect(label).not.toBe("Auto (−0.0 dB)"); // a real offset on the loud side

    // The switch opts out: raw preamps (the edit's true 0 dB — no preamp
    // line), plain "Auto" label again.
    vi.mocked(api.applyLive).mockClear();
    await fireEvent.click(container.querySelector(".pside .switch input")!);
    await waitFor(() => expect(api.applyLive).toHaveBeenCalled());
    const raw = vi.mocked(api.applyLive).mock.calls.at(-1)![0] as Config;
    expect(
      raw.lines.some((l) => l.kind === "Preamp" && l.value.channel.kind === "both"),
    ).toBe(false);
    expect(container.querySelector(".pside .sw-label")!.textContent).toBe("Auto");
  });

  it("identical-sounding sides match with a zero offset", async () => {
    // The edit only adds a 0 dB band — audibly identical, so matching applies
    // the same anti-clip preamp to both sides and no extra offset.
    const { container } = renderEditor(cfg(-10, [[3000, 6, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));
    await fireEvent.click(container.querySelector(".band-actions .add")!);
    const compareBtn = container.querySelector<HTMLButtonElement>(".compare-btn")!;
    await waitFor(() => expect(compareBtn.disabled).toBe(false));

    await fireEvent.click(compareBtn);
    await waitFor(() =>
      expect(container.querySelector(".pside .sw-label")!.textContent).toBe(
        "Auto (−0.0 dB)",
      ),
    );
    // Esc exits AND disarms the session: the label returns to plain Auto.
    await fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() =>
      expect(container.querySelector(".pside .sw-label")!.textContent).toBe("Auto"),
    );
  });
});

describe("Editor history browser", () => {
  const REV = { id: "1783300000000-save", savedAtMs: Date.now() - 2 * 60_000, op: "save" as const };

  it("lists revisions, auditions one matched, and returns to the edit", async () => {
    vi.mocked(api.presetHistory).mockResolvedValue([REV]);
    vi.mocked(api.getRevision).mockResolvedValue(cfg(0, [[500, 6, 1]]));
    const { container } = renderEditor(cfg(0, [[3000, 6, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));

    await fireEvent.click(container.querySelector(".hist-btn")!);
    await waitFor(() => expect(document.querySelector(".hist-menu .hist-item")).toBeTruthy());
    const item = document.querySelector(".hist-menu .hist-item")!;
    expect(item.textContent).toContain("v1"); // one revision -> the oldest is v1
    expect(item.textContent).toContain(longDate(REV.savedAtMs)); // creation date, not the op

    // Audition: the revision plays with an injected (matched) master preamp,
    // the editor locks like a compare, and the badge says history.
    vi.mocked(api.applyLive).mockClear();
    await fireEvent.click(item);
    await waitFor(() => expect(api.applyLive).toHaveBeenCalled());
    const onB = vi.mocked(api.applyLive).mock.calls.at(-1)![0] as Config;
    const filt = onB.lines.find((l) => l.kind === "Filter");
    expect(filt && filt.kind === "Filter" && filt.value.freq).toBe(500);
    const pre = onB.lines.find((l) => l.kind === "Preamp" && l.value.channel.kind === "both");
    expect(pre && pre.kind === "Preamp" && pre.value.gain).toBeLessThan(0);
    expect(container.querySelector(".live")!.textContent).toContain("history");
    expect(container.querySelector(".comparing")).toBeTruthy();

    // Second click: same revision at RAW levels — the injected matched preamp
    // is gone (this revision has none of its own) and the red matching is off.
    vi.mocked(api.applyLive).mockClear();
    await fireEvent.click(document.querySelector(".hist-menu .hist-item")!);
    await waitFor(() => expect(api.applyLive).toHaveBeenCalled());
    const raw = vi.mocked(api.applyLive).mock.calls.at(-1)![0] as Config;
    expect(
      raw.lines.some((l) => l.kind === "Preamp" && l.value.channel.kind === "both"),
    ).toBe(false);
    expect(container.querySelector(".live")!.textContent).toContain("history"); // still auditioning
    expect(container.querySelector(".pside .sw-label")!.textContent).toBe("Auto"); // matching off

    // Third click: back to the edit, dirty-free — and the opt-out reset, so a
    // fresh audition starts matched again.
    vi.mocked(api.applyLive).mockClear();
    await fireEvent.click(document.querySelector(".hist-menu .hist-item")!);
    await waitFor(() => expect(api.applyLive).toHaveBeenCalled());
    expect(container.querySelector(".live")!.textContent).toContain("live");
    expect(container.querySelector<HTMLButtonElement>(".primary")!.textContent).toContain("Saved"); // not dirtied

    vi.mocked(api.applyLive).mockClear();
    await fireEvent.click(document.querySelector(".hist-menu .hist-item")!);
    await waitFor(() => expect(api.applyLive).toHaveBeenCalled());
    const again = vi.mocked(api.applyLive).mock.calls.at(-1)![0] as Config;
    expect(
      again.lines.some((l) => l.kind === "Preamp" && l.value.channel.kind === "both"),
    ).toBe(true); // matched once more
  });

  it("Restore loads the revision as an unsaved edit; only Save persists it", async () => {
    vi.mocked(api.presetHistory).mockResolvedValue([REV]);
    vi.mocked(api.getRevision).mockResolvedValue(cfg(0, [[500, 6, 1]]));
    vi.mocked(api.restoreRevision).mockClear();
    vi.mocked(api.savePreset).mockClear();
    const { container } = renderEditor(cfg(0, [[3000, 6, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));

    await fireEvent.click(container.querySelector(".hist-btn")!);
    await waitFor(() => expect(document.querySelector(".hist-menu .hist-restore")).toBeTruthy());
    vi.mocked(api.applyLive).mockClear();
    await fireEvent.click(document.querySelector(".hist-menu .hist-restore")!);

    // Nothing was written: no backend restore, no save — the revision landed
    // in the editor as a dirty live edit instead.
    await waitFor(() => expect(api.applyLive).toHaveBeenCalled());
    expect(api.restoreRevision).not.toHaveBeenCalled();
    expect(api.savePreset).not.toHaveBeenCalled();
    const live = vi.mocked(api.applyLive).mock.calls.at(-1)![0] as Config;
    const filt = live.lines.find((l) => l.kind === "Filter");
    expect(filt && filt.kind === "Filter" && filt.value.freq).toBe(500);
    expect(document.querySelector(".hist-menu .hist-item")).toBeNull(); // menu closed
    const saveBtn = container.querySelector<HTMLButtonElement>(".primary")!;
    expect(saveBtn.textContent).toContain("Save"); // dirty
    expect(saveBtn.disabled).toBe(false);

    // Only the Save click persists it.
    await fireEvent.click(saveBtn);
    await waitFor(() => expect(api.savePreset).toHaveBeenCalled());
    const saved = vi.mocked(api.savePreset).mock.calls.at(-1)![1] as Config;
    const savedFilt = saved.lines.find((l) => l.kind === "Filter");
    expect(savedFilt && savedFilt.kind === "Filter" && savedFilt.value.freq).toBe(500);
  });
});
