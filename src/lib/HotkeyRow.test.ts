// @vitest-environment happy-dom
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, fireEvent, cleanup } from "@testing-library/svelte";
import HotkeyRow from "./HotkeyRow.svelte";
import type { Hotkey } from "./hotkeys.svelte";

afterEach(cleanup);

const base = (over: Partial<Hotkey> = {}): Hotkey => ({
  id: "h1",
  mod: "ctrl-alt",
  key: "B",
  action: "bypass",
  ...over,
});

const cbs = () => ({
  onUpdate: vi.fn(),
  onRemove: vi.fn(),
  onDragStart: vi.fn(),
});

const renderRow = (
  hotkey: Hotkey,
  presets: string[] = [],
  categories: Record<string, string> = {},
  extra: Record<string, unknown> = {},
) => render(HotkeyRow, { props: { hotkey, index: 0, presets, categories, ...cbs(), ...extra } });

describe("HotkeyRow", () => {
  it("shows no principal for the bypass action", () => {
    const { container } = renderRow(base());
    expect(container.querySelector(".principal .none")?.textContent).toContain("—");
  });

  it("shows the searchable preset picker for the preset action", () => {
    const { container } = renderRow(base({ action: "preset", preset: "A" }), ["A", "B"]);
    expect(container.querySelector(".principal .pp-label")?.textContent).toBe("A");
  });

  it("shows the tone picker for tone actions", () => {
    const { container } = renderRow(base({ action: "tone-up", tone: "treble" }));
    expect(container.querySelector(".principal .sm-label")?.textContent).toBe("Treble");
  });

  it("normalizes the key to uppercase and reports the change", async () => {
    const onUpdate = vi.fn();
    const { container } = renderRow(base({ key: "" }), [], {}, { onUpdate });
    await fireEvent.input(container.querySelector(".key-input")!, { target: { value: "x" } });
    expect(onUpdate).toHaveBeenCalledWith({ key: "X" });
  });

  it("seeds a default preset when switching to the preset action", async () => {
    const onUpdate = vi.fn();
    const { container } = renderRow(base({ action: "bypass" }), ["First", "Second"], {}, { onUpdate });
    // sm-btn order: [0] modifier, [1] action (no principal picker while bypass).
    await fireEvent.click(container.querySelectorAll(".sm-btn")[1]);
    const item = [...container.querySelectorAll(".sm-menu .sm-item")].find(
      (i) => i.textContent!.trim() === "Switch preset",
    )!;
    await fireEvent.click(item);
    expect(onUpdate).toHaveBeenCalledWith({ action: "preset", preset: "First" });
  });

  it("shows the output-device picker for the device action", () => {
    const devices = [{ id: "d1", name: "Speakers", is_default: true }];
    const { container } = renderRow(base({ action: "device", device: "d1" }), [], {}, { devices });
    expect(container.querySelector(".principal .sm-label")?.textContent).toBe("Speakers");
  });

  it("seeds the first device when switching to the device action", async () => {
    const onUpdate = vi.fn();
    const devices = [
      { id: "d1", name: "Speakers", is_default: true },
      { id: "d2", name: "USB DAC", is_default: false },
    ];
    const { container } = renderRow(base({ action: "bypass" }), [], {}, { onUpdate, devices });
    await fireEvent.click(container.querySelectorAll(".sm-btn")[1]); // action menu
    const item = [...container.querySelectorAll(".sm-menu .sm-item")].find(
      (i) => i.textContent!.trim() === "Switch output device",
    )!;
    await fireEvent.click(item);
    expect(onUpdate).toHaveBeenCalledWith({ action: "device", device: "d1", deviceName: "Speakers" });
  });

  it("marks a bound device that is no longer present as unavailable", () => {
    const devices = [{ id: "d1", name: "Speakers", is_default: true }];
    const { container } = renderRow(
      base({ action: "device", device: "gone", deviceName: "Old DAC" }),
      [],
      {},
      { devices },
    );
    expect(container.querySelector(".principal .sm-label")?.textContent).toBe("Old DAC (unavailable)");
  });

  it("fires onRemove from the delete button", async () => {
    const onRemove = vi.fn();
    const { container } = renderRow(base(), [], {}, { onRemove });
    await fireEvent.click(container.querySelector(".hk-remove")!);
    expect(onRemove).toHaveBeenCalled();
  });

  it("flags a failed registration", () => {
    const { container } = renderRow(base(), [], {}, { failed: true });
    expect(container.querySelector(".warn")).not.toBeNull();
    expect(container.querySelector(".hk-row")!.classList.contains("failed")).toBe(true);
  });

  it("flags a duplicate combo with its own warning", () => {
    const { container } = renderRow(base(), [], {}, { duplicate: true });
    expect(container.querySelector(".hk-row")!.classList.contains("duplicate")).toBe(true);
    expect(container.querySelector(".warn")!.getAttribute("title")).toContain("duplicates another hotkey");
  });
});
