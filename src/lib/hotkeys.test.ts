// @vitest-environment happy-dom
import { beforeEach, describe, it, expect, vi } from "vitest";
import * as api from "./api";
import {
  getHotkeys,
  initHotkeys,
  addHotkey,
  updateHotkey,
  removeHotkey,
  moveHotkey,
  validKey,
  accelerator,
  accelerators,
  duplicateIds,
  type Hotkey,
} from "./hotkeys.svelte";

vi.mock("./api", () => ({
  loadHotkeyBindings: vi.fn(() => Promise.resolve(null)),
  saveHotkeyBindings: vi.fn(() => Promise.resolve()),
}));

const KEY = "fastpeq.hotkeys";
const clearAll = () => getHotkeys().slice().forEach((h) => removeHotkey(h.id));

describe("hotkeys persistence", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.mocked(api.loadHotkeyBindings).mockResolvedValue(null);
    vi.mocked(api.saveHotkeyBindings).mockClear();
  });

  it("loads bindings from the backend file when one exists", async () => {
    const stored: Hotkey[] = [{ id: "h1", mod: "ctrl-shift", key: "K", action: "bypass" }];
    vi.mocked(api.loadHotkeyBindings).mockResolvedValue(JSON.stringify(stored));
    await initHotkeys();
    expect(getHotkeys()).toEqual(stored);
    // The file is authoritative — a plain load never writes anything back.
    expect(api.saveHotkeyBindings).not.toHaveBeenCalled();
  });

  it("migrates a pre-file localStorage list into the file when none exists", async () => {
    const legacy: Hotkey[] = [
      { id: "h2", mod: "ctrl-alt", key: "1", action: "preset", preset: "HD600" },
    ];
    localStorage.setItem(KEY, JSON.stringify(legacy));
    await initHotkeys();
    expect(getHotkeys()).toEqual(legacy);
    expect(api.saveHotkeyBindings).toHaveBeenCalledWith(JSON.stringify(legacy));
  });

  it("seeds a default Ctrl+Alt+B → Bypass binding on a true first run", async () => {
    await initHotkeys();
    expect(
      getHotkeys().some((h) => h.action === "bypass" && h.mod === "ctrl-alt" && h.key === "B"),
    ).toBe(true);
    expect(api.saveHotkeyBindings).toHaveBeenCalled(); // the seed lands in the file
  });

  it("never overwrites an unreadable file with a seed", async () => {
    vi.mocked(api.loadHotkeyBindings).mockResolvedValue("{corrupt");
    await initHotkeys();
    expect(getHotkeys()).toEqual([]); // starts empty, but the file is left alone
    expect(api.saveHotkeyBindings).not.toHaveBeenCalled();
  });

  it("respects an intentionally emptied list (no re-seed)", async () => {
    vi.mocked(api.loadHotkeyBindings).mockResolvedValue("[]");
    await initHotkeys();
    expect(getHotkeys()).toEqual([]);
    expect(api.saveHotkeyBindings).not.toHaveBeenCalled();
  });

  it("persists every edit to the file and the localStorage backup", async () => {
    await initHotkeys();
    clearAll();
    vi.mocked(api.saveHotkeyBindings).mockClear();
    const a = addHotkey();
    expect(api.saveHotkeyBindings).toHaveBeenCalledTimes(1);
    expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual(getHotkeys());
    removeHotkey(a);
  });
});

describe("hotkeys store", () => {
  it("adds, updates, removes and reorders bindings", () => {
    clearAll();
    const a = addHotkey();
    const b = addHotkey();
    expect(getHotkeys().length).toBe(2);

    updateHotkey(a, { key: "1", action: "preset", preset: "HD600" });
    expect(getHotkeys().find((h) => h.id === a)!.preset).toBe("HD600");

    // Move b to the front.
    moveHotkey(getHotkeys().findIndex((h) => h.id === b), 0);
    expect(getHotkeys()[0].id).toBe(b);

    removeHotkey(a);
    removeHotkey(b);
    expect(getHotkeys().length).toBe(0);
  });

  it("validates keys as a single letter or digit", () => {
    expect(validKey("A")).toBe(true);
    expect(validKey("7")).toBe(true);
    expect(validKey("a")).toBe(false); // must be uppercased first
    expect(validKey("")).toBe(false);
    expect(validKey("AB")).toBe(false);
    expect(validKey("+")).toBe(false);
  });

  it("builds an accelerator string per modifier", () => {
    expect(accelerator({ id: "x", mod: "ctrl-alt", key: "B", action: "bypass" })).toBe("Ctrl+Alt+B");
    expect(accelerator({ id: "y", mod: "ctrl-shift", key: "1", action: "bypass" })).toBe("Ctrl+Shift+1");
    const bad: Hotkey = { id: "z", mod: "ctrl-alt", key: "", action: "bypass" };
    expect(accelerator(bad)).toBeNull();
  });

  it("treats a device binding like any other for accelerator generation", () => {
    clearAll();
    const id = addHotkey();
    updateHotkey(id, { mod: "ctrl-alt", key: "D", action: "device", device: "{0.0.0}.{dac}" });
    expect(accelerators()).toEqual([{ id, accelerator: "Ctrl+Alt+D" }]);
    clearAll();
  });

  it("emits accelerators, skipping invalid keys and duplicate combos", () => {
    clearAll();
    const a = addHotkey();
    const b = addHotkey();
    const c = addHotkey();
    updateHotkey(a, { mod: "ctrl-alt", key: "1" });
    updateHotkey(b, { mod: "ctrl-alt", key: "" }); // invalid → skipped
    updateHotkey(c, { mod: "ctrl-alt", key: "1" }); // duplicate of a → skipped

    const accs = accelerators();
    expect(accs).toEqual([{ id: a, accelerator: "Ctrl+Alt+1" }]);
    clearAll();
  });

  it("flags later bindings that shadow an earlier combo, ignoring invalid keys", () => {
    clearAll();
    const a = addHotkey();
    const b = addHotkey();
    const c = addHotkey();
    const d = addHotkey();
    updateHotkey(a, { mod: "ctrl-alt", key: "F" });
    updateHotkey(b, { mod: "ctrl-alt", key: "F" }); // duplicate of a → flagged
    updateHotkey(c, { mod: "ctrl-shift", key: "F" }); // different modifier → fine
    updateHotkey(d, { mod: "ctrl-alt", key: "" }); // invalid key → not a duplicate

    const dups = duplicateIds();
    expect([...dups]).toEqual([b]); // only the later, shadowed binding
    expect(dups.has(a)).toBe(false);
    expect(dups.has(c)).toBe(false);
    expect(dups.has(d)).toBe(false);
    clearAll();
  });
});
