# OSD overlay intermittently doesn't appear — triage notes

**Status:** fix for hypothesis E (virtual-desktop cloaking, see 2026-07-11
evidence below) implemented on branch `osd-virtual-desktop-fix` — awaiting field
verification. If it proves insufficient, the fallback is the never-hide design
(see "Fallback plan: Option A" below).

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

## 2026-07-11 repro — virtual desktops implicate cloaking (hypothesis E)

Reproduced on a machine with **two Windows virtual desktops**. While the bug was
active (hotkeys registered and handled, no OSD visible on desktop 1):

- Firing a hotkey and **switching to desktop 2** showed the OSD there — the
  window *was* shown and fully rendered, just on the wrong desktop.
- Switching back to desktop 1 *after* the card had faded out → still broken.
- Switching to desktop 2 and back to desktop 1 **while the card was visible** →
  bug cleared; the OSD now appears on **both** desktops.
- Behavior identical with all windows minimized vs. visible. Onset is "after
  the app has been open for a while"; no specific trigger identified.

This rules out the earlier hypotheses for this variant: the page renders and the
event pipeline fires (not **A** throttling, not **D** focus gate), the card is
on top when on the right desktop (not **B**), and no hotkey burst is involved
(not **C**).

**E — virtual-desktop association / DWM cloaking.** Windows virtual desktops
track top-level windows per-desktop and *cloak* (DWM-hide) any window that
belongs to a non-current desktop; `ShowWindow` on a cloaked window "succeeds"
invisibly. Tool windows (`WS_EX_TOOLWINDOW`, no taskbar/Alt-Tab presence) are
normally **unmanaged** and render on every desktop — that's the healthy state,
and it's what the OSD returns to after the switch-while-visible maneuver forces
the shell to re-evaluate it. In the bad state the shell has (re)associated the
OSD window with one specific desktop, so every `show()` on the other desktop is
cloaked. Plausible (unconfirmed) triggers for the re-association: a desktop
switch landing while the card is visible/mid-fade, an explorer/DWM restart, or
the shell re-tracking the window on some later show. The "~10 s self-heal" in
the original report fits too — recovery is a shell re-evaluation (often a
desktop/foreground change), not a timer.

**Fix E (targeted) — implemented on `osd-virtual-desktop-fix`:** on each
`present()`, after `show()` resolves, the overlay invokes
`osd_ensure_on_current_desktop` ([src-tauri/src/commands.rs](../src-tauri/src/commands.rs)
→ `overlay::ensure_on_current_desktop` in
[src-tauri/src/lib.rs](../src-tauri/src/lib.rs)). Rust checks
`IVirtualDesktopManager::IsWindowOnCurrentVirtualDesktop(hwnd)` and, when false,
`MoveWindowToDesktop(hwnd, GetWindowDesktopId(GetForegroundWindow()))` — the
foreground window is on the current desktop by definition, and both interfaces
are documented shell COM APIs. The check must run *after* `show()`: it passes
vacuously for a hidden window. A successful move logs
`fastpeq: OSD was on another virtual desktop; moved to the current one` to
stderr, which doubles as field confirmation of the mechanism. Pinning via
`IVirtualDesktopPinnedApps` was rejected: undocumented, breaks across Windows
builds.

## Hypotheses (distinguishable by *when* it happens)

| # | Hypothesis | Tell-tale pattern | Why it self-heals |
|---|---|---|---|
| A | **Hidden-webview throttling** — WebView2 throttles a `visible:false` page's timers/rendering like a backgrounded browser tab | First OSD *after a quiet period* is the one that's missing; subsequent ones are fine | the webview wakes after a beat |
| B | **Topmost not re-raised** — `setAlwaysOnTop(true)` is called while the window is *already* topmost, which is a no-op that does not re-raise z-order; the card renders behind a foreground/fullscreen window | Happens while a game / video / another always-on-top window is in front | the other window yields the foreground |
| C | **show/hide IPC race** — a prior `dismiss()`'s `win.hide()` lands *after* a fresh `win.show()`, leaving the window hidden | Happens right after a rapid burst of hotkeys | the next event's `show()` wins the race |
| D | **Focus gate stuck true** (background variant) — a focus transition was missed so `windowFocused` never went `false` | Every OSD suppressed until the window is focused+unfocused once | next real focus event corrects the flag |

## Fallback plan: Option A — never hide the window

Kept in reserve in case the targeted fix E proves insufficient in the field.
The idea: keep the window permanently shown, transparent and click-through, and
animate only the `.shown` content-opacity class — `win.show()`/`win.hide()` go
away entirely. Details preserved here so the switch is cheap later:

**Changes**

- [src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json): leave
  `visible: false` (avoids a flash of unpainted window at launch); instead show
  once from [src/osd/main.ts](../src/osd/main.ts) after `position()` and
  `setIgnoreCursorEvents` — the card content is at `opacity: 0`, so nothing is
  visible.
- [src/osd/Osd.svelte](../src/osd/Osd.svelte):
  - `present()`: drop `win.show()`; keep the `shown = true` toggle and keep the
    `setAlwaysOnTop(true)` re-assert (still needed to re-raise z-order —
    hypothesis B).
  - `dismiss()`: drop `hideTimer` and `win.hide()`; the `.shown` fade-out *is*
    the dismissal.
  - **Keep the `osd_ensure_on_current_desktop` invoke**, moved to the start of
    `present()` (no `show()` to chain after; an always-visible window's desktop
    association is queryable at any time).
- [src-tauri/capabilities/osd.json](../src-tauri/capabilities/osd.json):
  `core:window:allow-show`/`allow-hide` become unused once main.ts's one-time
  show is the only caller (keep `allow-show` for that).

**What it fixes — and doesn't**

- Kills hypothesis A (no hidden webview to throttle) and C (no show/hide IPC
  pair to race); with the kept re-assert, B stays covered.
- Does **not** by itself fix E: a never-hidden window that the shell has
  associated with another desktop stays cloaked on the current one. It only
  removes the show() edge that plausibly *triggers* the re-association. The E
  re-anchor call must survive the switch.

**Tradeoffs / open questions**

- A permanently-visible always-on-top window can kick *exclusive-fullscreen*
  games out of fullscreen (modern borderless-fullscreen is unaffected). Since
  fastpeq may be used while gaming, evaluate mitigations before shipping: park
  the window off-screen while idle and reposition in `present()` (main.ts
  already does `setPosition`), or resize to 1×1 while idle.
- An always-composited transparent window costs a little GPU/memory
  persistently — likely negligible next to the webview itself.

## Rejected: Option B — keep show/hide, add raise/serialization hardening

Toggling `setAlwaysOnTop(false)`→`(true)`, promise-chaining show/hide, or a Rust
`SetWindowPos(HWND_TOPMOST, SWP_NOACTIVATE | SWP_SHOWWINDOW)` helper. These
harden against B/C only; the 2026-07-11 repro showed the real mechanism is E,
which none of this addresses. Revisit only if B- or C-pattern failures are
actually observed.

## Data to collect (to pick A vs B and confirm the mechanism)

> 2026-07-11: largely superseded by the virtual-desktop repro above — the
> remaining open question is what *triggers* the desktop re-association
> (desktop switch mid-fade? explorer restart?). Verifying the trigger is nice
> to have but not required to fix.

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
