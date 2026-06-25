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
  onDrop: vi.fn(),
});

const renderRow = (hotkey: Hotkey, presets: string[] = []) =>
  render(HotkeyRow, { props: { hotkey, index: 0, presets, ...cbs() } });

describe("HotkeyRow", () => {
  it("shows no principal for the bypass action", () => {
    const { container } = renderRow(base());
    expect(container.querySelector(".principal .none")?.textContent).toContain("—");
  });

  it("shows the preset picker for the preset action", () => {
    const { container } = renderRow(base({ action: "preset", preset: "A" }), ["A", "B"]);
    expect(container.querySelector(".principal .sm-label")?.textContent).toBe("A");
  });

  it("shows the tone picker for tone actions", () => {
    const { container } = renderRow(base({ action: "tone-up", tone: "treble" }));
    expect(container.querySelector(".principal .sm-label")?.textContent).toBe("Treble");
  });

  it("normalizes the key to uppercase and reports the change", async () => {
    const c = cbs();
    const { container } = render(HotkeyRow, {
      props: { hotkey: base({ key: "" }), index: 0, presets: [], ...c },
    });
    await fireEvent.input(container.querySelector(".key-input")!, { target: { value: "x" } });
    expect(c.onUpdate).toHaveBeenCalledWith({ key: "X" });
  });

  it("seeds a default preset when switching to the preset action", async () => {
    const c = cbs();
    const { container } = render(HotkeyRow, {
      props: { hotkey: base({ action: "bypass" }), index: 0, presets: ["First", "Second"], ...c },
    });
    // sm-btn order: [0] modifier, [1] action (no principal picker while bypass).
    await fireEvent.click(container.querySelectorAll(".sm-btn")[1]);
    const item = [...container.querySelectorAll(".sm-menu .sm-item")].find(
      (i) => i.textContent!.trim() === "Switch preset",
    )!;
    await fireEvent.click(item);
    expect(c.onUpdate).toHaveBeenCalledWith({ action: "preset", preset: "First" });
  });

  it("fires onRemove from the delete button", async () => {
    const c = cbs();
    const { container } = render(HotkeyRow, {
      props: { hotkey: base(), index: 0, presets: [], ...c },
    });
    await fireEvent.click(container.querySelector(".hk-remove")!);
    expect(c.onRemove).toHaveBeenCalled();
  });

  it("flags a failed registration", () => {
    const { container } = render(HotkeyRow, {
      props: { hotkey: base(), index: 0, presets: [], failed: true, ...cbs() },
    });
    expect(container.querySelector(".warn")).not.toBeNull();
    expect(container.querySelector(".hk-row")!.classList.contains("failed")).toBe(true);
  });
});
