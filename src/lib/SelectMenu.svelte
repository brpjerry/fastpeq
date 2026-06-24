<script lang="ts">
  // A custom dropdown for a plain list of options, styled like the rest of the
  // app's menus (TypeSelect, the device-type filter) instead of the native OS
  // <select>. Same fixed-position + dismissable popup pattern as TypeSelect.
  import { dismissable } from "./dismiss";

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
  let pos = $state<{ left: number; top: number; width: number } | null>(null);
  let btn = $state<HTMLButtonElement | null>(null);

  const current = $derived(options.find((o) => o.value === value));

  function toggle() {
    if (open || !btn) {
      open = false;
      return;
    }
    const r = btn.getBoundingClientRect();
    pos = { left: r.left, top: r.bottom + 4, width: Math.max(r.width, minWidth) };
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

{#if open && pos}
  <div
    class="sm-menu"
    style="left:{pos.left}px; top:{pos.top}px; min-width:{pos.width}px"
    use:dismissable={{ onDismiss: () => (open = false), ignore: btn }}
  >
    {#each options as o (o.value)}
      <button class="sm-item" class:sel={o.value === value} type="button" onclick={() => pick(o.value)}>
        {o.label}
      </button>
    {/each}
  </div>
{/if}

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
  .sm-menu {
    position: fixed;
    z-index: 51;
    max-height: 280px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    padding: 4px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.45);
  }
  .sm-item {
    text-align: left;
    white-space: nowrap;
    border: none;
    background: transparent;
    padding: 6px 8px;
    border-radius: 5px;
    font-size: 13px;
    color: var(--muted);
  }
  .sm-item:hover {
    background: var(--panel-2);
  }
  .sm-item.sel {
    color: var(--accent);
  }
</style>
