<script lang="ts">
  // Shared shell for the app's custom dropdowns/menus: a fixed-position,
  // dismissable container that the consumer fills with item buttons. Owns the
  // popup chrome (panel box, shadow, scroll) and the common item styling, so
  // SelectMenu, TypeSelect, and the App category/device-type menus don't each
  // re-implement it. The trigger and item content stay with the consumer; this
  // just positions and frames them.
  //
  //   <FloatingMenu open={open} anchor={anchor} onDismiss={...} ignore={btn}>
  //     <button class="menu-item">…</button>
  //   </FloatingMenu>
  import { dismissable } from "./dismiss";
  import type { Snippet } from "svelte";
  import type { Anchor } from "./floating";

  let {
    open,
    anchor,
    onDismiss,
    ignore = null,
    zIndex = 51,
    maxHeight = "280px",
    role,
    class: extraClass = "",
    children,
  }: {
    open: boolean;
    anchor: Anchor | { left: number; top?: number; bottom?: number; minWidth?: number; maxHeight?: number } | null;
    onDismiss: () => void;
    ignore?: HTMLElement | null;
    zIndex?: number;
    maxHeight?: string;
    role?: string;
    class?: string;
    children: Snippet;
  } = $props();
</script>

{#if open && anchor}
  <div
    class="fmenu {extraClass}"
    {role}
    style="left:{anchor.left}px; {anchor.top !== undefined ? `top:${anchor.top}px;` : ''} {anchor.bottom !== undefined ? `bottom:${anchor.bottom}px;` : ''} min-width:{anchor.minWidth ?? 0}px; z-index:{zIndex}; max-height:{anchor.maxHeight !== undefined ? `min(${maxHeight}, ${anchor.maxHeight}px)` : maxHeight}"
    use:dismissable={{ onDismiss, ignore }}
  >
    {@render children()}
  </div>
{/if}

<style>
  .fmenu {
    position: fixed;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    padding: 4px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.45);
  }
  /* Items are written in the consumer's scope, so reach them with :global but
     only inside this shell. Consumers add class="menu-item" and may layer their
     own modifier class for color/icons. */
  .fmenu :global(.menu-item) {
    text-align: left;
    white-space: nowrap;
    border: none;
    background: transparent;
    padding: 6px 8px;
    border-radius: 5px;
    font-size: 13px;
  }
  .fmenu :global(.menu-item:hover) {
    background: var(--panel-2);
  }
  .fmenu :global(.menu-item.sel) {
    color: var(--accent);
  }
</style>
