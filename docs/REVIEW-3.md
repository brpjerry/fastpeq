# fastpeq — code review #3 (post hardware-offload)

Third pass over the codebase after the hardware EQ offload work landed (PR #27).
Reviews #1 (`REVIEW.md`) and #2 (`REVIEW-2.md`) are fully worked through; this is
a fresh list. Health at review time: clippy clean, svelte-check clean, 176
frontend + all Rust tests green, no TODO/FIXME markers.

**Status:** ✅ P1 (items 1–3) and P2 item 4 done · ⬜ items 5–15 open.

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

- [ ] 5. **`reload()` serializes eight IPC round-trips** (`src/App.svelte`):
  `apoStatus`, `presetsDir`, `hardwareStatus`, then `listPresets`,
  `presetCategories`, `activePreset`, `bypassed`, `getTone` one at a time — and
  it runs on every window focus. Most are independent; `Promise.all` would cut
  focus-refresh latency substantially.

- [ ] 6. **Small ones:**
  - `apply()` (`src-tauri/src/state.rs`) loads the preset from disk, and when
    offload is off `manager.apply_preset` loads it a second time — pass the
    already-parsed config through.
  - `Manager::apply_config` (`crates/fastpeq-core/src/manager.rs`) re-reads and
    re-parses `config.txt` on every throttled drag write just to carry the
    provenance stamp; the shell already caches active-preset state and could
    supply it.
  - `PresetStore::list` (`crates/fastpeq-core/src/store.rs`) uses
    `sort_by_key(|n| n.to_lowercase())`, which re-allocates the key per
    comparison — `sort_by_cached_key` is the drop-in fix.

## P3 — Duplication / refactoring

- [ ] 7. **Editor.svelte duplicates its control strip between the two
  layouts.** The `PreampRow` invocation — including the non-trivial inline
  `onAutoPreampChange` handler — is copy-pasted verbatim in the collapsed panel
  and the expanded overlay. The file already uses `{#snippet}` for
  `headActions`/`bandActions`; wrap `PreampRow` + `FilterList` in a third
  snippet and hoist the handler to a named function.

- [ ] 8. **Config→editor-state parsing exists twice in Editor.svelte.**
  `load()` and `configToCurve` both walk `cfg.lines` splitting
  preamp/balance/filters with identical logic. Extract one shared
  "parse config into {filters, preamp, balance, raw}" helper.

- [ ] 9. **The trailing-throttle pattern is hand-rolled twice.** Editor's
  `schedule`/`commit`/`lastApply`/`timer` and App's
  `pushTone`/`toneLast`/`toneTimer` are the same throttle-with-trailing-call.
  Extract a shared `createTrailingThrottle(fn, ms)` utility (also makes the
  timers impossible to leak).

- [ ] 10. **`scrollCurrentIntoView` is defined identically in App.svelte and
  PresetsPanel.svelte**, both reaching into the DOM via global
  `document.querySelector`. Either lift into a shared helper, or let
  `PresetsPanel` own it — the unused `presetListEl` bindable (item 13) was
  seemingly added for exactly this and never wired up.

- [ ] 11. **Editor bypasses the storage helpers it's supposed to use.**
  `src/lib/Editor.svelte` does raw `localStorage.getItem/setItem` with
  `typeof localStorage` guards for `autoPreamp` — the `loadBool`/`save`
  wrappers in `src/lib/storage.ts` were centralized in review #1 for precisely
  this. (Nit: the key is `fastpeq:autoPreamp` while everything else uses the
  `fastpeq.` dot style.)

## P4 — Dead code / nits

- [ ] 12. `src/App.svelte` imports `getSpecialtyIcons` and `getBluetoothIcons`
  but never uses them (only `getToneStep` is used there).

- [ ] 13. `presetListEl = $bindable()` in `src/lib/PresetsPanel.svelte` is bound
  to the `<ul>` but no parent binds it and nothing reads it — dead API surface
  (or the missing piece of item 10).

- [ ] 14. `src/lib/Editor.svelte` has a mis-indented import, and `prefs.svelte`
  is imported on two separate lines — merge them.

- [ ] 15. The `clamp` closure in `offload_band`
  (`crates/fastpeq-core/src/offload.rs`) can just be `f64::clamp`.

---

## Non-findings (looked at, deliberately not flagged)

- The `lock().unwrap()` style in `state.rs` — fine for an app where a poisoned
  mutex means a crashed thread.
- The eq.ts ↔ offload.rs math duplication — intentional and documented as
  mirrored implementations (device and on-screen curve must agree).
- The O(n²) `chosen.contains` in `split` — n ≤ ~30, irrelevant.
- `read_text_file`'s path handling — user-picked via dialog, size-capped,
  acknowledged in a comment.
