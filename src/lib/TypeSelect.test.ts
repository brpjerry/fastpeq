// @vitest-environment happy-dom
import { describe, it, expect, vi, afterEach } from "vitest";
import { tick } from "svelte";
import { render, fireEvent, cleanup } from "@testing-library/svelte";
import TypeSelect from "./TypeSelect.svelte";

afterEach(cleanup);

const items = (root: ParentNode) => [...root.querySelectorAll<HTMLElement>(".ts-item")];

describe("TypeSelect", () => {
  it("shows the current type's token", () => {
    const { container } = render(TypeSelect, { props: { value: "Peak", onChange: vi.fn() } });
    expect(container.querySelector(".ts-btn .tok")?.textContent).toBe("PK");
  });

  it("opens on click and selects a different type", async () => {
    const onChange = vi.fn();
    const { container } = render(TypeSelect, { props: { value: "Peak", onChange } });
    expect(container.querySelector(".ts-menu")).toBeNull();

    await fireEvent.click(container.querySelector(".ts-btn")!);
    expect(container.querySelector(".ts-menu")).not.toBeNull();

    const lowShelf = items(container).find(
      (b) => b.textContent!.includes("Low shelf") && !b.textContent!.includes("(Q)"),
    );
    await fireEvent.click(lowShelf!);
    expect(onChange).toHaveBeenCalledWith("LowShelf");
  });

  it("does not call onChange when re-picking the current type", async () => {
    const onChange = vi.fn();
    const { container } = render(TypeSelect, { props: { value: "Peak", onChange } });
    await fireEvent.click(container.querySelector(".ts-btn")!);
    const peaking = items(container).find((b) => b.textContent!.includes("Peaking"));
    await fireEvent.click(peaking!);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("closes on an outside pointerdown", async () => {
    const { container } = render(TypeSelect, { props: { value: "Peak", onChange: vi.fn() } });
    await fireEvent.click(container.querySelector(".ts-btn")!);
    expect(container.querySelector(".ts-menu")).not.toBeNull();

    document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    await tick();
    expect(container.querySelector(".ts-menu")).toBeNull();
  });
});
