// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, vi } from "vitest";
import * as api from "./api";
import { ACCENTS, currentAccentId, applyAccent, setAccent, initTheme } from "./theme";

vi.mock("./api", () => ({
  loadUiState: vi.fn(() => Promise.resolve(null)),
  saveUiState: vi.fn(() => Promise.resolve()),
}));

const KEY = "fastpeq.accent";
const accentVar = () => document.documentElement.style.getPropertyValue("--accent");
const accentOf = (id: string) => ACCENTS.find((a) => a.id === id)!.accent;

beforeEach(() => {
  localStorage.clear();
  document.documentElement.style.removeProperty("--accent");
  document.documentElement.style.removeProperty("--accent-2");
  vi.mocked(api.loadUiState).mockResolvedValue(null);
  vi.mocked(api.saveUiState).mockClear();
});

describe("theme", () => {
  it("defaults the cached accent to blue", () => {
    expect(currentAccentId()).toBe("blue");
  });

  it("applyAccent only recolors — the startup re-apply must never persist", () => {
    applyAccent("teal");
    expect(accentVar()).toBe(accentOf("teal"));
    expect(localStorage.getItem(KEY)).toBeNull();
    expect(api.saveUiState).not.toHaveBeenCalled();
  });

  it("setAccent applies and persists to the file and the localStorage cache", () => {
    setAccent("rose");
    expect(accentVar()).toBe(accentOf("rose"));
    expect(localStorage.getItem(KEY)).toBe("rose");
    expect(api.saveUiState).toHaveBeenCalledWith("theme", JSON.stringify({ accent: "rose" }));
  });

  it("initTheme re-applies the accent from the backend file and refreshes the cache", async () => {
    vi.mocked(api.loadUiState).mockResolvedValue(JSON.stringify({ accent: "purple" }));
    await initTheme();
    expect(accentVar()).toBe(accentOf("purple"));
    expect(localStorage.getItem(KEY)).toBe("purple");
    // The file is authoritative — a plain load never writes anything back.
    expect(api.saveUiState).not.toHaveBeenCalled();
  });

  it("migrates the cached accent into the file when none exists", async () => {
    localStorage.setItem(KEY, "orange");
    await initTheme();
    expect(api.saveUiState).toHaveBeenCalledWith("theme", JSON.stringify({ accent: "orange" }));
  });

  it("never overwrites an unreadable file and keeps the cached accent applied", async () => {
    applyAccent("teal"); // what the pre-mount cached apply did
    vi.mocked(api.loadUiState).mockResolvedValue("{corrupt");
    await initTheme();
    expect(accentVar()).toBe(accentOf("teal"));
    expect(api.saveUiState).not.toHaveBeenCalled();
  });
});
