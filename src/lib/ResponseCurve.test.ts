// @vitest-environment happy-dom
import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup } from "@testing-library/svelte";
import ResponseCurve from "./ResponseCurve.svelte";

afterEach(cleanup);

describe("ResponseCurve", () => {
  it("draws the filter trace and no reference without a measurement", () => {
    const { container } = render(ResponseCurve, { props: { filters: [], preamp: 0 } });
    expect(container.querySelector(".resp.left")).toBeTruthy();
    expect(container.querySelector(".resp.reference")).toBeNull();
  });

  it("overlays a measurement reference when one is supplied", () => {
    const { container } = render(ResponseCurve, {
      props: {
        filters: [],
        preamp: 0,
        measurement: [
          { freq: 100, spl: 3 },
          { freq: 1000, spl: 0 },
        ],
      },
    });
    expect(container.querySelector(".resp.reference")).toBeTruthy();
  });

  it("hides the measurement reference when showMeas is off", () => {
    const { container } = render(ResponseCurve, {
      props: {
        filters: [],
        preamp: 0,
        measurement: [
          { freq: 100, spl: 3 },
          { freq: 1000, spl: 0 },
        ],
        showMeas: false,
      },
    });
    expect(container.querySelector(".resp.reference")).toBeNull();
  });

  it("compensating to a target shifts the trace", () => {
    const target = [
      { freq: 20, spl: 6 },
      { freq: 20000, spl: 6 },
    ]; // ~+6 dB across the band
    const dOf = (compensate: boolean) =>
      render(ResponseCurve, { props: { filters: [], preamp: 0, target, compensate } })
        .container.querySelector(".resp.left")!
        .getAttribute("d");

    // Flat response minus a +6 dB target ≠ the uncompensated flat trace.
    expect(dOf(true)).not.toBe(dOf(false));
  });
});
