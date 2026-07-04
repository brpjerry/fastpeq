// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, fireEvent, cleanup, waitFor } from "@testing-library/svelte";
import HardwarePanel from "./HardwarePanel.svelte";
import * as api from "./api";

vi.mock("./api", () => ({
  listHardwareDevices: vi.fn(),
  hardwareStatus: vi.fn(),
  refreshHardware: vi.fn(),
  setOffloadMode: vi.fn(() => Promise.resolve()),
}));

const dha15: api.HardwareDevice = {
  id: "hid-path-1",
  name: "Moondrop DHA15",
  manufacturer: "SPACETOUCH",
  model: "DHA15",
  max_filters: 8,
  user_pregain: false,
};

function status(over: Partial<api.HardwareStatus> = {}): api.HardwareStatus {
  return {
    enabled: false,
    active: false,
    device: null,
    version: null,
    error: null,
    max_filters: null,
    mode: "apo-only",
    ...over,
  };
}

beforeEach(() => {
  vi.mocked(api.listHardwareDevices).mockResolvedValue([dha15]);
  vi.mocked(api.hardwareStatus).mockResolvedValue(status());
  vi.mocked(api.refreshHardware).mockResolvedValue(status());
  vi.mocked(api.setOffloadMode).mockResolvedValue();
});
afterEach(cleanup);

const segLabels = (root: ParentNode) =>
  [...root.querySelectorAll(".seg-btn")].map((b) => b.textContent?.trim());

describe("HardwarePanel", () => {
  it("shows the five routing buttons and no per-device switch", async () => {
    const { container, getByText } = render(HardwarePanel, { props: {} });
    await waitFor(() => getByText("Moondrop DHA15"));
    expect(segLabels(container)).toEqual([
      "APO Only",
      "First 8",
      "Biggest effect",
      "Min. APO Preamp",
      "Hardware Only",
    ]);
    expect(container.querySelector(".switch")).toBeNull(); // no toggle anymore
  });

  it("defaults to APO Only (offload off) and says so", async () => {
    const { container, getByText } = render(HardwarePanel, { props: {} });
    await waitFor(() => getByText(/every band goes to Equalizer APO/i));
    const apo = [...container.querySelectorAll(".seg-btn")].find(
      (b) => b.textContent?.trim() === "APO Only",
    );
    expect(apo?.classList.contains("sel")).toBe(true);
  });

  it("changes the mode and notifies the parent", async () => {
    const onChanged = vi.fn();
    const { getByText } = render(HardwarePanel, { props: { onChanged } });
    await waitFor(() => getByText("Hardware Only"));
    await fireEvent.click(getByText("Hardware Only"));
    expect(api.setOffloadMode).toHaveBeenCalledWith("hardware-only");
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("disables the split modes without Equalizer APO; Hardware Only stays available", async () => {
    const { container, getByText } = render(HardwarePanel, { props: { apoInstalled: false } });
    await waitFor(() => getByText("Moondrop DHA15"));

    const btn = (label: string) =>
      [...container.querySelectorAll<HTMLButtonElement>(".seg-btn")].find(
        (b) => b.textContent?.trim() === label,
      )!;
    // The split modes run their software half in APO — dead without an install.
    expect(btn("First 8").disabled).toBe(true);
    expect(btn("Biggest effect").disabled).toBe(true);
    expect(btn("Min. APO Preamp").disabled).toBe(true);
    // Off and all-on-device remain the two honest choices.
    expect(btn("APO Only").disabled).toBe(false);
    expect(btn("Hardware Only").disabled).toBe(false);
    getByText(/only Hardware Only routes the full EQ/);
  });

  it("explains when offload is on but the active output isn't supported", async () => {
    vi.mocked(api.refreshHardware).mockResolvedValue(
      status({ enabled: true, active: false, mode: "first-x" }),
    );
    const { getByText } = render(HardwarePanel, { props: {} });
    await waitFor(() => getByText(/isn't a supported device/));
  });

  it("marks the active device when offload is engaged", async () => {
    vi.mocked(api.refreshHardware).mockResolvedValue(
      status({
        enabled: true,
        active: true,
        device: dha15,
        version: "V0.1",
        max_filters: 8,
        mode: "first-x",
      }),
    );
    const { container, getByText } = render(HardwarePanel, { props: {} });
    await waitFor(() => getByText(/Offloading to/));
    expect(getByText(/firmware V0\.1/)).toBeTruthy();
    expect(container.querySelector(".hw-list li.target")).toBeTruthy();
    expect(container.querySelector(".hw-chip")?.textContent).toBe("ON");
  });
});
