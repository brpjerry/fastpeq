// @vitest-environment happy-dom
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, fireEvent, cleanup } from "@testing-library/svelte";
import PresetPicker from "./PresetPicker.svelte";

afterEach(cleanup);

const presets = ["64 Audio U12t", "Sennheiser HD600", "Bose QC"];
const categories: Record<string, string> = {
  "64 Audio U12t": "iem",
  "Sennheiser HD600": "headphone",
};

const openMenu = (container: ParentNode) => fireEvent.click(container.querySelector(".pp-btn")!);

describe("PresetPicker", () => {
  it("shows the selected preset on the trigger", () => {
    const { container } = render(PresetPicker, {
      props: { value: "Bose QC", presets, categories, onChange: vi.fn() },
    });
    expect(container.querySelector(".pp-label")?.textContent).toBe("Bose QC");
  });

  it("filters the list by the search query", async () => {
    const { container } = render(PresetPicker, {
      props: { value: "", presets, categories, onChange: vi.fn() },
    });
    await openMenu(container);
    expect(container.querySelectorAll(".pp-item").length).toBe(3);

    await fireEvent.input(container.querySelector(".pp-search")!, { target: { value: "senn" } });
    const items = [...container.querySelectorAll(".pp-item")];
    expect(items.length).toBe(1);
    expect(items[0].textContent).toContain("Sennheiser HD600");
  });

  it("filters by device type and selects a preset", async () => {
    const onChange = vi.fn();
    const { container } = render(PresetPicker, { props: { value: "", presets, categories, onChange } });
    await openMenu(container);

    const iemChip = [...container.querySelectorAll(".pp-chip")].find(
      (c) => c.getAttribute("title") === "iem",
    )!;
    await fireEvent.click(iemChip);
    const items = [...container.querySelectorAll(".pp-item")];
    expect(items.length).toBe(1);

    await fireEvent.click(items[0]);
    expect(onChange).toHaveBeenCalledWith("64 Audio U12t");
  });
});
