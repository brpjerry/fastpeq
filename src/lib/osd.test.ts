import { describe, it, expect } from "vitest";
import { payloadForHotkey, OSD_EVENT, type OsdContext } from "./osd";
import type { Hotkey } from "./hotkeys.svelte";

const hk = (over: Partial<Hotkey>): Hotkey => ({
  id: "h",
  mod: "ctrl-alt",
  key: "X",
  action: "bypass",
  ...over,
});
const ctx = (over: Partial<OsdContext> = {}): OsdContext => ({
  tone: { bass: 0, mid: 0, treble: 0 },
  bypassed: false,
  ...over,
});

describe("payloadForHotkey", () => {
  it("bypass reflects the resulting state", () => {
    expect(payloadForHotkey(hk({ action: "bypass" }), ctx({ bypassed: true }))).toEqual({
      title: "Bypass",
      detail: "Filters off",
    });
    expect(payloadForHotkey(hk({ action: "bypass" }), ctx({ bypassed: false }))).toEqual({
      title: "Bypass",
      detail: "EQ on",
    });
  });

  it("preset shows the resolved name, or nothing when unresolved", () => {
    expect(
      payloadForHotkey(hk({ action: "preset", preset: "HD600" }), ctx({ presetName: "HD600" })),
    ).toEqual({ title: "Preset", detail: "HD600" });
    expect(payloadForHotkey(hk({ action: "preset", preset: "Gone" }), ctx())).toBeNull();
  });

  it("device shows the cached name, falling back to the id, or nothing", () => {
    expect(
      payloadForHotkey(hk({ action: "device", device: "id1", deviceName: "USB DAC" }), ctx()),
    ).toEqual({ title: "Output device", detail: "USB DAC" });
    expect(payloadForHotkey(hk({ action: "device", device: "id1" }), ctx())).toEqual({
      title: "Output device",
      detail: "id1",
    });
    expect(payloadForHotkey(hk({ action: "device" }), ctx())).toBeNull();
  });

  it("tone up/down show a signed dB detail and a ±12 bar", () => {
    expect(
      payloadForHotkey(hk({ action: "tone-up", tone: "treble" }), ctx({ tone: { bass: 0, mid: 0, treble: 3 } })),
    ).toEqual({ title: "Treble", detail: "+3.0 dB", bar: { value: 3, min: -12, max: 12 } });
    expect(
      payloadForHotkey(hk({ action: "tone-down", tone: "bass" }), ctx({ tone: { bass: -1.5, mid: 0, treble: 0 } })),
    ).toEqual({ title: "Bass", detail: "-1.5 dB", bar: { value: -1.5, min: -12, max: 12 } });
  });

  it("tone-reset is a simple label", () => {
    expect(payloadForHotkey(hk({ action: "tone-reset" }), ctx())).toEqual({
      title: "Tone",
      detail: "Reset",
    });
  });

  it("exposes the shared event name", () => {
    expect(OSD_EVENT).toBe("osd:show");
  });
});
