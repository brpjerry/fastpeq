# fastpeq — code review #2 (post curve-editor suite)

A second pass over the codebase after the curve-editor/targets/compensation work
landed (PR #11). The first review (`REVIEW.md`) is fully done; this is a fresh
list of fixes, cleanup, and best-practice items, prioritized. Nothing here is
executed yet — same flow as last time: review, then work through it.

Health is good overall: no `TODO`/`FIXME`, no stray `console.log`, no `any` /
`@ts-ignore`, svelte-check + clippy clean, 90 frontend + 29 Rust + 5 E2E tests
green. The items below are improvements, not breakage — except **P1**, which is a
real data-loss bug.

**Status (worked through):** ✅ P1, P2, P3, P4, P5, P6, P7, P8 done · ⬜ P9
deferred ("only if it bites"). P5 extracted both `GraphTools.svelte` and
`BandRow.svelte` from `Editor.svelte`. Frontend tests grew 90 → 110.

---

## P1 — Per-preset view state is lost on rename and orphaned on delete *(bug)*

The new per-preset settings — selected target, compensate, the two reference
toggles, and the imported measurement — live in `localStorage` keyed by preset
**name** (`presetView.svelte.ts`), separate from the backend preset files.

- **Rename** (`App.svelte` → `api.renamePreset`) renames the `.txt` and migrates
  the *categories* sidecar in the core, but nothing migrates the `presetView`
  entry. After a rename the preset reverts to defaults (Flat target, no
  measurement) and the old entry is orphaned.
- **Delete** leaves a dangling `presetView` (and measurement) entry forever.

**Action:** add `renamePresetView(from, to)` / `clearPresetView(name)` to
`presetView.svelte.ts` and call them from the App's rename/delete handlers. (The
backend already does the equivalent for categories — mirror that.) Decide the
same for `targets` is N/A since targets are global, not per-preset.

---

## P2 — Graph colors are hardcoded and duplicated across both graphs

`CurveEditor.svelte` and `ResponseCurve.svelte` repeat the same literal hex
colors: `#181b21` (plot bg), `#e0a458` (right channel), `#2a2f38`/`#3a4150`
(grid/axis), `#11141a` (label outline), and `#6fcf97` (target) — the target
color appears **twice in CurveEditor alone** (`.resp.target` stroke and
`.delta-lbl` fill) and must be kept in sync by hand.

**Action:** lift these into CSS variables on `:root` in `app.css`
(`--graph-bg`, `--chan-right`, `--graph-grid`, `--graph-axis`, `--label-outline`,
`--target`) and reference them in both components. Keeps the two graphs
consistent and makes the accent/target themeable.

---

## P3 — Switch styling is duplicated (App vs the new Switch component)

`Switch.svelte` (used in the curve editor) reimplements the same track/thumb
toggle CSS that already lives inline in `App.svelte` for the Tone panel and the
Settings switches (`.switch`/`.track`/`.thumb`, ~50 lines each).

**Action:** migrate the App tone/settings toggles to `<Switch>` and delete the
duplicated CSS — one source of truth for the toggle look.

---

## P4 — `App.svelte` is 1598 lines; split it up

It now holds the header, the entire Settings page, the preset list + category
pickers + filter dropdown, the Tone panel, and every handler. It's the hardest
file to navigate.

**Action (incremental):** extract `Settings.svelte` (the whole settings panel —
accent, band count, filter set, curve editor, targets, categories, tone-gen,
import, storage, APO status) first; it's self-contained and ~400 lines. Then
consider `PresetList.svelte` (list + category cycle/menu + device filter).

---

## P5 — `Editor.svelte` is 1068 lines; extract the obvious blocks

The band row, the balance popover, and the graph-tools (target dropdown +
compensate/measurement switches + import) are large self-contained snippets.

**Action:** pull the band row into `BandRow.svelte` and the graph-tools into
`GraphTools.svelte`. Reduces the file and makes the controls testable in
isolation.

---

## P6 — Curve-editor cursor/gap/compensation math isn't unit-tested

The FR-to-target gap, the displayed target line value, and the compensation
offset are computed inline in `CurveEditor.svelte`, which can't render in
happy-dom (it's layout-gated), so none of that logic is covered.

**Action:** move the pure math (gap = |FR − target| at a frequency, the
`targetLine`/compensate transforms) into `graph.ts` (or a new `curve.ts`) as
plain functions and unit-test them, the way `eq.ts`/`measurement.ts` are. The
component then just calls them.

---

## P7 — Measurements stored as full point arrays in one localStorage blob

`presetView` serializes every preset's measurement (potentially thousands of
`{freq, spl}` points from a REW export) into a single JSON blob, rewritten on
every change. A few large measurements can approach the ~5 MB origin quota and
make each write heavier.

**Action:** downsample measurements on import to ~256 log-spaced points before
storing (the graphs sample onto the plot grid anyway, so visual fidelity is
unaffected), and/or store each preset's measurement under its own key.

---

## P8 — `read_text_file` reads any path the frontend asks for

`commands.rs::read_text_file` does `fs::read_to_string(path)` for whatever path
it's handed. In practice the path always comes from a dialog the user just
confirmed, so the risk is low, but it's an unrestricted file read in the IPC
surface.

**Action:** rely on the Tauri `fs`/dialog scope, or at least verify the path is a
regular file before reading. Low priority, defense-in-depth.

---

## P9 — Filter shapes recompute per band on every render

With "filter shapes" on, `shapePath(band)` runs `responseCurve` over ~plotW
points for **each** band on every reactive update (incl. during a drag). Fine for
typical presets; could get heavy at 20+ bands on a large graph.

**Action (only if it bites):** memoize per-band shapes keyed by
`(kind, freq, gain, q, preamp, plotW)`, or coarsen the shape sampling vs the main
trace.

---

## Notes / non-issues

- **localStorage for view state is a deliberate trade-off** (frontend-only, no
  Rust/IPC), but it's the root of P1 and P7. If portability of measurements/
  targets across machines ever matters, revisit backend sidecars — otherwise the
  P1/P7 fixes are enough.
- The `compensate` effective-vs-raw value split in `Editor.svelte` is subtle but
  correct and commented; leave as is.
- Test coverage and CI gating are in good shape; P6 is the main gap.
