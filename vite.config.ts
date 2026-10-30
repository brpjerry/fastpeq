import { defineConfig, lazyPlugins } from "vite-plus";
import { svelte } from "@sveltejs/vite-plugin-svelte";

const host = process.env.TAURI_DEV_HOST;

// Tauri-aware Vite+ config: fixed dev port, no clearing of the Tauri logs. Also
// holds the Oxfmt/Oxlint settings used by `vp fmt`, `vp lint` and `vp check`.
export default defineConfig({
  fmt: {
    // Oxfmt defaults for TS/JS/CSS/JSON/YAML. Markdown (hand-wrapped docs) and
    // TOML (Cargo manifests, Rust's side) are left as written. `.svelte` files
    // aren't formatted either: Oxfmt only does so with `svelte: true`.
    ignorePatterns: ["**/*.md", "**/*.toml"],
  },
  lint: {
    jsPlugins: [{ name: "vite-plus", specifier: "vite-plus/oxlint-plugin" }],
    rules: { "vite-plus/prefer-vite-plus-imports": "error" },
    options: { typeAware: true, typeCheck: true },
  },
  plugins: lazyPlugins(() => [svelte()]),
  clearScreen: false,
  // Two entry points: the main app and the small always-on-top OSD overlay window.
  build: {
    rollupOptions: {
      input: { main: "index.html", osd: "osd.html" },
    },
  },
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
