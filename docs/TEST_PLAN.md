# fastpeq — test plan (post-1.0)

Plan for adding UI and end-to-end coverage **after the first release**. The
"lean combo": one runner (vitest) for unit **and** component/UI tests, plus a
small WebDriver suite for a handful of true end-to-end smokes.

> All of this is **dev/CI tooling** — none of it ships in the app bundle (the
> Tauri build only embeds `dist/`, from the two runtime deps). "More deps" here
> means install time + CI, never binary size or shipped attack surface.

## Current coverage (already in place)

- **Rust** — 29 tests across parse/serialize round-trip, `PresetStore`, the
  `Manager` flow (apply/bypass/restore, capture), PEACE import, name safety,
  and APO env. (`cargo test`)
- **Frontend pure logic** — 28 vitest tests for `eq.ts` (biquad magnitude,
  `balanceTrim`, `peakGainDb`, `toneFilters`), `measurement.ts`, `graph.ts`,
  `starter.ts`. (`npm test`)

**The gap this plan fills:** component/UI *interaction* behavior, and *true
integration* (real Svelte UI ↔ real Rust backend ↔ real `config.txt`). These are
exactly the two layers that were painful to verify by hand and that hid the two
bugs we found late (the dropdown render bug and the bypass-restore regression).

---

## Layer 1 — Component & UI tests (vitest + @testing-library/svelte) ✅ DONE

Render Svelte components in a headless DOM and assert behavior, with the backend
mocked. Reuses the existing vitest runner, so the marginal footprint is small.

> **Status:** implemented. 37 component tests (`dismiss` 9, `TypeSelect` 4,
> `Knob` 5, `Editor` 10, `App` 9) on top of the 28 pure-logic tests — **65
> frontend tests** total, all green and folded into `npm test` / CI. Backend
> mocked at `src/lib/api.ts`; `@tauri-apps/api/event` and `plugin-dialog` mocked
> per file. The Web Audio stub turned out unnecessary — the tone generator only
> constructs `AudioContext` lazily on play, so the expanded view renders headless.

### Dependencies to add (dev)
- `@testing-library/svelte` (v5+, Svelte 5 compatible)
- `happy-dom` (DOM environment; lighter than jsdom)
- `@testing-library/jest-dom` (matchers like `toBeInTheDocument`)
- `@testing-library/user-event` (realistic pointer/keyboard; `fireEvent` may
  suffice for simple cases)

### Config
- Keep pure-logic tests in the fast `node` environment; opt component test files
  into the DOM with a per-file docblock: `// @vitest-environment happy-dom`.
- Add a small `vitest.config.ts` (or a `test` block) with `setupFiles` importing
  `@testing-library/jest-dom/vitest`. The svelte plugin already in
  `vite.config.ts` compiles `.svelte` for vitest.

### Mocking the backend
All IPC funnels through `src/lib/api.ts` — mock that one module, not scattered
`invoke` calls. Provide a tiny in-memory fake (preset list, categories, tone,
active preset, status) with a builder so each test sets up its own scenario.
- `localStorage`-backed prefs/theme/starter work as-is (happy-dom has
  localStorage).
- Stub `AudioContext` for `ToneGenerator` (happy-dom has no Web Audio).

### What to test (priority order — risk- and regression-driven)
1. ✅ **Popovers / dropdowns + the `dismissable` action** *(highest value — this
   is where the dropdown bug lived)*: `dismissable` (9 cases incl. outside
   `pointerdown`, `Escape`, scroll, resize, the `ignore` trigger), `TypeSelect`
   (open/select/no-op/dismiss), the device-type filter and category right-click
   menu in `App.svelte`, and the balance popover in `Editor.svelte`.
2. ✅ **Preset list filtering**: search query + device-type filter; the filter
   only lists categories that are actually used.
3. ✅ **Category assignment**: left-click cycle (calls `setCategory`), right-click
   picker assigns a chosen category. *(Optimistic revert-on-failure left to the
   Rust/manual side — the happy path is covered.)*
4. ✅ **Editor band ops**: add/remove band, a gain edit propagates to the live
   config via `applyLive`. *(Per-field clamping and the L/R/Both toggle: partial
   — could extend.)*
