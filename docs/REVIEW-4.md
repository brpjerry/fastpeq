# fastpeq — code review #4 (deep-dive audit)

Fourth pass over the whole codebase (core, hardware crate, Tauri shell, frontend,
CLI). Reviews #1–#3 are fully worked through. Health at review time: clippy
clean, svelte-check clean, 224 frontend + all Rust tests green, no TODO/FIXME
markers. The P1 items below were verified against the real code (item 1 with a
reproducing test), not just read off the page.

**Status:** 🟨 P1 items (1–3) fixed; item 4 moved to `PRESET-HISTORY.md`; the
rest open.

---

## P1 — Bugs

- [x] 1. **`parse()` panics on multi-byte UTF-8 at a prefix boundary — a
  non-ASCII comment in `config.txt` crashes the app.** `strip_prefix_ci`
  (`crates/fastpeq-core/src/apo/parse.rs:52`) does `line[..prefix.len()]` after
  only a *byte-length* check, and `strip_filter_prefix` (`parse.rs:98`) does the
  same with `line[..6]`. If byte 6/7/8 of a line falls inside a multi-byte
  character, the slice panics. **Verified:** `parse("#中文注释")` panics
  (`end byte index 8 is not a char boundary; it is inside '注'`); `"Prea😀 x"`
  and `"Chan😀 y"` panic too. `parse()` runs on every `config.txt` read —
  `current_config()` on every apply, tone change, and active-preset derive — so
  one hand-written comment in the user's config (Chinese/emoji/accented text is
  entirely plausible in an APO config PEACE users share) takes the whole app
  down. **Fix:** make the comparison byte-wise so no slice is taken —
  `line.as_bytes().get(..prefix.len()).is_some_and(|b| b.eq_ignore_ascii_case(prefix.as_bytes()))`
  (prefixes are pure ASCII, so a mid-character "match" is impossible and the
  remainder slice is then boundary-safe) — or guard with
  `line.is_char_boundary(n)`. Same treatment for the `[..6]` in
  `strip_filter_prefix`. Add the three lines above as regression tests.
  **Fixed:** `strip_prefix_ci` now compares byte-wise via `as_bytes().get(..)`,
  and `strip_filter_prefix` delegates to it instead of slicing `[..6]` itself.
  Regression test `non_ascii_lines_are_preserved_not_panicked_on` in
  `tests/roundtrip.rs` covers all three offsets plus round-trip preservation.

