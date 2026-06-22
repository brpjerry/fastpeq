// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, fireEvent, cleanup, waitFor } from "@testing-library/svelte";
import * as api from "./lib/api";
import App from "./App.svelte";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("./lib/api", () => {
  const ok = () => vi.fn(() => Promise.resolve());
  const report = () => vi.fn(() => Promise.resolve({ imported: [], skipped: [], ignored: 0 }));
  return {
    // Data reads — configured per test below.
    apoStatus: vi.fn(),
    presetsDir: vi.fn(),
    listPresets: vi.fn(),
    presetCategories: vi.fn(),
    activePreset: vi.fn(),
    bypassed: vi.fn(),
    getTone: vi.fn(),
    getPreset: vi.fn(() => Promise.resolve({ lines: [] })),
    // Mutations — resolve to void / a report.
    applyPreset: ok(),
    toggleBypass: ok(),
    captureCurrent: ok(),
    deletePreset: ok(),
    renamePreset: ok(),
    setCategory: ok(),
    savePreset: ok(),
    applyLive: ok(),
    setTone: ok(),
    readTextFile: vi.fn(() => Promise.resolve("")),
    setPresetsDir: ok(),
    resetPresetsDir: ok(),
    openPresetsDir: ok(),
    importPeacePresets: report(),
    importPeaceFiles: report(),
  };
});

const FLAT_TONE = { bass: 0, mid: 0, treble: 0, invert: false, swap: false };
const rows = (root: ParentNode) => [...root.querySelectorAll(".presets li:not(.empty)")];

beforeEach(() => {
  vi.mocked(api.apoStatus).mockResolvedValue({ installed: true, config_path: "C:/config.txt", error: null });
  vi.mocked(api.presetsDir).mockResolvedValue("C:/presets");
  vi.mocked(api.bypassed).mockResolvedValue(false);
  vi.mocked(api.getTone).mockResolvedValue(FLAT_TONE);
  vi.mocked(api.activePreset).mockResolvedValue(null); // no active preset → no Editor rendered
  vi.mocked(api.listPresets).mockResolvedValue([]);
  vi.mocked(api.presetCategories).mockResolvedValue({});
});
afterEach(cleanup);

function withLibrary() {
  vi.mocked(api.listPresets).mockResolvedValue(["64 Audio U12t", "Sennheiser HD600"]);
  vi.mocked(api.presetCategories).mockResolvedValue({
    "64 Audio U12t": "iem",
    "Sennheiser HD600": "headphone",
  });
}

describe("App preset list", () => {
  it("renders presets from the backend", async () => {
    withLibrary();
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));
    expect(container.textContent).toContain("64 Audio U12t");
    expect(container.textContent).toContain("Sennheiser HD600");
  });

  it("filters by search query", async () => {
    withLibrary();
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));
    await fireEvent.input(container.querySelector(".search")!, { target: { value: "64" } });
    expect(rows(container).length).toBe(1);
    expect(rows(container)[0].textContent).toContain("64 Audio U12t");
  });

  it("filters by device type via the icon dropdown", async () => {
    withLibrary();
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));

    await fireEvent.click(container.querySelector(".type-trigger")!);
    const menu = container.querySelector(".type-menu");
    expect(menu).toBeTruthy();

    const headphone = [...menu!.querySelectorAll(".cat-menu-item")].find(
      (b) => b.textContent!.trim() === "Headphone",
    );
    await fireEvent.click(headphone!);

    expect(rows(container).length).toBe(1);
    expect(rows(container)[0].textContent).toContain("Sennheiser HD600");
  });

  it("only offers device types that some preset uses", async () => {
    withLibrary(); // iem + headphone only
    const { container } = render(App);
    await waitFor(() => expect(rows(container).length).toBe(2));
    await fireEvent.click(container.querySelector(".type-trigger")!);
    const labels = [...container.querySelectorAll(".type-menu .cat-menu-item")].map((b) =>
      b.textContent!.trim(),
    );
    expect(labels).toContain("All types");
    expect(labels).toContain("Headphone");
    expect(labels).toContain("IEM");
    expect(labels).not.toContain("Speaker"); // no speaker presets exist
  });
});
