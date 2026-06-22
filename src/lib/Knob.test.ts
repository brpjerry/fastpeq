// @vitest-environment happy-dom
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, fireEvent, cleanup } from "@testing-library/svelte";
import Knob from "./Knob.svelte";

afterEach(cleanup);

function renderKnob(value: number, onInput = vi.fn()) {
  const { container } = render(Knob, { props: { value, label: "Bass", onInput } });
  return { container, onInput, dial: container.querySelector<SVGElement>(".dial")! };
}

describe("Knob", () => {
  it("displays the signed value, unit, and label", () => {
    const { container } = renderKnob(3);
    expect(container.querySelector(".val")!.textContent).toContain("+3.0");
    expect(container.querySelector(".val")!.textContent).toContain("dB");
    expect(container.querySelector(".name")!.textContent).toBe("Bass");
  });

  it("steps up and down with arrow keys", async () => {
    const { dial, onInput } = renderKnob(0);
    await fireEvent.keyDown(dial, { key: "ArrowUp" });
    expect(onInput).toHaveBeenLastCalledWith(0.5);
    await fireEvent.keyDown(dial, { key: "ArrowDown" });
    expect(onInput).toHaveBeenLastCalledWith(-0.5);
  });

  it("steps with the scroll wheel", async () => {
    const { dial, onInput } = renderKnob(1);
    await fireEvent.wheel(dial, { deltaY: -1 });
    expect(onInput).toHaveBeenLastCalledWith(1.5);
  });

  it("resets to 0 on right-click", async () => {
    const { dial, onInput } = renderKnob(4);
    await fireEvent.contextMenu(dial);
    expect(onInput).toHaveBeenCalledWith(0);
  });

  it("clamps at the limit (no callback past max)", async () => {
    const { dial, onInput } = renderKnob(12); // default max
    await fireEvent.keyDown(dial, { key: "ArrowUp" });
    expect(onInput).not.toHaveBeenCalled();
  });
});
