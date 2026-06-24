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

- **Duplicate / "Save as" a preset** *(S)* — clone the current preset under a new
  name. Today the only way to fork a tuning is to hand-copy it. Pure store op.
- **Undo / redo in the editor** *(M)* — edits apply live with no take-back; a
  bounded undo stack (band add/remove/edit, preamp, balance) would make
  experimentation safe. Frontend-only, keyed off the existing `schedule` points.
- **A/B compare** *(S–M)* — a momentary "compare" button (or hotkey) that flips
  between the working tuning and the last-saved version (or a pinned B preset),
  so you can hear the difference instantly. Leans on the instant-switch design.
- **Reveal-on-conflict rename / duplicate-name guard** *(S)* — surface a clear
  error when a rename/duplicate would collide, instead of failing silently.

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

## Notes

- Several **frontend-only** items (undo, A/B, sort, favorites, graph PNG) are low
  risk and don't touch the Rust core or the APO config format.
- The bigger ones (**per-device profiles**, **separate L/R measurements**) need
  core/model changes and should get their own design pass first.
- Revisit whether per-preset **view state** (targets/measurements, now in
  `localStorage`) should move to backend sidecars if **export/import** or
  cross-machine portability becomes a goal — see `docs/REVIEW-2.md` (P1/P7).
