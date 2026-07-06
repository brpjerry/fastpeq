<script lang="ts">
  // The overlay card. Listens for OSD_EVENT and shows a volume-indicator-style
  // card that updates in place (rapid tone steps coalesce into one card) and
  // auto-hides after a hold. The window is shown/hidden here; the no-activate
  // styles (set in Rust) keep show() from stealing focus.
  import { onMount } from "svelte";
  import { Spring } from "svelte/motion";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { OSD_EVENT, type OsdPayload } from "../lib/osd";

  const HOLD_MS = 1200; // visible after the last event
  const FADE_MS = 180; // keep in sync with the opacity transition below

  let payload = $state<OsdPayload | null>(null);
  let shown = $state(false);
  let holdTimer: ReturnType<typeof setTimeout> | null = null;
  let hideTimer: ReturnType<typeof setTimeout> | null = null;

  // The level bar glides to each new value instead of snapping. A tone hotkey can
  // be bound to a knob that fires many steps a second; a spring absorbs that —
  // `set` just retargets, and its own rAF loop coalesces to the frame rate, so no
  // matter how fast the steps arrive the fill smoothly chases the latest value.
  // High stiffness + heavy damping keeps the chase quick and overshoot-free.
  const barValue = new Spring(0, { stiffness: 0.3, damping: 0.9 });

  const win = getCurrentWindow();

  function present(p: OsdPayload) {
    // A newly-appearing card (or a switch to a different control) snaps the bar to
    // its starting value; only steps while the same card is already up animate.
    const fresh = !shown || !payload?.bar || payload.title !== p.title;
    if (p.bar) barValue.set(p.bar.value, fresh ? { instant: true } : undefined);
    payload = p;
    shown = true;
    win.show().catch(() => {});
    // Re-assert topmost on every show: a non-activating window created hidden can
    // drop out of the topmost z-order, and show() alone doesn't raise it back.
    win.setAlwaysOnTop(true).catch(() => {});
    if (hideTimer) {
      clearTimeout(hideTimer);
      hideTimer = null;
    }
    if (holdTimer) clearTimeout(holdTimer);
    holdTimer = setTimeout(dismiss, HOLD_MS);
  }

  function dismiss() {
    shown = false; // fade out, then hide the window once the transition is done
    hideTimer = setTimeout(() => win.hide().catch(() => {}), FADE_MS);
  }

  onMount(() => {
    const un = listen<OsdPayload>(OSD_EVENT, (e) => present(e.payload));
    return () => {
      un.then((f) => f());
      if (holdTimer) clearTimeout(holdTimer);
      if (hideTimer) clearTimeout(hideTimer);
    };
  });

  // Bipolar level bar: fill spans from the zero marker to the value. Fed by the
  // spring's animated value so the fill eases toward each step.
  const clamp01 = (n: number) => Math.max(0, Math.min(1, n));
  const fillPos = $derived(
    payload?.bar ? clamp01((barValue.current - payload.bar.min) / (payload.bar.max - payload.bar.min)) : 0,
  );
  const zeroPos = $derived(
    payload?.bar ? clamp01((0 - payload.bar.min) / (payload.bar.max - payload.bar.min)) : 0.5,
  );
</script>

<div class="osd" class:shown role="status" aria-live="polite">
  {#if payload}
    <div class="title">{payload.title}</div>
    {#if payload.detail}<div class="detail">{payload.detail}</div>{/if}
    {#if payload.bar}
      <div class="bar">
        <span class="zero" style="left:{zeroPos * 100}%"></span>
        <span class="fill" style="left:{Math.min(zeroPos, fillPos) * 100}%; width:{Math.abs(fillPos - zeroPos) * 100}%"></span>
      </div>
    {/if}
  {/if}
</div>

<style>
  /* Sized to its content and centered by #osd, with transparent window space
     around it so all four corners round and the shadow isn't clipped. */
  .osd {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 240px;
    /* Hard cap so the card (and its shadow) can never exceed the window's safe
       area, even for very long device names; the detail line ellipsizes. */
    max-width: 350px;
    padding: 12px 16px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 5px 14px rgba(0, 0, 0, 0.5);
    opacity: 0;
    transform: translateY(8px);
    transition:
      opacity 180ms ease,
      transform 180ms ease;
  }
  .osd.shown {
    opacity: 1;
    transform: translateY(0);
  }
  .title {
    font-size: 12px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .detail {
    font-size: 20px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bar {
    position: relative;
    height: 6px;
    margin-top: 4px;
    border-radius: 3px;
    background: var(--panel-2);
    overflow: hidden;
  }
  .fill {
    position: absolute;
    top: 0;
    bottom: 0;
    background: var(--accent);
    border-radius: 3px;
  }
  .zero {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    background: var(--border);
  }
</style>
