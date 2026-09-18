import { describe, expect, it } from "vitest";
import { resolveAudioDevice } from "./audio-device";

const device = (id: string, name: string, is_default = false) => ({ id, name, is_default });

describe("audio hotkey device recovery", () => {
  it("keeps an available endpoint id even if its name changed or is duplicated", () => {
    const current = device("saved", "Renamed DAC");
    expect(resolveAudioDevice("saved", "USB DAC", [device("other", "USB DAC"), current])).toBe(current);
    expect(resolveAudioDevice("saved", undefined, [current])).toBe(current);
  });

  it.each([
    ["Speakers (USB DAC)", "Speakers (USB DAC)"],
    ["  speakers  (USB DAC) ", "Speakers (usb dac)"],
    ["Speakers (USB DAC)", "Speakers (2- USB DAC)"],
    ["Speakers (2- USB DAC)", "Speakers (3- USB DAC)"],
    ["2- USB DAC", "USB DAC"],
  ])("recovers %s as %s", (saved, name) => {
    const current = device("new", name);
    expect(resolveAudioDevice("old", saved, [device("other", "HDMI"), current])).toBe(current);
  });

  it("prefers a full-name match over an instance-number match", () => {
    const exact = device("exact", "Speakers (2- USB DAC)");
    expect(resolveAudioDevice("old", exact.name, [device("other", "Speakers (USB DAC)"), exact])).toBe(exact);
  });

  it.each([
    ["USB DAC", "USB DAC", "usb dac"],
    ["Speakers (USB DAC)", "Speakers (2- USB DAC)", "Speakers (3- USB DAC)"],
  ])("rejects ambiguous matches for %s even when one is default", (saved, first, second) => {
    expect(() => resolveAudioDevice("old", saved, [device("a", first, true), device("b", second)]))
      .toThrow("Multiple audio outputs match");
  });

  it.each([undefined, "", "  ", "DAC", "USB DAC Pro", "USB DAC 2"])(
    "does not guess from a missing, partial or different model name: %s", (name) => {
      expect(() => resolveAudioDevice("old", name, [device("new", "USB DAC")])).toThrow("unavailable");
    },
  );

  it("leaves an unplugged device unresolved", () => {
    expect(() => resolveAudioDevice("old", "USB DAC", [])).toThrow("unavailable");
  });
});
