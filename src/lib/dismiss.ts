// A Svelte action for floating menus / popovers: call `onDismiss` when the user
// clicks outside, presses Escape, or scrolls/resizes (which would strand a
// fixed-positioned menu). Replaces the per-component backdrop element + close
// effect that every dropdown used to carry its own copy of.
//
//   <div class="menu" use:dismissable={{ onDismiss: () => (open = false), ignore: triggerEl }}>
//
// `ignore` (usually the trigger) is treated as "inside", so clicking the trigger
// toggles the menu via its own handler instead of being dismissed first.

export interface DismissOptions {
  onDismiss: () => void;
  ignore?: HTMLElement | null;
}

export function dismissable(node: HTMLElement, opts: DismissOptions) {
  let current = opts;
  const inside = (t: EventTarget | null) =>
    t instanceof Node && (node.contains(t) || !!current.ignore?.contains(t));

  // Pointerdown (capture) registers only now — after the gesture that opened the
  // menu — so the opening click can't immediately dismiss it.
  const onDown = (e: PointerEvent) => {
    if (!inside(e.target)) current.onDismiss();
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") current.onDismiss();
  };
  // Scrolling within the menu is fine; an outer scroll would misalign a fixed menu.
  const onScroll = (e: Event) => {
    if (!(e.target instanceof Node) || !node.contains(e.target)) current.onDismiss();
  };
  const onResize = () => current.onDismiss();

  document.addEventListener("pointerdown", onDown, true);
  document.addEventListener("scroll", onScroll, true);
  window.addEventListener("keydown", onKey);
  window.addEventListener("resize", onResize);

  return {
    update(next: DismissOptions) {
      current = next;
    },
    destroy() {
      document.removeEventListener("pointerdown", onDown, true);
      document.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onResize);
    },
  };
}
