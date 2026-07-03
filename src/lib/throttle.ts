// A trailing-edge throttle: `schedule()` runs `fn` at most once per `ms`,
// firing immediately when the window is clear and otherwise coalescing the
// burst into one trailing call — so the final value of a drag always lands.
// Shared by the editor's live-apply and the tone panel's knob writes.

export interface TrailingThrottle {
  /** Request a run: immediate if the window is clear, else one trailing call. */
  schedule(): void;
  /** Run `fn` right now (resetting the window), dropping any pending call. */
  flush(): void;
  /** Drop any pending trailing call without running it. */
  cancel(): void;
}

export function createTrailingThrottle(fn: () => void, ms: number): TrailingThrottle {
  let last = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  function fire() {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    last = Date.now();
    fn();
  }

  return {
    schedule() {
      const elapsed = Date.now() - last;
      if (timer !== null) clearTimeout(timer);
      if (elapsed >= ms) fire();
      else timer = setTimeout(fire, ms - elapsed);
    },
    flush: fire,
    cancel() {
      if (timer !== null) clearTimeout(timer);
      timer = null;
    },
  };
}
