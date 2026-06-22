// @vitest-environment happy-dom
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, fireEvent, cleanup, waitFor } from "@testing-library/svelte";
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
});
