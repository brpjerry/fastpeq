# fastpeq code review — action list

Findings from a full pass over the Rust core, Tauri shell, and Svelte frontend
(clippy clean, 28 Rust tests passing). Work top‑down; check items off as done.

## P1 — Correctness bugs
- [x] 1. Window close quits the app instead of hiding to tray. Add a `CloseRequested` handler in `src-tauri/src/lib.rs` → `window.hide()` + prevent close, so the tray stays functional. ("Quit fastpeq" remains the real exit.)
- [x] 2. Global hotkey Ctrl+Alt+B only bypasses, never restores (comment says "toggles"). Make it a true toggle; share the toggle across hotkey, tray, and the Bypass button, and keep the frontend in sync.
- [x] 3. `svelte-check` type error: `categories: Record<string,string>` is assigned `undefined` (`src/App.svelte`). Keep the map free of `undefined` (delete the key to clear).

## P2 — Performance
- [x] 4. `active_preset()` (`crates/fastpeq-core/src/manager.rs`) reads & parses every preset on each call; called on every tray refresh, reload, and window focus. Cache contents/hashes or track active in memory.
- [x] 5. Frontend↔tray reload loop: each action → `tray::refresh` → `fastpeq:changed` → full frontend `reload()`. De‑dupe self‑triggered events or return data from commands.
- [x] 6. `apply_config` re-reads `.tone.json` from disk on every throttled drag write. Cache tone in `Inner`.

## P3 — Cleanup / duplication
- [x] 7. Remove dead `get_preset_text`/`save_preset_text` commands + `state.get_text`/`save_text` (no frontend caller).
- [x] 8. Consolidate the 3–4 near-identical dropdown/popover implementations (`TypeSelect`, App category menu, App type-filter, Editor balance popover) into one reusable component/action.
- [x] 9. Extract shared graph-plotting helpers shared by `ResponseCurve` and `CurveEditor` (xOf/yOf/pathFor/grids/preamp-centering).
- [x] 10. Centralize the repeated `localStorage` try/catch helpers (`prefs`, `theme`, `starter`).

## P4 — Robustness / hardening
- [x] 11. `save_settings` uses non-atomic `fs::write` (`src-tauri/src/state.rs`); route through `write_text_atomic`.
- [x] 12. `is_safe_name` doesn't reject Windows reserved names (CON, NUL, COM1…, trailing dot/space) (`crates/fastpeq-core/src/store.rs`).
- [x] 13. `csp: null` in `tauri.conf.json` — set a restrictive CSP before public release. (Verified in a release build: app renders and IPC loads presets under the CSP.)

## P5 — Stale comments
- [x] 14. `src/App.svelte` cycle-order comment is wrong ("none → speaker → headphone → IEM").
- [x] 15. `src-tauri/src/lib.rs` "toggles bypass" comment (tied to #2) — corrected during #2.

## P6 — Testing
- [x] 16. Add frontend unit tests (vitest) for pure logic: `eq.ts` biquad math, `measurement.ts` interpolation, `starter.ts`, `balanceTrim`. (28 tests across eq/measurement/graph/starter; `npm test`.)

## P7 — Repo hygiene (before going public)
- [x] 17. Add a root `LICENSE` file; add a `license` field to the `src-tauri` package (core declares MIT).
- [x] 18. Rename bundle identifier `com.fastpeq.app` → `com.fastpeq.desktop` (Tauri warned the `.app` suffix conflicts with macOS). Note: changes the app data dir — existing presets live under the old `com.fastpeq.app` dir.
- [x] 19. Add CI (`.github/workflows/ci.yml`: build + `cargo test` + `cargo clippy -D warnings` + `cargo fmt --check` + `npm run check` + `npm test`).
- [x] 20. Ran `cargo fmt --all` and adopted canonical formatting.

## Follow-ups — data/retention model (post-0.4)
The MVP assumed the live `config.txt` is a faithful, reversible projection of the
active preset, so "which preset is active" can be recovered by comparing the live
config against the library. The feature set has eroded that: Auto Preamp rewrites
the master `Preamp:` at write time, the tone overlay injects/strips lines, and the
editor reorders lines — so the live config is now a *lossy, non-deterministic*
function of the preset. `Config::is_equivalent` (`apo/model.rs`) is the third patch
over this crack. Worth retiring the inference model rather than patching it again:

- [ ] 21. **Provenance, not inference.** On apply, stamp the live config with a
  marker comment (`# fastpeq:preset=<name>` + a hash of the base EQ). APO ignores
  `#` lines and the model round-trips them as `Line::Raw`, so `active_preset()`
  becomes O(1) and exact, immune to Auto Preamp, and can report "active but
  modified" when a hand-edit changes the hash. Keep `is_equivalent` as the
  fallback for externally-edited configs. Also resolves perf item #4 (no library
  scan). Note the inherent limit it removes: two presets differing *only* by master
  gain are indistinguishable by content once Auto Preamp overwrites that field.
- [ ] 22. **Per-preset editor metadata is keyed by name in `localStorage`**
  (target, target offset, align freq, measurement, show-ref flags, compensate) —
  unlike categories/tone, which are Rust sidecars that `rename_preset` migrates.
  So renaming a preset silently orphans its target + measurement, and the metadata
  doesn't travel when a preset is shared/backed up. Move it to a sidecar next to
  the preset (or a Rust-managed store) so it migrates on rename and exports.
