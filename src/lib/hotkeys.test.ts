// @vitest-environment happy-dom
import { describe, it, expect } from "vitest";
import {
  getHotkeys,
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

const clearAll = () => getHotkeys().slice().forEach((h) => removeHotkey(h.id));

describe("hotkeys store", () => {
  // Runs first (in-file order), before any test clears the list — verifies the
  // one-time seed applied on import.
  it("seeds a default Ctrl+Alt+B → Bypass binding on first run", () => {
    expect(
      getHotkeys().some((h) => h.action === "bypass" && h.mod === "ctrl-alt" && h.key === "B"),
    ).toBe(true);
  });

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
