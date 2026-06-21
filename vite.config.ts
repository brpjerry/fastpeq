import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

const host = process.env.TAURI_DEV_HOST;

// Tauri-aware Vite config: fixed dev port, no clearing of the Tauri logs.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    // Don't watch Rust build output. In this workspace layout `target/` lives at
    // the repo root (not under src-tauri/), and watching the in-flight
    // `fastpeq_lib.dll` triggers EBUSY and crashes the dev server.
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
});
