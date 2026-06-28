import { mount } from "svelte";
import "./osd.css";
import Osd from "./Osd.svelte";
import { applyAccent, currentAccentId } from "../lib/theme";
import { getCurrentWindow, currentMonitor, PhysicalPosition } from "@tauri-apps/api/window";

// Match the app's accent (shared via localStorage, same origin as the main window).
applyAccent(currentAccentId());

const win = getCurrentWindow();
// Click-through: the overlay must never intercept the pointer.
win.setIgnoreCursorEvents(true).catch(() => {});

// Park the OSD bottom-center of the primary monitor (everything in physical px).
async function position() {
  try {
    const mon = await currentMonitor();
    if (!mon) return;
    const size = await win.outerSize();
    const x = mon.position.x + Math.round((mon.size.width - size.width) / 2);
    const y = mon.position.y + Math.round(mon.size.height * 0.82);
    await win.setPosition(new PhysicalPosition(x, y));
  } catch {
    /* best effort — fall back to the configured position */
  }
}
position();

const app = mount(Osd, { target: document.getElementById("osd")! });

export default app;
