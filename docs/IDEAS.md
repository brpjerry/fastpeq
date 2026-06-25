# fastpeq — feature & usability ideas

A backlog of improvements worth considering, grouped by area and roughly
prioritized. Nothing here is committed; it's a menu to pull from for the next
few releases. Effort is a rough guess: **S** (hours), **M** (a day or two),
**L** (multi-day).

Guiding principles, to keep the bar high:

- Stay a **fast, focused preset manager** — don't grow into a DAW plugin host.
- Keep the **Rust core pure** (parse/serialize/store); features that touch the
  EQ model belong there with round-trip tests.
- **Out of scope, on purpose:** auto-generating EQ to match a target (AutoEQ-style
  inversion). fastpeq is a manual/visual editor; the new target overlay + Match
  control support hand-tuning, not automation.

---

## High value, low risk (do first)

- ✅ **Duplicate / "Save as" a preset** — done via **Save current** (the create
  panel's "Save current" button writes the live config as a new preset).
- ✅ **Undo / redo in the editor** — done. Bounded history of bands + preamp +
  balance, coalesced per gesture, with toolbar buttons and `Ctrl+Z`/`Ctrl+Y`.
- ✅ **Conflict rename / duplicate-name guard** — done. Create/rename now collide
  **case-insensitively** (Windows filesystem) with a clear error, while a
  case-only rename of the same preset is still allowed (core `rename` fix).
- ✅ **A/B compare** — done. A **Compare** toggle (enabled when there are unsaved
  changes, `Ctrl+\``) flips the live output to the saved version with the editing
  controls locked, a "● saved" badge, and a faded ghost of the saved curve on the
  graph. Design notes kept below.

## Preset workflow

- **Per-preset global hotkeys** *(M)* — already on the roadmap. Bind a hotkey to
  jump straight to a preset (extends the existing `Ctrl+Alt+B` infrastructure).
- **Recently used / favorites** *(S)* — pin favorites to the top and/or show a
  "recent" group; useful once a library grows past a screen.
- **Export / import a single preset** *(S)* — write a preset (plus its category)
  to a shareable `.txt`/JSON and read it back, for sharing tunings between
  machines. Pairs with the per-preset view-state question below.
- **Sort options for the list** *(S)* — by name, by category, by recently used
  (currently fixed order + search/type filter).
- **Bulk category assignment** *(S)* — multi-select presets to tag at once.

## Curve editor

- **Keyboard nudging of a focused band** *(S–M)* — arrow keys to move freq/gain,
  `[`/`]` for Q, on the selected handle or row. Complements the new Tab order.
- **Solo / mute a band** *(S)* — temporarily hear one band (or mute it) without
  deleting it, to judge its contribution.
- **Copy a band / copy all bands between presets** *(M)* — clipboard for bands;
  handy when reusing a correction across headphones.
- **Plain dB readout at the cursor** *(S)* — alongside the FR-to-target gap, show
  the absolute response/level under the crosshair (not just the gap).
- **Link L/R editing** *(M)* — edit "both" while a measurement or correction is
  per-channel; a link toggle to mirror edits across channels.
- **Export the graph as PNG** *(S)* — save the response curve image to share a
  tuning visually.

## Measurements & targets

- **Built-in target library** *(S–M)* — ship a few common targets (e.g. Harman
  over-ear/in-ear, flat-tilt) so users don't have to source CSVs to start. Just
  seed `targets` with bundled curves.
- **Measurement smoothing** *(M)* — 1/3- or 1/6-octave smoothing on import/display
  so noisy REW exports read cleanly under the trace.
- **Separate L/R measurements** *(M)* — import a left and right measurement and
  show both, for asymmetric headphones/rooms. Bigger model change to
  `presetView`.
- **Target offset: live drag** *(S)* — let the user drag the target line
  vertically to set the offset, in addition to the numeric field just added.

## System integration

- **Autostart with Windows** *(S)* — on the roadmap; start minimized to tray.
- **Update checker** *(S)* — check the GitHub Releases API and surface "a new
  version is available" with a link (no silent auto-update).
- **Per-output-device profiles** *(L)* — APO config is per capture/render device;
  a way to manage and switch presets per device. Significant core + UI work.
- **Config backup management** *(S)* — a small UI to view/restore the first-run
  `config.txt` backup the app already makes.

## Polish & onboarding

- **First-run walkthrough** *(S–M)* — a short guide when APO is detected for the
  first time (what a preset is, how switching works, where files live).
- **Keyboard-shortcut help overlay** *(S)* — a `?` cheat-sheet of shortcuts.
- **Light theme** *(M)* — accent theming exists; a full light mode would help in
  bright environments.
- **Empty/zero states** *(S)* — friendlier copy when there are no presets, no APO,
  or no targets yet.

---

## A/B compare — detailed proposal

**Goal:** while tuning, instantly hear "is my edit actually better?" by flipping
the live output between the working edit and a reference, without losing the
edit.

### What A and B are

- **A = the working edit** (the current in-editor state, possibly unsaved).
- **B = the last-saved version** of the same preset (the reference).

This covers the dominant need — judging unsaved edits — and reuses state the
editor already has. A later extension can let B be a *pinned other preset* for
cross-headphone comparison (see end).

### Where the control goes

A single **Compare** toggle in the editor header actions, immediately left of
**Save** (so the cluster reads `↶ ↷ | Compare | Save`):

```
Beyerdynamic DT990 Pro 250        ● live   ↶  ↷   [ A ⇄ B ]   [ Save ]
```

- **Enabled only when `dirty`** — with no unsaved changes, A and B are identical,
  so there's nothing to compare (button greyed with a "no unsaved changes to
  compare" tooltip).
- The button shows which side is live: **`A ⇄ B`** with the active side
  highlighted (A = accent while editing, B = a distinct "reference" colour while
  comparing). A small **`B · saved`** badge replaces the `● live` indicator while
  on B, so it's obvious you're hearing the reference.
- **Keyboard:** `Tab`-free shortcut — hold **`\``** (backtick) or press
  **`Ctrl+\``** to toggle; guarded so it never fires while a text field is
  focused (same guard as the new undo shortcuts).

### Behaviour

- **Toggle model** (recommended over press-and-hold for discoverability): click
  (or shortcut) flips A→B→A. While on **B**, fastpeq `applyLive`s the saved
  config; flipping back `applyLive`s the working config.
- **Editing is locked on B** — band rows, sliders and the preamp go read-only
  (dimmed) while comparing, with a hint "Comparing with the saved version —
  switch back to A to edit." This prevents edits against the wrong baseline.
- **Graph:** draw the *inactive* side as a faded dashed reference (reuse the
  existing measurement/target reference styling) so the difference is visible as
  well as audible.
- **Exiting:** leaving compare, saving, or switching presets always restores **A**
  and unlocks editing. `Esc` exits compare.

### Implementation sketch

- The editor already builds the working config with `buildConfig()`. Capture the
  **saved** config once at load and after each save: `savedConfig =
  $state.snapshot` of the loaded `Config` (or re-`buildConfig()` right after a
  successful `save()`).
- Add `comparing = $state(false)`. An `$effect`/handler applies the right side:
  `api.applyLive(comparing ? savedConfig : buildConfig())`. The existing throttle
  path is reused; comparing just swaps the source.
- Gate the live-apply `schedule()` while `comparing` so in-flight edits don't
  fight the reference (belt-and-braces with the read-only lock).
- Pass a `comparing`/`reference` prop to `CurveEditor`/`ResponseCurve` to render
  the faded other-side trace.
- Restore on `onDestroy`, preset change, and save.

**Effort:** ~**M**. Most of the plumbing (live apply, reference traces, the header
action slot, the keyboard-guard pattern) already exists; the new parts are the
`comparing` state machine, the read-only lock, and the saved-config cache.

### Later: cross-preset B

Add "Pin as compare (B)" to a preset's right-click menu. When a B preset is
pinned, Compare flips between the active preset and B by `applyLive`-ing each
(no editor lock needed since neither is "the edit"). This is purely additive on
top of the toggle above.

---

## Notes

- Several **frontend-only** items (undo, A/B, sort, favorites, graph PNG) are low
  risk and don't touch the Rust core or the APO config format.
- The bigger ones (**per-device profiles**, **separate L/R measurements**) need
  core/model changes and should get their own design pass first.
- Revisit whether per-preset **view state** (targets/measurements, now in
  `localStorage`) should move to backend sidecars if **export/import** or
  cross-machine portability becomes a goal — see `docs/REVIEW-2.md` (P1/P7).
