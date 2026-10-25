// @vitest-environment happy-dom
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, fireEvent, cleanup } from "@testing-library/svelte";
import BandRow from "./BandRow.svelte";
import type { Channel, FilterKind } from "./types";

afterEach(cleanup);

type Band = {
  id: number;
  enabled: boolean;
  kind: FilterKind;
  freq: number;
  gain: number;
  q: number | null;
  slope: number | null;
  channel: Channel;
};

const band = (over: Partial<Band> = {}): Band => ({
  id: 1,
  enabled: true,
  kind: "Peak",
  freq: 1000,
  gain: 0,
  q: 1,
  slope: null,
  channel: { kind: "both" },
  ...over,
});

const cbs = () => ({
  onChange: vi.fn(),
  onChangeKind: vi.fn(),
  onRemove: vi.fn(),
  onHover: vi.fn(),
});

describe("BandRow", () => {
  it("shows freq, gain and Q fields for a peaking band", () => {
    const { container } = render(BandRow, { props: { band: band(), hovered: false, ...cbs() } });
    expect(container.querySelector(".field.freq")).not.toBeNull();
    expect(container.querySelector(".field.gain")).not.toBeNull();
    expect(container.querySelector(".field.q")).not.toBeNull();
  });

  it("hides the Q field for a shelf without Q", () => {
    const { container } = render(BandRow, {
      props: { band: band({ kind: "LowShelf", q: null }), hovered: false, ...cbs() },
    });
    expect(container.querySelector(".field.gain")).not.toBeNull();
    expect(container.querySelector(".field.q")).toBeNull();
  });

  it("edits the Q a fixed shelf's line came with (APO's corner-frequency shelf)", async () => {
    const b = band({ kind: "HighShelf", q: 0.7 });
    const c = cbs();
    const { container } = render(BandRow, { props: { band: b, hovered: false, ...c } });
    const input = container.querySelector<HTMLInputElement>(".field.q input")!;
    await fireEvent.input(input, { target: { value: "1.2" } });
    await fireEvent.change(input);
    expect(b.q).toBe(1.2);
    expect(c.onChange).toHaveBeenCalled();
  });

  it("edits a slope shelf's slope in place of its Q", async () => {
    const b = band({ kind: "LowShelfQ", slope: 12 });
    const c = cbs();
    const { container } = render(BandRow, { props: { band: b, hovered: false, ...c } });
    const field = container.querySelector(".field.slope");
    expect(field?.textContent).toContain("dB/oct");
    expect(container.querySelectorAll(".field.q")).toHaveLength(1); // no second Q box
    const input = field!.querySelector<HTMLInputElement>("input")!;
    await fireEvent.input(input, { target: { value: "6" } });
    await fireEvent.change(input);
    expect(b.slope).toBe(6);
    expect(b.q).toBe(1); // untouched underneath
    expect(c.onChange).toHaveBeenCalled();
  });

  it("hides the gain field for a band with no gain (e.g. high-pass)", () => {
    const { container } = render(BandRow, {
      props: { band: band({ kind: "HighPass" }), hovered: false, ...cbs() },
    });
    expect(container.querySelector(".field.gain")).toBeNull();
  });

  it("writes edits back to the band and fires onChange", async () => {
    const b = band();
    const c = cbs();
    const { container } = render(BandRow, { props: { band: b, hovered: false, ...c } });
    const freq = container.querySelector<HTMLInputElement>(".field.freq input")!;
    await fireEvent.input(freq, { target: { value: "2500" } });
    await fireEvent.change(freq);
    expect(b.freq).toBe(2500);
    expect(c.onChange).toHaveBeenCalled();
  });

  it("keeps the gain slider out of the tab order", () => {
    const { container } = render(BandRow, { props: { band: band(), hovered: false, ...cbs() } });
    const slider = container.querySelector<HTMLInputElement>(".field.gain input[type='range']")!;
    expect(slider.getAttribute("tabindex")).toBe("-1");
    // The number boxes stay tabbable so Tab from Hz lands on the gain value.
    const numbers = container.querySelectorAll(".field input[type='number']");
    numbers.forEach((n) => expect(n.getAttribute("tabindex")).toBeNull());
  });

  it("resets gain to 0 on right-click of the slider", async () => {
    const b = band({ gain: 6 });
    const c = cbs();
    const { container } = render(BandRow, { props: { band: b, hovered: false, ...c } });
    const slider = container.querySelector<HTMLInputElement>(".field.gain input[type='range']")!;
    await fireEvent.contextMenu(slider);
    expect(b.gain).toBe(0);
    expect(c.onChange).toHaveBeenCalled();
  });

  it("holds a gain edit to the limit, from the slider and the number box", async () => {
    const b = band();
    const c = cbs();
    const { container } = render(BandRow, {
      props: { band: b, hovered: false, gainLimit: 12, ...c },
    });
    const num = container.querySelector<HTMLInputElement>(".field.gain input[type='number']")!;
    await fireEvent.input(num, { target: { value: "20" } });
    await fireEvent.change(num);
    expect(b.gain).toBe(12);

    const slider = container.querySelector<HTMLInputElement>(".field.gain input[type='range']")!;
    await fireEvent.input(slider, { target: { value: "-20" } });
    expect(b.gain).toBe(-12);
    expect(c.onChange).toHaveBeenCalled();
  });

  it("keeps a preset's over-limit gain until that gain is edited", async () => {
    const b = band({ gain: 18 });
    const c = cbs();
    const { container } = render(BandRow, {
      props: { band: b, hovered: false, gainLimit: 12, ...c },
    });
    // Loading it changes nothing, and the controls stretch to show the real value.
    expect(b.gain).toBe(18);
    expect(container.querySelector(".field.gain")!.classList.contains("over")).toBe(true);
    const slider = container.querySelector<HTMLInputElement>(".field.gain input[type='range']")!;
    expect(slider.max).toBe("18");

    // Editing another field leaves the over-limit gain alone...
    const freq = container.querySelector<HTMLInputElement>(".field.freq input")!;
    await fireEvent.input(freq, { target: { value: "2000" } });
    await fireEvent.change(freq);
    expect(b.gain).toBe(18);

    // ...but touching the gain itself brings it inside the limit.
    const num = container.querySelector<HTMLInputElement>(".field.gain input[type='number']")!;
    await fireEvent.input(num, { target: { value: "17" } });
    await fireEvent.change(num);
    expect(b.gain).toBe(12);
  });

  it("uses a wider limit when settings allow it", async () => {
    const b = band();
    const { container } = render(BandRow, {
      props: { band: b, hovered: false, gainLimit: 24, ...cbs() },
    });
    const num = container.querySelector<HTMLInputElement>(".field.gain input[type='number']")!;
    await fireEvent.input(num, { target: { value: "20" } });
    await fireEvent.change(num);
    expect(b.gain).toBe(20);
  });

  it("calls onRemove when the remove button is clicked", async () => {
    const c = cbs();
    const { container } = render(BandRow, { props: { band: band(), hovered: false, ...c } });
    await fireEvent.click(container.querySelector(".remove")!);
    expect(c.onRemove).toHaveBeenCalled();
  });

  it("reports hover enter/leave for graph-row linking", async () => {
    const c = cbs();
    const { container } = render(BandRow, { props: { band: band(), hovered: false, ...c } });
    const row = container.querySelector(".band")!;
    await fireEvent.mouseEnter(row);
    await fireEvent.mouseLeave(row);
    expect(c.onHover).toHaveBeenNthCalledWith(1, true);
    expect(c.onHover).toHaveBeenNthCalledWith(2, false);
  });

  it("marks the row off and hovered from props", () => {
    const { container } = render(BandRow, {
      props: { band: band({ enabled: false }), hovered: true, ...cbs() },
    });
    const row = container.querySelector(".band")!;
    expect(row.classList.contains("off")).toBe(true);
    expect(row.classList.contains("hover")).toBe(true);
  });

  const statusOf = (root: ParentNode) => root.querySelector<HTMLButtonElement>(".status")!;

  it("labels the status toggle ON/OFF outside a hybrid split", () => {
    const on = render(BandRow, { props: { band: band(), hovered: false, ...cbs() } });
    expect(statusOf(on.container).textContent!.trim()).toBe("ON");

    const off = render(BandRow, {
      props: { band: band({ enabled: false }), hovered: false, ...cbs() },
    });
    expect(statusOf(off.container).textContent!.trim()).toBe("OFF");
  });

  it("labels the status toggle APO/HW per the backend's placement in hybrid mode", () => {
    // Placement arrives via the `offloaded` prop — the row itself must not infer
    // it (selection may be mode-driven or, in the future, user-assigned).
    const apo = render(BandRow, {
      props: { band: band(), hovered: false, hybrid: true, offloaded: false, ...cbs() },
    });
    expect(statusOf(apo.container).textContent!.trim()).toBe("APO");
    expect(statusOf(apo.container).classList.contains("hw")).toBe(false);

    const hw = render(BandRow, {
      props: { band: band(), hovered: false, hybrid: true, offloaded: true, ...cbs() },
    });
    expect(statusOf(hw.container).textContent!.trim()).toBe("HW");
    expect(statusOf(hw.container).classList.contains("hw")).toBe(true);
  });

  it("shows OFF over APO/HW for a disabled band even in hybrid mode", () => {
    const { container } = render(BandRow, {
      props: { band: band({ enabled: false }), hovered: false, hybrid: true, offloaded: true, ...cbs() },
    });
    expect(statusOf(container).textContent!.trim()).toBe("OFF");
  });

  it("toggles enabled from the status button and fires onChange", async () => {
    const b = band();
    const c = cbs();
    const { container } = render(BandRow, { props: { band: b, hovered: false, ...c } });
    await fireEvent.click(statusOf(container));
    expect(b.enabled).toBe(false);
    expect(c.onChange).toHaveBeenCalled();
  });

  it("marks a Hardware Only muted band as silent (still ON, hollow)", () => {
    const { container } = render(BandRow, {
      props: { band: band(), hovered: false, muted: true, ...cbs() },
    });
    expect(statusOf(container).textContent!.trim()).toBe("ON");
    expect(statusOf(container).classList.contains("silent")).toBe(true);
    expect(container.querySelector(".band")!.classList.contains("muted")).toBe(true);
  });
});
