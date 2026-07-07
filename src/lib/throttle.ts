// Rate-limiting helpers.
//
// A trailing-edge throttle: `schedule()` runs `fn` at most once per `ms`,
// firing immediately when the window is clear and otherwise coalescing the
// burst into one trailing call — so the final value of a drag always lands.
// Shared by the editor's live-apply and the tone panel's knob writes.
//
// A debounce: `schedule()` runs `fn` once, `ms` after the *last* call — no
// leading call, so a burst does nothing until it settles. For work where only
// the settled state matters (re-registering OS hotkeys while the user types).

export interface TrailingThrottle {
  /** Request a run: immediate if the window is clear, else one trailing call. */
  schedule(): void;
  /** Run `fn` right now (resetting the window), dropping any pending call. */
  flush(): void;
  /** Drop any pending trailing call without running it. */
  cancel(): void;
}

export interface Debounce {
  /** (Re)start the timer: `fn` runs `ms` after the most recent call. */
  schedule(): void;
  /** Drop any pending run without executing it. */
  cancel(): void;
}

export function createDebounce(fn: () => void, ms: number): Debounce {
  let timer: ReturnType<typeof setTimeout> | null = null;
  return {
    schedule() {
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        fn();
      }, ms);
    },
    cancel() {
      if (timer !== null) clearTimeout(timer);
      timer = null;
    },
  };
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
