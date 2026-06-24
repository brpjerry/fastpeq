<script lang="ts">
  // A custom dropdown for a plain list of options, styled like the rest of the
  // app's menus instead of the native OS <select>. The popup chrome and dismiss
  // wiring live in FloatingMenu; this supplies the trigger and the option list.
  import FloatingMenu from "./FloatingMenu.svelte";
  import { anchorBelow, type Anchor } from "./floating";

  let {
    value,
    options,
    onChange,
    title = "",
    minWidth = 150,
  }: {
    value: string;
    options: { value: string; label: string }[];
    onChange: (v: string) => void;
    title?: string;
    minWidth?: number;
  } = $props();

  let open = $state(false);
  let anchor = $state<Anchor | null>(null);
  let btn = $state<HTMLButtonElement | null>(null);

  const current = $derived(options.find((o) => o.value === value));

  function toggle() {
    if (open || !btn) {
      open = false;
      return;
    }
    anchor = anchorBelow(btn, minWidth);
    open = true;
  }

  function pick(v: string) {
    open = false;
    if (v !== value) onChange(v);
  }
</script>

<button bind:this={btn} class="sm-btn" type="button" onclick={toggle} {title}>
  <span class="sm-label">{current?.label ?? value}</span>
  <svg class="chev" viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M6 9l6 6 6-6" />
  </svg>
</button>

<FloatingMenu class="sm-menu" {open} {anchor} onDismiss={() => (open = false)} ignore={btn}>
  {#each options as o (o.value)}
    <button class="menu-item sm-item" class:sel={o.value === value} type="button" onclick={() => pick(o.value)}>
      {o.label}
    </button>
  {/each}
</FloatingMenu>

<style>
  .sm-btn {
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    padding: 2px 6px;
    font-size: 12px;
    max-width: 220px;
  }
  .sm-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chev {
    flex: none;
    opacity: 0.55;
  }
  .sm-item {
    color: var(--muted);
  }
</style>
