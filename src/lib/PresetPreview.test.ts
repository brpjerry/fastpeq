// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, fireEvent, cleanup, waitFor } from "@testing-library/svelte";
import PresetPreview from "./PresetPreview.svelte";
import * as api from "./api";
import type { Config } from "./types";

vi.mock("./api", () => ({ getPreset: vi.fn() }));

const both = { kind: "both" as const };

const config: Config = {
  lines: [
    { kind: "Preamp", value: { gain: -6, channel: both } },
    {
      kind: "Filter",
      value: { enabled: true, kind: "Peak", freq: 120, gain: 4.5, q: 1.2, index: 1, channel: both },
    },
    {
      kind: "Filter",
      value: { enabled: false, kind: "Peak", freq: 3000, gain: -3, q: 2, index: 2, channel: both },
    },
    {
      kind: "Filter",
      value: { enabled: true, kind: "HighPass", freq: 30, gain: null, q: null, index: 3, channel: both },
    },
  ],
};

const props = (over: Record<string, unknown> = {}) => ({
  name: "Sennheiser HD600",
  onClose: vi.fn(),
  onApply: vi.fn(),
  onDelete: vi.fn(),
  ...over,
});

const rows = (root: ParentNode) => [...root.querySelectorAll(".prow")];

beforeEach(() => {
  vi.mocked(api.getPreset).mockResolvedValue(config);
});
afterEach(cleanup);

describe("PresetPreview", () => {
  it("lists only the enabled filters, with type, frequency, gain and Q", async () => {
    const { container } = render(PresetPreview, { props: props() });
    await waitFor(() => expect(rows(container).length).toBe(2));

    const peak = rows(container)[0];
    expect(peak.querySelector(".tok")!.textContent).toBe("PK");
    expect(peak.querySelector(".freq")!.textContent).toContain("120");
    expect(peak.querySelector(".gain")!.textContent).toContain("+4.5");
    expect(peak.querySelector(".q")!.textContent).toContain("1.20");

    // The disabled 3 kHz band isn't listed at all.
    expect(container.textContent).not.toContain("3000");
  });

  it("draws a handle-less gain bar and nothing editable", async () => {
    const { container } = render(PresetPreview, { props: props() });
    await waitFor(() => expect(rows(container).length).toBe(2));

    // Every row has a bar track; only the gain-carrying filter gets a fill.
    expect(container.querySelectorAll(".bar-track").length).toBe(2);
    expect(container.querySelectorAll(".bar-fill").length).toBe(1);
    // View-only: no inputs, sliders, or checkboxes anywhere in the pane.
    expect(container.querySelectorAll("input, select, textarea").length).toBe(0);
  });

  it("closes on the back button, on Escape, and on a click outside the pane", async () => {
    const onClose = vi.fn();
    const { container } = render(PresetPreview, { props: props({ onClose }) });
    await waitFor(() => expect(rows(container).length).toBe(2));

    await fireEvent.click(container.querySelector(".back")!);
    expect(onClose).toHaveBeenCalledTimes(1);

    await fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(2);

    await fireEvent.click(container.querySelector(".preview-backdrop")!);
    expect(onClose).toHaveBeenCalledTimes(3);

    // A click that lands inside the pane (here, its own header) stays put.
    await fireEvent.click(container.querySelector(".preview-head")!);
    expect(onClose).toHaveBeenCalledTimes(3);
  });

  it("hands Apply and Delete back to the caller", async () => {
    const onApply = vi.fn();
    const onDelete = vi.fn();
    const { container, getByText } = render(PresetPreview, { props: props({ onApply, onDelete }) });
    await waitFor(() => expect(rows(container).length).toBe(2));

    await fireEvent.click(getByText("Apply"));
    expect(onApply).toHaveBeenCalledWith("Sennheiser HD600");

    await fireEvent.click(getByText("Delete"));
    expect(onDelete).toHaveBeenCalledWith("Sennheiser HD600");
  });

  it("says so when the preset has no filters on", async () => {
    vi.mocked(api.getPreset).mockResolvedValue({ lines: [] });
    const { container } = render(PresetPreview, { props: props() });
    await waitFor(() => expect(container.querySelector(".none")).toBeTruthy());
    expect(container.querySelector(".none")!.textContent).toContain("No filters are on");
  });

  it("surfaces a read failure instead of an empty graph", async () => {
    vi.mocked(api.getPreset).mockRejectedValue("preset file missing");
    const { container } = render(PresetPreview, { props: props() });
    await waitFor(() => expect(container.querySelector(".err")).toBeTruthy());
    expect(container.querySelector(".err")!.textContent).toContain("preset file missing");
    // Applying a preset we couldn't read is blocked.
    expect(container.querySelector<HTMLButtonElement>("button.primary")!.disabled).toBe(true);
  });
});
