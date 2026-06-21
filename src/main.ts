import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import { applyAccent, currentAccentId } from "./lib/theme";

// Apply the saved accent before mount so there's no flash of the default blue.
applyAccent(currentAccentId());

// This is a desktop app, so the browser's right-click menu (Reload, Back,
// Inspect, …) is out of place — suppress it everywhere. Controls that use
// right-click themselves (e.g. tone knobs reset to 0) still run their handlers.
window.addEventListener("contextmenu", (e) => e.preventDefault());

const app = mount(App, {
  target: document.getElementById("app")!,
});

export default app;
