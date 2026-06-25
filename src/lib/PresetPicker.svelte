<script lang="ts">
  // A fixed-width preset dropdown with a search box and a device-type filter in
  // the popup — the same affordances as the main preset list, for picking the
  // preset a hotkey switches to. Built on the shared FloatingMenu shell.
  import FloatingMenu from "./FloatingMenu.svelte";
  import CategoryIcon from "./CategoryIcon.svelte";
  import { anchorBelow, type Anchor } from "./floating";

  let {
    value,
    presets,
    categories,
    onChange,
    width = 190,
  }: {
    value: string;
    presets: string[];
    categories: Record<string, string>;
    onChange: (name: string) => void;
    width?: number;
  } = $props();

  let open = $state(false);
  let anchor = $state<Anchor | null>(null);
  let btn = $state<HTMLButtonElement | null>(null);
  let query = $state("");
  let typeFilter = $state(""); // "" = all, "__none" = uncategorized, else a category

  // Only the device types some preset actually uses, in first-seen order.
  const usedCats = $derived([...new Set(presets.map((p) => categories[p]).filter(Boolean))]);
  const hasUncat = $derived(presets.some((p) => !categories[p]));

  const filtered = $derived(
    presets.filter((p) => {
      const q = query.trim().toLowerCase();
      if (q && !p.toLowerCase().includes(q)) return false;
      const cat = categories[p];
      if (typeFilter === "") return true;
      return typeFilter === "__none" ? !cat : cat === typeFilter;
    }),
  );

  function toggle() {
    if (open || !btn) {
      open = false;
      return;
    }
    query = "";
    typeFilter = "";
    anchor = anchorBelow(btn, 280);
    open = true;
  }

  function pick(name: string) {
    open = false;
    if (name !== value) onChange(name);
  }

  function focusInput(node: HTMLInputElement) {
    node.focus();
  }
</script>

<button
  bind:this={btn}
  class="pp-btn"
  type="button"
  onclick={toggle}
  style="width:{width}px"
  title={value || "Choose a preset"}
>
  {#if value}
    <span class="pp-ico"><CategoryIcon category={categories[value]} /></span>
    <span class="pp-label">{value}</span>
  {:else}
    <span class="pp-placeholder">Choose preset…</span>
  {/if}
  <svg class="chev" viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M6 9l6 6 6-6" />
  </svg>
</button>

<FloatingMenu class="pp-menu" {open} {anchor} onDismiss={() => (open = false)} ignore={btn} maxHeight="420px">
  <input
    class="pp-search"
    type="search"
    placeholder="Search presets…"
    bind:value={query}
    use:focusInput
    onkeydown={(e) => {
      if (e.key === "Enter" && filtered.length) pick(filtered[0]);
    }}
  />
  {#if usedCats.length || hasUncat}
    <div class="pp-filters">
      <button class="pp-chip" class:sel={typeFilter === ""} onclick={() => (typeFilter = "")} title="All types">
        All
      </button>
      {#each usedCats as c}
        <button class="pp-chip" class:sel={typeFilter === c} onclick={() => (typeFilter = c)} title={c}>
          <CategoryIcon category={c} />
        </button>
      {/each}
      {#if hasUncat}
        <button
          class="pp-chip"
          class:sel={typeFilter === "__none"}
          onclick={() => (typeFilter = "__none")}
          title="Uncategorized"
        >
          <CategoryIcon category={undefined} />
        </button>
      {/if}
    </div>
  {/if}
  <div class="pp-list">
    {#each filtered as p (p)}
      <button class="menu-item pp-item" class:sel={p === value} type="button" onclick={() => pick(p)}>
        <span class="pp-ico"><CategoryIcon category={categories[p]} /></span>
        <span class="pp-name">{p}</span>
      </button>
    {:else}
      <p class="pp-empty">No matching presets</p>
    {/each}
  </div>
</FloatingMenu>

<style>
  .pp-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 2px 6px;
    font-size: 12px;
  }
  .pp-ico {
    flex: none;
    display: inline-flex;
    color: var(--muted);
  }
  .pp-ico :global(svg) {
    width: 15px;
    height: 15px;
  }
  .pp-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }
  .pp-placeholder {
    flex: 1;
    text-align: left;
    color: var(--muted);
  }
  .chev {
    flex: none;
    opacity: 0.55;
  }
  /* Search box + filter row are fixed at the top; only the list scrolls. */
  .pp-search {
    width: 100%;
    margin-bottom: 4px;
    padding: 4px 6px;
    font-size: 12px;
  }
  .pp-filters {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-bottom: 4px;
  }
  .pp-chip {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 26px;
    height: 24px;
    padding: 0 6px;
    font-size: 11px;
    color: var(--muted);
  }
  .pp-chip :global(svg) {
    width: 15px;
    height: 15px;
  }
  .pp-chip.sel {
    border-color: var(--accent);
    color: var(--accent);
  }
  .pp-list {
    max-height: 300px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }
  .pp-item {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .pp-item.sel {
    color: var(--accent);
  }
  .pp-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pp-empty {
    margin: 0;
    padding: 8px;
    color: var(--muted);
    font-size: 12px;
  }
</style>
