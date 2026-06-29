# OSD overlay intermittently doesn't appear — triage notes

**Status:** open, collecting data. No fix applied yet.

## Symptom (as reported)

> Occasionally notifications will stop showing up, but this issue goes away
> after 10 seconds or so.

Confirmed with the reporter: the missing surface is the **OSD overlay** (the
volume-indicator-style card near the bottom of the screen), seen when the main
window is **minimized to the tray or in the background** — not the in-window
toast.

## How the OSD works today

- A hotkey fires → backend emits `hotkey-pressed` → `dispatchHotkey` runs the
  action, then `maybeOsd(h)` ([src/App.svelte](../src/App.svelte)).
- `maybeOsd` is gated: `if (windowFocused) return;` — it only emits when the
  main window is **not** focused ([src/App.svelte:99](../src/App.svelte)). It
  then `emit(OSD_EVENT, payload)`.
- The separate `osd` window listens and runs `present()` →
  `win.show()` + `setAlwaysOnTop(true)`, holds 1200 ms, then `dismiss()` fades
  and `win.hide()`s ([src/osd/Osd.svelte:21-39](../src/osd/Osd.svelte)).
- The window is created `visible:false`, `alwaysOnTop`, `skipTaskbar`,
  no-activate / tool-window styles
  ([src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json),
  [src-tauri/src/lib.rs](../src-tauri/src/lib.rs) `overlay::make_noactivate`).

## Why the focus gate is *not* the cause here

When the window is minimized/background it is unfocused, so `windowFocused` is
`false` and the gate is **open** — the emit does fire. The failure is therefore
**downstream, inside the `osd` window** (show / topmost / render), not in the
gate. (There's a separate, weaker possibility that a *missed* focus event leaves
`windowFocused` stuck `true`; see hypothesis-D below. Lower priority because the
reporter is in the minimized/background case.)

## "~10 seconds" is not a constant

Every `setTimeout`/`setInterval` in the frontend was enumerated. There is **no**
~10 s timer; the OSD's only timers are `HOLD_MS = 1200` and `FADE_MS = 180`
([src/osd/Osd.svelte:11-12](../src/osd/Osd.svelte)). So "~10 s" is a
self-healing *transient state* clearing, not a coded delay.

## Hypotheses (distinguishable by *when* it happens)

| # | Hypothesis | Tell-tale pattern | Why it self-heals |
|---|---|---|---|
| A | **Hidden-webview throttling** — WebView2 throttles a `visible:false` page's timers/rendering like a backgrounded browser tab | First OSD *after a quiet period* is the one that's missing; subsequent ones are fine | the webview wakes after a beat |
| B | **Topmost not re-raised** — `setAlwaysOnTop(true)` is called while the window is *already* topmost, which is a no-op that does not re-raise z-order; the card renders behind a foreground/fullscreen window | Happens while a game / video / another always-on-top window is in front | the other window yields the foreground |
| C | **show/hide IPC race** — a prior `dismiss()`'s `win.hide()` lands *after* a fresh `win.show()`, leaving the window hidden | Happens right after a rapid burst of hotkeys | the next event's `show()` wins the race |
| D | **Focus gate stuck true** (background variant) — a focus transition was missed so `windowFocused` never went `false` | Every OSD suppressed until the window is focused+unfocused once | next real focus event corrects the flag |

## Candidate fixes

**Option A (recommended, robust): stop fully hiding the window.** Keep it
`show()`n once, transparent + click-through, and animate only the `.shown`
opacity class instead of `win.hide()`/repeated `win.show()`. This removes the
hidden page (kills A), the re-hide (kills B), and the show/hide race (kills C) in
one move.

- Tradeoff: a permanently always-on-top window can kick *exclusive-fullscreen*
  games out of fullscreen (modern borderless-fullscreen is unaffected). Since
  fastpeq may be used while gaming, confirm this is acceptable, or keep it
  zero-opacity/off-screen when idle.

**Option B (conservative): keep show/hide, make it reliable.**
- (a) Force the re-raise by toggling `setAlwaysOnTop(false)` → `true` on show.
- (b) Serialize show/hide through a promise chain so a stale `hide()` can't land
  after a `show()`.
- (c) Optionally add a Rust `SetWindowPos(HWND_TOPMOST, SWP_NOACTIVATE | SWP_SHOWWINDOW)`
  helper for a dependable raise.
- No fullscreen risk; more moving parts.

## Data to collect (to pick A vs B and confirm the mechanism)

- [ ] Does it fail mainly on the **first** hotkey after the app/screen has been
      idle a while? → points to **A**.
- [ ] Does it fail while a **game / video / fullscreen** app is in front? → **B**.
- [ ] Does it fail right after **mashing** several hotkeys quickly? → **C**.
- [ ] Roughly how long until it recovers (is "~10 s" consistent)?
- [ ] Does focusing then re-minimizing the main window fix it immediately? → **D**.
- [ ] Multi-monitor? Which monitor is the OSD parked on vs. the focused app?
- [ ] Does the card appear *late* (delayed) or not at all during the bad window?

## Files involved

- [src/osd/Osd.svelte](../src/osd/Osd.svelte) — present/dismiss, show/hide, topmost.
- [src/osd/main.ts](../src/osd/main.ts) — window position, click-through.
- [src/App.svelte](../src/App.svelte) — `maybeOsd`, focus gate, emit.
- [src-tauri/src/lib.rs](../src-tauri/src/lib.rs) — `overlay::make_noactivate` (where a Rust raise helper would go).
- [src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json) — `osd` window config.

## Verification

Unit tests can't catch this (Windows window-manager timing). Repro manually with
a debug build: `npx tauri build --debug --no-bundle` then `npm run e2e` infra, or
just run the debug binary and exercise hotkeys per the patterns above.
