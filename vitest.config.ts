import { defineConfig } from "vite-plus";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { svelteTesting } from "@testing-library/svelte/vite";

// Test runner config for `vp test` (separate from vite.config.ts, which is for
// dev/build and lint/format).
// The svelte plugin compiles components; svelteTesting wires up the browser
// resolve conditions and auto-cleanup for @testing-library/svelte.
export default defineConfig({
  plugins: [svelte(), svelteTesting()],
  test: {
    include: ["src/**/*.test.ts"],
    // Pure-logic tests run in Node (fast). Component/DOM tests opt into a DOM
    // per-file with a `// @vitest-environment happy-dom` docblock.
    environment: "node",
  },
});