- [x] 2. **Driver reply decoders index unchecked into device replies, and a
  decoder panic leaves the session lying about its state.** `read_reply` only
  guarantees `payload.len() > 1` (moondrop/walkplay) or `> 6` (fiio), but:
  - `decode_version` reads `p[3..]` and falls back to `p[3], p[4], p[5]`
    (`moondrop.rs:249`, `walkplay.rs:305`) — a 2-byte reply panics;
  - `decode_band` reads `p[27..33]` (`walkplay.rs:321`, reachable via `pull`)
    and `p[7..13]` (`fiio.rs:418`) — a short parameter reply panics.

  These run on the worker thread (`worker.rs::run`), and a panic there unwinds
  *past* the trailing `set_status(connected = false)` with no error recorded —
  the UI keeps showing a connected session with a firmware version while the
  worker is dead (the next push only fails at the channel send). Two fixes,
  both worth doing: (a) have `read_reply` take/enforce a minimum payload length
  per command (or switch decoders to `p.get(..)` with defaults), and (b) make
  the status update panic-proof — set it from a drop guard, or wrap the loop
  body in `catch_unwind`, so `connected` can never be left stale.
  **Fixed:** every decoder (`decode_version`/`decode_band` × 3 drivers, plus
  the pregain-probe test's raw reads) is now total — missing bytes read as
  zero via `p.get(..)` — with a `short_replies_decode_without_panicking` test
  per driver; and `run()` holds a `DisconnectOnExit` drop guard that clears
  `connected` (recording a "Hardware worker crashed" error) on any exit,
  panics included (`a_panicking_worker_still_reads_as_disconnected`).

- [x] 3. **The push worker can drop a pending flash commit on shutdown.**
  `worker.rs` coalesces pushes into `pending` and promises "a requested commit
  sticks until the next flush so a flash save is never dropped" — but the loop
  `break`s on `Command::Stop` *and* on channel disconnect (app quit drops the
  `Sender`) before flushing `pending`. A commit queued within the 60 ms
  throttle window when the session stops (editor flashes on mouse release →
  user immediately quits or the reconciler closes the session) is silently
  discarded. **Fix:** on loop exit, flush a still-pending push if it carries
  `commit` (best-effort, ignore errors) before releasing the device.
  **Fixed:** the loop was split into a testable `run_loop(&mut dyn HardwareEq,
  …)`; on exit it flushes a pending *commit* (after honoring the remaining
  throttle interval, skipping if identical to the last write) and drops
  volatile pending state as before. Covered by
  `pending_commit_is_flushed_on_stop` / `…_on_disconnect` /
  `volatile_pushes_are_never_flash_committed_on_stop` with a mock device.

## P2 — Design questions / robustness edges

- 4. *Moved.* Unconfirmed one-click preset deletion is folded into the preset
  versioning plan — see `PRESET-HISTORY.md` (undo-delete ships as its Phase 2,
  on top of general per-preset version history).

- [x] 5. **Every preset click / bypass / mode change writes the device's
  flash — question whether that's the right default.** `AppState::apply`,
  `toggle_bypass` (via `clear_hardware_eq` → `push_commit`), un-bypass, and
  `set_offload_mode` all send `commit = true`, while `sync_offload`
  deliberately uses RAM-only pushes ("following the output shouldn't wear the
  device's flash"). Commit is *required* on the DHA15 (`commit_to_apply` —
  nothing latches without it), but on the KA17/Space Pro a volatile write is
  audible immediately, and a user cycling presets from the tray/hotkey writes
  the KA17's USER-slot flash on every press (the same firmware whose legacy
  save could fault the device). **Fix suggestion:** commit only when
  `profile.commit_to_apply`, or debounce commits (volatile immediately, flash
  after N seconds of quiet); keep the explicit behavior for the DHA15.
  **Fixed:** the worker now has a `CommitPolicy` — `Immediate` for
  commit-to-apply devices (unchanged DHA15 behavior), `Debounced(2 s)`
  otherwise: pushes apply volatile instantly and one flash is written for the
  final state after the device goes quiet (deadline restarts on further
  writes; still flushed on shutdown; identical re-commits still skipped via a
  `flashed` cache). Tests: `debounced_commits_coalesce_into_one_flash`,
  `identical_recommit_never_reflashes`. PERSISTENCE.md updated.

- [x] 6. **`HardwareStatus.active` ignores whether the worker is actually
  connected.** `state.rs::hardware_status` reports `active: enabled` whenever a
  session object exists; `RuntimeStatus.connected` is never read by the GUI (only
  the CLI uses it). A session whose worker died (device unplugged, or item 2's
  panic) still shows "Offloading to <device>" with the error line below as the
  only hint. Consider `active: enabled && rt.connected` — or, if the brief
  startup-connect flicker is the concern, an explicit tri-state
  (connecting / offloading / error) since `reconciled` already exists for the
  startup case.
  **Fixed:** `active` is now `enabled && rt.connected`. The startup race is
  closed at the source: `HardwareSession::wait_ready(timeout)` blocks
  (bounded, off the UI thread) until the open settles, called by
  `sync_offload` right after `start` — it also replaces the CLI `session`
  command's hand-rolled copy of the same poll. `HardwarePanel` gained a
  distinct "connection to <device> is down" status line for the
  session-present-but-disconnected case.

