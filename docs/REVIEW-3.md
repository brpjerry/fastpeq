# fastpeq — code review #3 (post hardware-offload)

Third pass over the codebase after the hardware EQ offload work landed (PR #27).
Reviews #1 (`REVIEW.md`) and #2 (`REVIEW-2.md`) are fully worked through; this is
a fresh list. Health at review time: clippy clean, svelte-check clean, 176
frontend + all Rust tests green, no TODO/FIXME markers.

**Status:** ✅ All items (1–15) worked through.

---

## P1 — Robustness bugs

- [x] 1. **A failed offload session open gets stuck until something else
  changes.** `sync_offload` (`src-tauri/src/state.rs`) updates the `last_sync`
  cache *before* reconciling, then opens the session with
  `if let Ok(profile) = hardware::profile(&dev.id)` and no else branch. If
  `profile()` fails transiently (device jostled mid-enumeration, HID glitch), no
  session opens but the cache records this `(enabled, output)` pair as
  reconciled — every later `sync_offload` short-circuits and offload silently
  stays off until the output or mode changes. The worker-death path already
  resets `last_sync` in `clear_hardware_session`; the failed-open path must do
  the same.

- [x] 2. **A failed preset load can wipe the live EQ.** `load()` in
  `src/lib/Editor.svelte` runs `api.applyLive(buildConfig(false), …)` from its
  `finally` block whenever Auto Preamp is effective. If `getPreset` threw,
  `bands`/`rawLines` were just reset to empty — so this writes an essentially
  empty config to `config.txt`, silencing whatever was playing. Gate the apply
  on the load having succeeded (`!err`).

- [x] 3. **`set_offload_mode` mutates memory before persisting.**
  (`src-tauri/src/state.rs`) It sets the in-memory mode, then persists; if
  `save_settings` fails it returns an error but the in-memory mode stays
  changed, so the UI shows a mode that won't survive a restart. Persist first —
  `set_presets_dir` already does it in that order.

## P2 — Performance

- [x] 4. **Biquad coefficients are recomputed per frequency point — on both
  sides of the IPC boundary.** The one real optimization opportunity, on hot
  paths:
  - `crates/fastpeq-core/src/offload.rs`: `magnitude_db` called
    `biquad_coeffs` (cos/sin/powf/sqrt) for every one of the 240 probe
    frequencies. `select_min_preamp` called `peak_gain_db` inside a greedy loop
    over trials — roughly `max × n² × 240` magnitude evaluations per split,
    each with fresh transcendentals, on every throttled live-drag write in
    Minimize-preamp mode (and again via `selected_filter_positions` from the
    editor's debounced per-band indicator query). `band_area` was also
    recomputed inside sort comparators. **Fix:** split coefficient computation
    from evaluation (`coeffs_magnitude_db`), precompute one magnitude-response
    vector per band (`band_response`), make the greedy loop pure vector sums,
    and rank by precomputed areas. Measured (release, 20-band preset):
    `split(MinimizePreamp)` 22.3 ms → 0.45 ms/call (~50×),
    `split(LargestChange)` 2.2 ms → 0.25 ms/call (~9×).
  - `src/lib/eq.ts`: `magnitudeDb` rebuilt coefficients per `(filter, freq)`
    pair; `CurveEditor` samples up to ~1600 points per trace per drag frame,
    plus `clipPeak`/`computeAutoPreamp` each run `peakGainDb` (two full
    `responseCurve` passes) per frame. **Fix:** compute coefficients once per
    filter (`biquadCoeffs`), and the per-frequency trig once per point shared
    across all filters.

- [x] 5. **`reload()` serializes eight IPC round-trips** (`src/App.svelte`):
  `apoStatus`, `presetsDir`, `hardwareStatus`, then `listPresets`,
  `presetCategories`, `activePreset`, `bypassed`, `getTone` one at a time — and
  it runs on every window focus. Most are independent; `Promise.all` would cut
  focus-refresh latency substantially. **Fix:** two parallel `Promise.all`
  rounds (the second gated on `installed`, as before).

- [x] 6. **Small ones:**
  - `apply()` (`src-tauri/src/state.rs`) loads the preset from disk, and when
    offload is off `manager.apply_preset` loads it a second time — pass the
    already-parsed config through. **Fix:** new
    `Manager::apply_loaded_preset(name, config, tone)`; `apply_preset` (kept —
    the core integration tests use it) delegates to it.
  - ~~`Manager::apply_config` re-reads `config.txt` on every throttled drag
    write just to carry the provenance stamp.~~ **Deliberately skipped** on a
    closer look: the carried stamp must reflect what is actually in
    `config.txt` — a cache would need invalidation on every writer *including
    edits made by other tools* (which should drop the stamp; a cache would
    wrongly resurrect it). A ~kB read+parse behind the 75 ms live-apply
    throttle doesn't justify that risk.
  - `PresetStore::list` (`crates/fastpeq-core/src/store.rs`) used
    `sort_by_key(|n| n.to_lowercase())`, which re-allocates the key per
    comparison — switched to `sort_by_cached_key`.

## P3 — Duplication / refactoring

- [x] 7. **Editor.svelte duplicates its control strip between the two
  layouts.** The `PreampRow` invocation — including the non-trivial inline
  `onAutoPreampChange` handler — is copy-pasted verbatim in the collapsed panel
  and the expanded overlay. The file already uses `{#snippet}` for
  `headActions`/`bandActions`. **Fix:** new `eqControls` snippet
  (PreampRow + FilterList + bandActions) rendered by both layouts; the handler
  hoisted to a named `setAutoPreamp`.

- [x] 8. **Config→editor-state parsing exists twice in Editor.svelte.**
  `load()` and `configToCurve` both walk `cfg.lines` splitting
  preamp/balance/filters with identical logic. **Fix:** shared
  `parseConfigEq(cfg)` in `eq.ts` (returns filters/preamp/balance/hadPreamp/raw);
  `configToCurve` deleted, both paths use it.

- [x] 9. **The trailing-throttle pattern is hand-rolled twice.** Editor's
  `schedule`/`commit`/`lastApply`/`timer` and App's
  `pushTone`/`toneLast`/`toneTimer` are the same throttle-with-trailing-call.
  **Fix:** shared `createTrailingThrottle(fn, ms)` in `src/lib/throttle.ts`
  (with unit tests); both call sites migrated, `resetTone` uses its `flush()`.

- [x] 10. **`scrollCurrentIntoView` is defined identically in App.svelte and
  PresetsPanel.svelte**, both reaching into the DOM via global
  `document.querySelector`. **Fix:** one definition in PresetsPanel's
  `<script module>`, imported by App; the dead `presetListEl` bindable (item
  13) removed rather than wired up.

- [x] 11. **Editor bypasses the storage helpers it's supposed to use.**
  `src/lib/Editor.svelte` did raw `localStorage.getItem/setItem` with
  `typeof localStorage` guards for `autoPreamp`. **Fix:** `loadBool`/`save`
  from `storage.ts`. The `fastpeq:autoPreamp` key (colon, not dot) is kept so
  existing user settings survive.

## P4 — Dead code / nits

- [x] 12. `src/App.svelte` imported `getSpecialtyIcons` and `getBluetoothIcons`
  but never used them (only `getToneStep`) — removed. Same for the unused
  `tick` import in `PresetsPanel.svelte`.

- [x] 13. `presetListEl = $bindable()` in `src/lib/PresetsPanel.svelte` was
  bound to the `<ul>` but no parent bound it and nothing read it — removed
  (item 10 went the module-script route instead).

- [x] 14. `src/lib/Editor.svelte` mis-indented import / duplicate
  `prefs.svelte` import lines — merged.

- [x] 15. The `clamp` closure in `offload_band`
  (`crates/fastpeq-core/src/offload.rs`) now uses `f64::clamp`.

---

## Non-findings (looked at, deliberately not flagged)

- The `lock().unwrap()` style in `state.rs` — fine for an app where a poisoned
  mutex means a crashed thread.
- The eq.ts ↔ offload.rs math duplication — intentional and documented as
  mirrored implementations (device and on-screen curve must agree).
- The O(n²) `chosen.contains` in `split` — n ≤ ~30, irrelevant.
- `read_text_file`'s path handling — user-picked via dialog, size-capped,
  acknowledged in a comment.
