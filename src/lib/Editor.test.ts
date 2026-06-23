// @vitest-environment happy-dom
import { describe, it, expect, vi, afterEach } from "vitest";
import { tick } from "svelte";
import { render, fireEvent, cleanup, waitFor } from "@testing-library/svelte";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { Config } from "./types";
import * as api from "./api";
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

  it("imports a REW measurement in the expanded view", async () => {
    vi.mocked(openDialog).mockResolvedValue("C:/curves/harman.txt");
    vi.mocked(api.readTextFile).mockResolvedValue("20 -3\n500 0\n1000 0.5\n10000 -2");

    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));

    await fireEvent.click(container.querySelector(".expand-btn")!);
    const importBtn = await waitFor(() => {
      const b = [...container.querySelectorAll("button")].find((x) =>
        x.textContent!.includes("Import REW"),
      );
      if (!b) throw new Error("import button not rendered yet");
      return b;
    });

    await fireEvent.click(importBtn);
    await waitFor(() => expect(container.querySelector(".meas-name")?.textContent).toContain("harman.txt"));
  });

  it("wraps the expanded graph in the fixed-aspect container", async () => {
    const { container } = renderEditor(cfg(-10, [[1000, 0, 1]]));
    await waitFor(() => expect(bandCount(container)).toBe(1));
    await fireEvent.click(container.querySelector(".expand-btn")!);
    await waitFor(() => expect(container.querySelector(".graph-fit")).toBeTruthy());
    expect(container.querySelector(".graph-fit .ce-wrap")).toBeTruthy();
  });
});
