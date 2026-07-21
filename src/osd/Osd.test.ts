// @vitest-environment happy-dom
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@testing-library/svelte";

// Capture the OSD event listener and stub the native presentation IPC call.
const { ev, invoke, win } = vi.hoisted(() => ({
  ev: { cb: null as null | ((e: { payload: unknown }) => void) },
  invoke: vi.fn(() => Promise.resolve()),
  win: {
    show: vi.fn(() => Promise.resolve()),
    hide: vi.fn(() => Promise.resolve()),
    setAlwaysOnTop: vi.fn(() => Promise.resolve()),
  },
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((_event: string, cb: (e: { payload: unknown }) => void) => {
    ev.cb = cb;
    return Promise.resolve(() => {});
  }),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => win,
}));

import Osd from "./Osd.svelte";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  ev.cb = null;
});

describe("Osd overlay", () => {
  it("renders a payload's title, detail and bar, and marks itself shown", async () => {
    const { container } = render(Osd);
    await waitFor(() => expect(ev.cb).toBeTruthy());

    ev.cb!({ payload: { title: "Bass", detail: "+3.0 dB", bar: { value: 3, min: -12, max: 12 } } });

    await waitFor(() => expect(container.querySelector(".title")?.textContent).toBe("Bass"));
    expect(container.querySelector(".detail")?.textContent).toBe("+3.0 dB");
    expect(container.querySelector(".bar")).not.toBeNull();
    expect(container.querySelector(".osd")!.classList.contains("shown")).toBe(true);
  });

  it("coalesces rapid events into a single updating card", async () => {
    const { container } = render(Osd);
    await waitFor(() => expect(ev.cb).toBeTruthy());

    ev.cb!({ payload: { title: "Bass", detail: "+0.5 dB", bar: { value: 0.5, min: -12, max: 12 } } });
    ev.cb!({ payload: { title: "Bass", detail: "+1.0 dB", bar: { value: 1, min: -12, max: 12 } } });

    await waitFor(() => expect(container.querySelector(".detail")?.textContent).toBe("+1.0 dB"));
    expect(container.querySelectorAll(".osd").length).toBe(1);
  });

  it("delegates show, desktop placement, and raise to one native command", async () => {
    render(Osd);
    await waitFor(() => expect(ev.cb).toBeTruthy());

    ev.cb!({ payload: { title: "Bypass", detail: "EQ on" } });

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("osd_present"));
    expect(win.show).not.toHaveBeenCalled();
    expect(win.setAlwaysOnTop).not.toHaveBeenCalled();
  });
});
