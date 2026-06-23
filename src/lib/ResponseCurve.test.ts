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
});
