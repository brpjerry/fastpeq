// @vitest-environment happy-dom
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, fireEvent, cleanup } from "@testing-library/svelte";
import SelectMenu from "./SelectMenu.svelte";

afterEach(cleanup);

const options = [
  { value: "a", label: "Flat" },
  { value: "b", label: "Harman" },
  { value: "c", label: "Bright" },
];

const items = (root: ParentNode) => [...root.querySelectorAll<HTMLElement>(".sm-item")];

describe("SelectMenu", () => {
  it("shows the current option's label on the trigger", () => {
    const { container } = render(SelectMenu, { props: { value: "b", options, onChange: vi.fn() } });
    expect(container.querySelector(".sm-label")?.textContent).toBe("Harman");
  });

  it("opens on click and selects a different option", async () => {
    const onChange = vi.fn();
    const { container } = render(SelectMenu, { props: { value: "a", options, onChange } });
    expect(container.querySelector(".sm-menu")).toBeNull();

    await fireEvent.click(container.querySelector(".sm-btn")!);
    expect(container.querySelector(".sm-menu")).not.toBeNull();
    expect(items(container).map((i) => i.textContent!.trim())).toEqual(["Flat", "Harman", "Bright"]);

    await fireEvent.click(items(container).find((i) => i.textContent!.trim() === "Bright")!);
    expect(onChange).toHaveBeenCalledWith("c");
  });

  it("does not call onChange when re-picking the current option", async () => {
    const onChange = vi.fn();
    const { container } = render(SelectMenu, { props: { value: "a", options, onChange } });
    await fireEvent.click(container.querySelector(".sm-btn")!);
    await fireEvent.click(items(container).find((i) => i.textContent!.trim() === "Flat")!);
    expect(onChange).not.toHaveBeenCalled();
  });

  // Outside-click dismissal is the shared FloatingMenu + dismissable behaviour,
  // covered by dismiss.test.ts and TypeSelect's equivalent; not re-tested here.

  it("falls back to the raw value when no option matches", () => {
    const { container } = render(SelectMenu, { props: { value: "missing", options, onChange: vi.fn() } });
    expect(container.querySelector(".sm-label")?.textContent).toBe("missing");
  });
});
