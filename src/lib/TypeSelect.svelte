<script lang="ts">
  // Compact filter-type picker: shows just the token (e.g. "PK") when closed,
  // and the full "PK — Peaking" in the dropdown. A native <select> can't do
  // different closed/open text, hence this small custom control.
  import { FILTER_TYPES, BASIC_FILTER_KINDS } from "./eq";
  import { getFilterSet } from "./prefs.svelte";
  import { dismissable } from "./dismiss";
  import type { FilterKind } from "./types";

  let { value, onChange }: { value: FilterKind; onChange: (v: FilterKind) => void } = $props();

  // The dropdown list honours the basic/full setting, but always keeps the
  // current value selectable (so an existing non-basic filter isn't stranded).
  const visibleTypes = $derived(
    getFilterSet() === "full"
      ? FILTER_TYPES
      : FILTER_TYPES.filter((t) => BASIC_FILTER_KINDS.includes(t.value) || t.value === value),
  );

  let open = $state(false);
  let pos = $state<{ left: number; top: number; width: number } | null>(null);
  let btn = $state<HTMLButtonElement | null>(null);

  const current = $derived(FILTER_TYPES.find((t) => t.value === value));

  function toggle() {
    if (open || !btn) {
      open = false;
      return;
    }
    const r = btn.getBoundingClientRect();
    pos = { left: r.left, top: r.bottom + 4, width: Math.max(r.width, 170) };
    open = true;
  }

  function pick(v: FilterKind) {
    open = false;
    if (v !== value) onChange(v);
  }
</script>

<button
  bind:this={btn}
  class="ts-btn"
  type="button"
  onclick={toggle}
  title={current ? `${current.token} — ${current.label}` : ""}
>
  <span class="tok">{current?.token ?? value}</span>
  <svg class="chev" viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M6 9l6 6 6-6" />
  </svg>
</button>

{#if open && pos}
  <div
    class="ts-menu"
    style="left:{pos.left}px; top:{pos.top}px; min-width:{pos.width}px"
    use:dismissable={{ onDismiss: () => (open = false), ignore: btn }}
  >
    {#each visibleTypes as t}
      <button class="ts-item" class:sel={t.value === value} type="button" onclick={() => pick(t.value)}>
        <span class="tok">{t.token}</span> — {t.label}
      </button>
    {/each}
  </div>
{/if}

<style>
  .ts-btn {
    flex: none;
    width: 50px;
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 4px;
    padding: 2px 5px;
    font-size: 12px;
  }
  .ts-btn .tok {
    font-variant-numeric: tabular-nums;
  }
  .chev {
    flex: none;
    opacity: 0.55;
  }
  .ts-menu {
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
  .ts-item {
    text-align: left;
    white-space: nowrap;
    border: none;
    background: transparent;
    padding: 6px 8px;
    border-radius: 5px;
    font-size: 13px;
    color: var(--muted);
  }
  .ts-item:hover {
    background: var(--panel-2);
  }
  .ts-item.sel {
    color: var(--accent);
  }
  .ts-item .tok {
    font-weight: 600;
    color: var(--text);
  }
  .ts-item.sel .tok {
    color: var(--accent);
  }
</style>
