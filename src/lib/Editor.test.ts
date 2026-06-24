// @vitest-environment happy-dom
import { describe, it, expect, vi, afterEach } from "vitest";
import { tick } from "svelte";
import { render, fireEvent, cleanup, waitFor } from "@testing-library/svelte";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { Config } from "./types";
import * as api from "./api";
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
} from "./presetView.svelte";
import Editor from "./Editor.svelte";

// The Editor only touches these four IPC calls; an explicit mock is clearer than
// auto-mock and resolves the mutations to void by default.
vi.mock("./api", () => ({
  getPreset: vi.fn(),
  applyLive: vi.fn(() => Promise.resolve()),
  savePreset: vi.fn(() => Promise.resolve()),
  readTextFile: vi.fn(() => Promise.resolve("")),
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
});
