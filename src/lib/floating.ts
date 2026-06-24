// Positioning for floating menus (the custom dropdowns). Shared by the controls
// that anchor a popup under their trigger button so the math isn't copy-pasted.

export interface Anchor {
  left: number;
  top: number;
  minWidth: number;
}

/// Place a menu flush under `el` (its trigger), as fixed-position coordinates.
/// Width is at least `minWidth`, and the left edge is clamped to the viewport so
/// a trigger near the right edge doesn't push the menu off-screen.
export function anchorBelow(el: HTMLElement, minWidth = 0): Anchor {
  const r = el.getBoundingClientRect();
  const width = Math.max(r.width, minWidth);
  const left = Math.max(8, Math.min(r.left, window.innerWidth - width - 8));
  return { left, top: r.bottom + 4, minWidth: width };
}