- [x] 7. **`HardwareSession::start` swallows a thread-spawn failure.** The
  `Builder::spawn(...).ok()` leaves `join: None` and the status at its default
  (`connected: false`, `error: None`) — the session looks like it's connecting
  forever. Vanishingly rare, but one line fixes it: on spawn error, record it
  in `status.error` before constructing the session.
  **Fixed:** the spawn result is matched; on error, `status.error` records
  "Could not start the hardware worker: <e>" (which also makes
  `wait_ready` return immediately instead of timing out).

## P3 — Code reuse / organization

- [x] 8. **Extract the shared HID-driver plumbing — the largest duplication in
  the repo.** `moondrop.rs` and `walkplay.rs` are byte-identical in
  `coeff_bytes`, `le16`, `decode_version`, `decode_band`, `FLAT_BAND`, the
  type-code table (`TYPE_PK/LSQ/HSQ` with the same values), `dry_run`, and
  near-identical in `send` (zero-pad + report id + dry-run log + inter-packet
  pace) and `read_reply` (read-until-2-byte-match with deadline); `fiio.rs`
  re-declares `dry_run`, `FLAT_BAND`, and the same `send`/`read_reply` shape
  again (~150 duplicated lines across the three). Suggested split:
  - `hw/src/common.rs` (or `protocol.rs`): paced `send_report(dev, report_id,
    payload, pace)`, `read_matching(dev, timeout, match_fn, min_len)` (folds in
    item 2's length guard), the `dry_run()` guard, and `FLAT_BAND`;
  - `hw/src/moondrop_family.rs`: the shared packet codec (`coeff_bytes`,
    `le16`, band packet layout, `decode_band`, `decode_version`) parameterized
    by the two real differences (byte 35 slot tag, per-band enable vs apply-all).

  The per-driver files then keep only what's genuinely per-device: identify,
  profile, command bytes, and the push/commit sequences.
  **Fixed:** exactly this split — `protocol.rs` (paced `send_report`,
  `read_matching`, `drain_input`, `dry_run`, `FLAT_BAND`, `READ_TIMEOUT`) and
  `moondrop_family.rs` (report constants, `write_packet(…, slot)`,
  `decode_band`/`decode_version`, `le16`/`coeff_bytes`, the shared
  `read_reply` matcher). moondrop.rs 485→337 lines, walkplay.rs 596→454;
  the duplicated codec tests were deduped into family tests while each
  driver keeps its own byte-layout test (they pin the differing slot byte).

- [x] 9. **Editor.svelte re-asserts the live config with the same guarded call
  in three places.** `api.applyLive(buildConfig(false), livePregain).catch(...)`
  under (a variant of) `(effectiveAuto || offloadActive) && !loading &&
  !comparing` appears in the `hwBandIdx` effect (~line 109), `load()`'s
  `finally` (~line 415), and the tone-change effect (~line 599). Extract a
  `reassertLive()` helper (the guard included) so the condition can't drift
  between the three — two of the past P1 bugs (#REVIEW-3 items 1–2) lived in
  exactly this logic.
  **Fixed:** `reassertLive()` owns the `loading`/`comparing`/derived-stage
  guard; the three sites call it (load additionally gates on `!err`, its
  one genuine difference).

- [ ] 10. **The editor band type is declared three times.** `type Band` in
  `Editor.svelte`, an identical `type Band` in `CurveEditor.svelte`, and
  `EditorBand` in `history.svelte.ts`. Export one (`EditorBand` already lives
  in the one plain-TS module of the three) and import it everywhere.

- [ ] 11. **`set_hotkey_bindings` re-implements the UI-state validation it sits
  next to.** `hotkeys.json` is exactly a UI-state document with `JsonShape::
  Array` (`state.rs:940` vs the `UI_STATE_DOCS` table). Add `("hotkeys",
  Array)` to the table and make `set_hotkey_bindings`/`hotkey_bindings` thin
  wrappers over `set_ui_state`/`ui_state` (keeping the command names for
  frontend compat) — deletes the ad-hoc validator and its divergent error copy.

- [ ] 12. **`set_presets_dir` / `reset_presets_dir` are the same function.**
  Both do load-settings → mutate `presets_dir` → save → `build_inner` →
  replace `inner` (`state.rs:403`). Fold into one
  `update_presets_dir(&self, dir: Option<PathBuf>)`.

- [ ] 13. **`split` and `selected_filter_positions` build their candidate lists
  with two hand-written loops** that must stay in sync (same
  enabled/Both/`offload_band` predicate, different keying —
  `offload.rs:169` vs `:224`). A shared iterator yielding `(line_idx,
  filter_pos, band)` lets both keep their keying while sharing the predicate.

- [ ] 14. **App.svelte hand-rolls a debounce for hotkey registration**
  (`hkTimer`, ~line 154) with the timer variable outliving the effect, while
  `throttle.ts` exists precisely to name this pattern. Add a `createDebounce`
  sibling (or an option on the throttle) and migrate — the module-level `let`
  and double-clear logic go away.

## P4 — Dead code / nits

- [ ] 15. **`Config::is_equivalent` is dead production code.** Nothing outside
  tests calls it (grep: only `model.rs` tests and `storage.rs` integration
  tests) — the provenance stamp replaced content-equivalence as the
  active-preset mechanism, and its 30-line doc comment still narrates the old
  design as if it were load-bearing. Either delete it (and its tests) or move
  it into the test support explicitly; keeping it public invites someone to
  build on the abandoned model.

- [ ] 16. **`restoreSnap` in `Editor.svelte` (~line 450) is dead** — never
  called; the `createHistory` callback (~line 429) duplicates its body. Delete
  one and use the other as the callback.

- [ ] 17. **`Instant::now() - MIN_INTERVAL` (`worker.rs:139`) can panic in
  theory** (an `Instant` too close to its epoch can't be rewound). Use
  `Instant::now().checked_sub(MIN_INTERVAL).unwrap_or_else(Instant::now)` or
  track "never written" with an `Option<Instant>`.

- [ ] 18. **`is_reserved_device` (`store.rs`) doesn't cover `CONIN$`/`CONOUT$`
  or the superscript-digit COM names (`COM¹`…`COM³`)** that newer Windows also
  reserves. Genuinely obscure; note it in the comment or extend the check —
  either is fine, silence isn't.

---

## Non-findings (looked at, deliberately not flagged)

- The `audio.rs` COM/`IPolicyConfig` unsafe block — vtable padding, `ComGuard`
  semantics (S_FALSE pairing, RPC_E_CHANGED_MODE non-ownership), and
  `take_pwstr` ownership are all correct as written.
- `sync_offload`'s `try_lock` no-op pattern and the `last_sync` cache — the
  failed-open reset from review #3 item 1 is in place; the remaining races are
  serialized by `sync_guard` or benign for a single-user desktop app.
- The eq.ts ↔ offload.rs mirrored biquad math — still intentional and
  documented (device and on-screen curve must agree); re-checked that the
  RBJ coefficients and probe grids actually match.
- `Manager::apply_preset` kept alongside `apply_loaded_preset` — used by the
  core integration tests, documented.
- `peakGainDb`'s `Math.max(...left, ...right)` spread — 480 arguments, well
  under engine limits.
- The three localStorage-migration stores (`hotkeys`, `prefs`, `preset-view`)
  share an init/persist/parse shape that *looks* extractable, but the
  migration rules differ per store (seeded default vs legacy per-key read vs
  plain blob) and each is under 40 lines of mechanism — a generic
  `createBackedStore` would trade three readable modules for one abstraction
  with three configuration knobs. Not worth it at this count; reconsider at
  the fourth store.
- `flash()`'s timer-overlap guard in App.svelte (`message === m`) — correct.
- The E2E fake 450 ms refresh delay — deliberate UX, commented.