5. ✅ **Clipping indicator**: shown when the summed boost tops 0 dB, hidden when
   the preamp keeps it under.
6. ✅ **Bypass indicator**: live vs bypassed track the prop.
7. ✅ **Knob**: arrow keys, scroll, right-click reset to 0, clamp, value readout.
8. ✅ **Settings**: accent swatch applies the `--accent` CSS var; filter-set
   toggle flips `getFilterSet()`; the specialty switch gates the group.
9. ✅ **Measurement import**: with `open`/`readTextFile` mocked, importing a REW
   file in the expanded view surfaces the measurement name + clear control.

### Known limitation
happy-dom has no real layout — `getBoundingClientRect` returns zeros — so menu
*positioning* and curve-editor *drag geometry* aren't meaningfully testable here.
Test the **logic and DOM** (does it open, does selecting work, does dismiss
fire), and leave pixel/positioning to Layer 2 or manual checks.

---

## Layer 2 — End-to-end smokes (WebdriverIO + tauri-driver)

Drive the **real built app** over WebDriver — real UI, real Rust backend, real
`config.txt` writes. Heavier and flakier, so keep it to a handful of critical
paths, not broad coverage.

### Tooling to add
- `cargo install tauri-driver` (a dev/CI binary, not a `Cargo.toml` dep).
- `msedgedriver` matched to the installed WebView2 Runtime version
  (per Tauri's WebDriver docs).
- npm (dev): `@wdio/cli`, `@wdio/local-runner`, `@wdio/mocha-framework`,
  `@wdio/spec-reporter`, plus a `wdio.conf.ts` that boots `tauri-driver`.
- A debug app build for WDIO to launch (`target/debug/fastpeq.exe`).

### Determinism prerequisite (small core change)
The app detects APO from the registry and writes the real `config.txt`. For
hermetic E2E, add a test override (e.g. a `FASTPEQ_TEST_DATA_DIR` /
`FASTPEQ_TEST_CONFIG` env var) so a run points the preset store **and** the APO
config file at temp dirs, seeded with a couple of preset files. Without this,
E2E only works on a machine with APO installed and mutates the real config.

### The smokes (~4–6)
1. **Launch** → presets render (seeded list).
2. **Apply** → click a preset → it highlights active **and** temp `config.txt`
   contains its filters.
3. **Bypass round-trip** *(the headline — guards the regression we just fixed)*:
   edit a band (don't save) → Bypass (indicator "bypassed", config has preamp,
   no filters) → Bypass again → the exact edit is restored.
4. **Save** → edit a band's gain → Save → the preset `.txt` reflects it.
5. **Create** → new preset from a band count → file created, appears in list.
6. **Device-type filter** → dropdown shows icons, selecting filters the list
   (the dropdown bug, at real-render fidelity).

Tray and global-hotkey paths are OS-level and awkward via WebDriver — leave them
to manual smoke tests.

---

## CI integration
- **Layer 1** folds into the existing `npm test` step in `.github/workflows/ci.yml`
  (vitest runs both the node and happy-dom files) — no new job.
- **Layer 2** becomes its own job (slower, possibly flaky): Windows runner,
  steps to install `msedgedriver` + `cargo install tauri-driver` + build the app
  + run WDIO. Start it **non-blocking** (or nightly / manual dispatch) until it's
  proven stable.

---

## Suggested phasing
1. ✅ **Phase 1** — Layer 1 popover/dropdown + `dismissable` tests (highest-risk
   surface).
2. ✅ **Phase 2** — the rest of Layer 1 (editor, settings, filtering, knob).
3. ⬜ **Phase 3** — the `FASTPEQ_TEST_DATA_DIR` hook, then the Layer 2 smokes
   (bypass round-trip first).

## Dependency footprint summary
- Layer 1: ~4 small npm dev-deps, no binaries, reuses vitest.
- Layer 2: the `@wdio/*` npm stack + `tauri-driver` (cargo) + `msedgedriver`
  (external binary). Most of the moving parts; version of msedgedriver must
  track WebView2.
- Shipped app: **unchanged** by any of it.
