// @vitest-environment happy-dom
import { describe, it, expect, beforeEach } from "vitest";
import { anchorBelow } from "./floating";

const elWith = (rect: { left: number; top?: number; bottom: number; width: number }) =>
  ({ getBoundingClientRect: () => ({ top: rect.bottom - 20, ...rect }) }) as unknown as HTMLElement;

describe("anchorBelow", () => {
  beforeEach(() => {
    window.innerWidth = 1000;
    window.innerHeight = 1000;
  });

  it("places the menu flush under the trigger when in top 80%", () => {
    expect(anchorBelow(elWith({ left: 100, bottom: 50, width: 80 }))).toEqual({
      left: 100,
      top: 54,
      minWidth: 80,
      maxHeight: 938,
    });
  });

  it("places the menu flush above the trigger when in bottom 20%", () => {
    expect(anchorBelow(elWith({ left: 100, bottom: 900, width: 80 }))).toEqual({
      left: 100,
      bottom: 124, // 1000 - 880 + 4
      minWidth: 80,
      maxHeight: 868, // 880 - 12
    });
  });

  it("honours a minimum width", () => {
    expect(anchorBelow(elWith({ left: 100, bottom: 50, width: 40 }), 150).minWidth).toBe(150);
  });

  it("clamps the left edge so a wide menu stays on-screen", () => {
    window.innerWidth = 300;
    // left would be 280, but the 150-wide menu would overflow → 300 - 150 - 8.
    expect(anchorBelow(elWith({ left: 280, bottom: 20, width: 40 }), 150).left).toBe(142);
  });

  it("never goes left of the 8px margin", () => {
    window.innerWidth = 100;
    expect(anchorBelow(elWith({ left: -50, bottom: 20, width: 40 }), 200).left).toBe(8);
  });
});
