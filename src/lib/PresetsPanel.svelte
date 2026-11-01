<script module lang="ts">
  // Scroll the active preset row into view. Module-scoped so App — which owns
  // reload/open and the settings/hotkeys subpages — can call the same one
  // definition instead of duplicating the query.
  export function scrollCurrentIntoView() {
    const el = document.querySelector(".presets .active") as HTMLElement | null;
    if (el && typeof el.scrollIntoView === "function") {
      el.scrollIntoView({ block: "center" });
    }
  }
</script>

<script lang="ts">
  import type { ApoStatus, VariantSet } from "./api";
  import CategoryIcon from "./CategoryIcon.svelte";
  import FloatingMenu from "./FloatingMenu.svelte";
  import { anchorBelow, type Anchor } from "./floating";
  import { getSpecialtyIcons, getBluetoothIcons } from "./prefs.svelte";
  import { CATEGORIES, CATEGORY_LABELS, CATEGORY_NONE } from "./constants";
  import { variantCreateLabel, variantLabel, variantNoun, variantPlaceholder } from "./variants";


  let {
    presets,
    categories,
    versions = {},
    active,
    selected,
    isBypassed,
    status,
    refreshing,
    busy,
    bandCount,
    onRefresh,
    onToggleBypass,
    onOpen,
    onPreview,
    onRemove,
    onSetCategory,
    onNewPreset,
    onCapture,
    onRename,
    variants = {},
    subPresets = true,
    onPickVariant,
    onNewVariant,
    onRemoveVariant,
    onRenameVariant,
  }: {
    presets: string[];
    categories: Record<string, string>;
    /** History revision counts per preset; a preset's current content is
     * version `count + 1` — shown as a small gray "vN" after the name. */
    versions?: Record<string, number>;
    active: string | null;
    selected: string | null;
    isBypassed: boolean;
    status: ApoStatus | null;
    refreshing: boolean;
    busy: boolean;
    bandCount: number;
    onRefresh: () => void;
    onToggleBypass: () => void;
    onOpen: (name: string) => void;
    /** Right-click on a preset name: open its read-only preview. */
    onPreview: (name: string) => void;
    onRemove: (name: string) => void;
    onSetCategory: (name: string, value: string | null) => void;
    onNewPreset: (name: string) => void;
    onCapture: (name: string) => void;
    onRename: (from: string, to: string) => void;
    /** Each preset's sub-presets (pad / tip variants) and the selected one;
     * presets without any are absent. */
    variants?: Record<string, VariantSet>;
    /** The global sub-presets switch (Settings). Off hides the per-row dropdown
     * and the create button; a preset still loads whichever variant is selected. */
    subPresets?: boolean;
    /** A variant was picked from a row's dropdown: select it and load it. */
    onPickVariant: (name: string, variant: string) => void;
    /** Create a variant of `parent` from its current settings. */
    onNewVariant: (parent: string, name: string) => void;
    onRemoveVariant: (name: string, variant: string) => void;
    onRenameVariant: (name: string, from: string, to: string) => void;
  } = $props();

  let query = $state("");
  let typeFilter = $state("");
  const filteredPresets = $derived(
    presets.filter((p) => {
      if (!p.toLowerCase().includes(query.trim().toLowerCase())) return false;
      if (typeFilter === "") return true;
      const cat = categories[p] ?? null;
      return typeFilter === CATEGORY_NONE ? cat === null : cat === typeFilter;
    }),
  );

  const categoryLabel = (c: string | undefined) =>
    c ? (CATEGORY_LABELS[c] ?? c) : "Uncategorized";

  const selectableCategories = $derived(
    CATEGORIES.filter(
      (c) =>
        c.group === "base" ||
        (c.group === "specialty" && getSpecialtyIcons()) ||
        (c.group === "bluetooth" && getBluetoothIcons()),
    ),
  );

  const usedCategories = $derived(
    CATEGORIES.filter((c) => presets.some((p) => categories[p] === c.value)),
  );
  const hasUncategorized = $derived(presets.some((p) => !categories[p]));
  
  $effect(() => {
    const stillValid =
      typeFilter === "" ||
      (typeFilter === CATEGORY_NONE ? hasUncategorized : usedCategories.some((c) => c.value === typeFilter));
    if (!stillValid) typeFilter = "";
  });

  const cycleCategory = (name: string) => {
    const cycle: (string | null)[] = [null, ...selectableCategories.map((c) => c.value)];
    const current = categories[name] ?? null;
    onSetCategory(name, cycle[(cycle.indexOf(current) + 1) % cycle.length]);
  };

  let catMenu = $state<{ name: string; x: number; y: number } | null>(null);
  function openCatMenu(e: MouseEvent, name: string) {
    e.preventDefault();
    const w = 200;
    const h = (selectableCategories.length + 1) * 30 + 10;
    catMenu = {
      name,
      x: Math.max(8, Math.min(e.clientX, window.innerWidth - w - 8)),
      y: Math.max(8, Math.min(e.clientY, window.innerHeight - h - 8)),
    };
  }
  function pickCategory(name: string, value: string | null) {
    catMenu = null;
    onSetCategory(name, value);
  }

  let typeMenu = $state<Anchor | null>(null);
  let typeTriggerEl = $state<HTMLButtonElement | null>(null);
  const typeFilterLabel = $derived(
    typeFilter === ""
      ? "All types"
      : typeFilter === CATEGORY_NONE
        ? "Uncategorized"
        : (CATEGORY_LABELS[typeFilter] ?? typeFilter),
  );
  function toggleTypeMenu() {
    if (typeMenu) {
      typeMenu = null;
      return;
    }
    if (!typeTriggerEl) return;
    typeMenu = anchorBelow(typeTriggerEl);
  }
  function pickType(v: string) {
    typeFilter = v;
    typeMenu = null;
  }

  // The create form at the bottom: a new preset, or a variant of the selected
  // preset (same form, one Create button — a variant always copies its parent).
  let creating = $state<false | "preset" | "variant">(false);
  let newName = $state("");

  function startCreate() {
    newName = "";
    creating = "preset";
  }
  function startCreateVariant() {
    if (!selected) return;
    newName = "";
    creating = "variant";
  }
  function cancelCreate() {
    creating = false;
    newName = "";
  }
  function submitCreate() {
    onNewPreset(newName);
    cancelCreate();
    query = "";
    typeFilter = "";
  }
  function submitCapture() {
    onCapture(newName);
    cancelCreate();
    query = "";
    typeFilter = "";
  }
  function submitVariant() {
    if (selected) onNewVariant(selected, newName);
    cancelCreate();
  }
  // Enter in the name box submits whichever flavour of create is open.
  function submitDefault() {
    if (creating === "variant") submitVariant();
    else submitCreate();
  }

  let renaming = $state<string | null>(null);
  let renameValue = $state("");

  function focusInput(node: HTMLInputElement) {
    node.focus();
    node.select();
  }
  function startRename(name: string) {
    renaming = name;
    renameValue = name;
  }
  function cancelRename() {
    renaming = null;
  }
  function commitRename() {
    const from = renaming;
    if (from === null) return;
    renaming = null;
    onRename(from, renameValue);
  }

  // A preset's variant dropdown (the row's "<variant> pads ▾" trigger). The
  // menu floats outside the list like the category picker, so a row never
  // gains nested `.active` elements (scrollCurrentIntoView keys off that class).
  const hasVariants = (name: string) =>
    subPresets && (variants[name]?.variants.length ?? 0) > 0;
  let variantMenu = $state<{ name: string; anchor: Anchor; trigger: HTMLElement } | null>(null);
  let renamingVariant = $state<string | null>(null);
  let renameVariantValue = $state("");

  function toggleVariantMenu(trigger: HTMLElement, name: string) {
    renamingVariant = null;
    if (variantMenu?.name === name) {
      variantMenu = null;
      return;
    }
    variantMenu = { name, anchor: anchorBelow(trigger, 200), trigger };
  }
  function pickVariant(name: string, variant: string) {
    variantMenu = null;
    onPickVariant(name, variant);
  }
  function removeVariant(name: string, variant: string) {
    variantMenu = null;
    onRemoveVariant(name, variant);
  }
  function startRenameVariant(variant: string) {
    renamingVariant = variant;
    renameVariantValue = variant;
  }
  function commitRenameVariant(name: string) {
    const from = renamingVariant;
    if (from === null) return;
    renamingVariant = null;
    onRenameVariant(name, from, renameVariantValue);
  }
  // A reload can take the preset (or its last variant) out from under an open menu.
  $effect(() => {
    if (variantMenu && !hasVariants(variantMenu.name)) variantMenu = null;
  });
  // The variant form creates for the selected preset; should that selection go
  // away while it is open (the preset was deleted), there is nothing to create for.
  $effect(() => {
    if (creating === "variant" && !selected) cancelCreate();
  });

  let prevQuery = "";
  let prevFilter = "";
  $effect(() => {
    const q = query.trim();
    const tf = typeFilter;
    const searchCleared = !!prevQuery && !q;
    const filterChanged = tf !== prevFilter;
    prevQuery = q;
    prevFilter = tf;
    if (searchCleared || filterChanged) scrollCurrentIntoView();
  });
</script>

<section class="panel">
  <div class="panel-head">
    <h2>Presets</h2>
    <div class="head-actions">
      <button
        class="refresh"
        onclick={onRefresh}
        disabled={refreshing || !status}
        title="Refresh preset list"
        aria-label="Refresh preset list"
      >
        <svg class:spin={refreshing} viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M21 12a9 9 0 1 1-2.64-6.36" />
          <path d="M21 3v5h-5" />
        </svg>
      </button>
      <button
        class="ghost"
        class:on={isBypassed}
        onclick={onToggleBypass}
        disabled={busy || !status}
        title="Drop the EQ filters (keeps the preamp) — click again to restore"
      >
        {isBypassed ? "Bypassed" : "Bypass"}
      </button>
    </div>
  </div>

  <div class="filters">
    <input
      class="search"
      type="search"
      placeholder="Search presets…"
      bind:value={query}
      onkeydown={async (e) => {
        if (e.key === "Enter" && filteredPresets.length) {
          onOpen(filteredPresets[0]);
          query = "";
        }
      }}
      disabled={!status}
    />
    <div class="type-dd">
      <button
        bind:this={typeTriggerEl}
        class="type-trigger"
        class:open={!!typeMenu}
        onclick={toggleTypeMenu}
        disabled={!status}
        aria-haspopup="listbox"
        aria-expanded={!!typeMenu}
        aria-label="Filter by device type"
      >
        <span class="type-trigger-icon">
          {#if typeFilter === ""}
            <svg class="type-all-svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 5h18M6 12h12M10 19h4" /></svg>
          {:else if typeFilter === CATEGORY_NONE}
            <CategoryIcon category={undefined} />
          {:else}
            <CategoryIcon category={typeFilter} />
          {/if}
        </span>
        <span class="type-trigger-label">{typeFilterLabel}</span>
        <svg class="type-caret" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M6 9l6 6 6-6" />
        </svg>
      </button>
    </div>
  </div>

  <ul class="presets">
    {#each filteredPresets as name (name)}
      <li class:active={name === active} class:selected={name === selected}>
        {#if renaming === name}
          <input
            class="rename-input"
            bind:value={renameValue}
            use:focusInput
            onblur={commitRename}
            onkeydown={(e) => {
              if (e.key === "Enter") commitRename();
              else if (e.key === "Escape") cancelRename();
            }}
          />
        {:else}
          <button
            class="cat"
            class:empty={!categories[name]}
            onclick={() => cycleCategory(name)}
            oncontextmenu={(e) => openCatMenu(e, name)}
            disabled={busy}
            title={`${categoryLabel(categories[name])} — click to cycle, right-click to choose`}
          >
            <CategoryIcon category={categories[name]} />
          </button>
          <button
            class="name"
            onclick={() => onOpen(name)}
            ondblclick={() => startRename(name)}
            oncontextmenu={(e) => {
              e.preventDefault();
              onPreview(name);
            }}
            title="Click to load (reverts unsaved live changes) · double-click to rename · right-click to preview"
          >
            {name}{#if versions[name]}<span class="ver">v{versions[name] + 1}</span>{/if}
          </button>
          {#if hasVariants(name)}
            {@const noun = variantNoun(categories[name])}
            {@const current = variants[name].selected}
            <button
              class="variant-trigger"
              class:open={variantMenu?.name === name}
              onclick={(e) => toggleVariantMenu(e.currentTarget, name)}
              disabled={busy}
              aria-haspopup="listbox"
              aria-expanded={variantMenu?.name === name}
              title={`Choose ${noun ?? "sub-preset"} · the preset loads the one selected here`}
            >
              <span class="variant-current">{current ? variantLabel(current, noun) : "Choose…"}</span>
              <svg class="type-caret" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M6 9l6 6 6-6" />
              </svg>
            </button>
          {/if}
          <div class="row-actions">
            <button class="icon" onclick={() => startRename(name)} disabled={busy} title="Rename">
              &#9998;
            </button>
            <button class="danger icon" onclick={() => onRemove(name)} disabled={busy} title="Delete">
              &#10005;
            </button>
          </div>
        {/if}
      </li>
    {:else}
      <li class="empty">
        {query.trim() || typeFilter
          ? "No presets match your filters."
          : "No presets yet — create or save one below."}
      </li>
    {/each}
  </ul>

  {#if creating}
    {@const parentCategory = categories[selected ?? ""]}
    <div class="create" class:variant={creating === "variant"}>
      <input
        placeholder={creating === "variant" ? variantPlaceholder(parentCategory) : "New preset name"}
        bind:value={newName}
        use:focusInput
        onkeydown={(e) => {
          if (e.key === "Enter") submitDefault();
          else if (e.key === "Escape") cancelCreate();
        }}
        disabled={busy || !status}
      />
      <div class="create-actions">
        {#if creating === "variant"}
          <button
            class="primary"
            onclick={submitVariant}
            disabled={busy || !status || !selected}
            title={`Copy “${selected}”, unsaved edits included, into a new ${variantNoun(parentCategory) ?? "sub-preset"} variant and switch to it`}
          >
            Create
          </button>
        {:else}
          <button
            class="primary"
            onclick={submitCreate}
            disabled={busy || !status}
            title="Start from {bandCount} empty bands (set the count in Settings)"
          >
            From scratch
          </button>
          <button
            class="capture-btn"
            onclick={submitCapture}
            disabled={busy || !status}
            title="Save the current live Equalizer APO config as this preset"
          >
            Save current
          </button>
        {/if}
        <button class="ghost create-cancel" onclick={cancelCreate} title="Cancel">Cancel</button>
      </div>
    </div>
  {:else}
    <div class="new-row">
      <button
        class="primary new-btn"
        onclick={startCreate}
        disabled={busy || !status}
      >
        + New preset
      </button>
      {#if subPresets}
        <button
          class="variant-btn"
          onclick={startCreateVariant}
          disabled={busy || !status || !selected}
          title={selected
            ? `Add a ${variantNoun(categories[selected]) ?? "sub-preset"} variant of “${selected}”, starting from what the editor shows`
            : "Select a preset first"}
        >
          {variantCreateLabel(categories[selected ?? ""])}
        </button>
      {/if}
    </div>
  {/if}
</section>

{#if catMenu}
  {@const menu = catMenu}
  <FloatingMenu
    class="cat-menu"
    open={true}
    anchor={{ left: menu.x, top: menu.y, minWidth: 184 }}
    onDismiss={() => (catMenu = null)}
    zIndex={81}
    maxHeight="70vh"
  >
    <button class="menu-item cat-menu-item" class:sel={!categories[menu.name]} onclick={() => pickCategory(menu.name, null)}>
      <span class="cat-menu-icon"><CategoryIcon category={undefined} /></span>
      Uncategorized
    </button>
    {#each selectableCategories as c}
      <button
        class="menu-item cat-menu-item"
        class:sel={categories[menu.name] === c.value}
        onclick={() => pickCategory(menu.name, c.value)}
      >
        <span class="cat-menu-icon"><CategoryIcon category={c.value} /></span>
        {c.label}
      </button>
    {/each}
  </FloatingMenu>
{/if}

<FloatingMenu
  class="cat-menu type-menu"
  role="listbox"
  open={!!typeMenu}
  anchor={typeMenu}
  onDismiss={() => (typeMenu = null)}
  ignore={typeTriggerEl}
  zIndex={81}
  maxHeight="70vh"
>
  <button class="menu-item cat-menu-item" class:sel={typeFilter === ""} onclick={() => pickType("")}>
    <span class="cat-menu-icon"><svg class="type-all-svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 5h18M6 12h12M10 19h4" /></svg></span>
    All types
  </button>
  {#each usedCategories as c}
    <button class="menu-item cat-menu-item" class:sel={typeFilter === c.value} onclick={() => pickType(c.value)}>
      <span class="cat-menu-icon"><CategoryIcon category={c.value} /></span>
      {c.label}
    </button>
  {/each}
  {#if hasUncategorized}
    <button class="menu-item cat-menu-item" class:sel={typeFilter === CATEGORY_NONE} onclick={() => pickType(CATEGORY_NONE)}>
      <span class="cat-menu-icon"><CategoryIcon category={undefined} /></span>
      Uncategorized
    </button>
  {/if}
</FloatingMenu>

{#if variantMenu}
  {@const menu = variantMenu}
  {@const vs = variants[menu.name]}
  {@const noun = variantNoun(categories[menu.name])}
  <FloatingMenu
    class="cat-menu variant-menu"
    role="listbox"
    open={!!vs}
    anchor={menu.anchor}
    onDismiss={() => (variantMenu = null)}
    ignore={menu.trigger}
    zIndex={81}
    maxHeight="70vh"
  >
    {#each vs?.variants ?? [] as v (v)}
      {@const sel = v === vs?.selected}
      <div class="variant-row">
        {#if renamingVariant === v}
          <input
            class="rename-input variant-rename"
            bind:value={renameVariantValue}
            use:focusInput
            onblur={() => commitRenameVariant(menu.name)}
            onkeydown={(e) => {
              if (e.key === "Enter") commitRenameVariant(menu.name);
              else if (e.key === "Escape") renamingVariant = null;
            }}
          />
        {:else}
          <button
            class="menu-item variant-item"
            class:sel
            role="option"
            aria-selected={sel}
            onclick={() => pickVariant(menu.name, v)}
            title="Click to load"
          >
            {variantLabel(v, noun)}
          </button>
          <button class="icon variant-action" onclick={() => startRenameVariant(v)} disabled={busy} title="Rename">
            &#9998;
          </button>
          <button class="danger icon variant-action" onclick={() => removeVariant(menu.name, v)} disabled={busy} title="Delete">
            &#10005;
          </button>
        {/if}
      </div>
    {/each}
  </FloatingMenu>
{/if}

<style>
  .ghost.on {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .ghost.on:hover:not(:disabled) {
    background: var(--accent-2);
    border-color: var(--accent-2);
  }
  .head-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .refresh {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 6px;
    border-radius: 7px;
    color: var(--muted);
  }
  .refresh:hover:not(:disabled) {
    color: var(--text);
  }
  .refresh svg {
    display: block;
  }
  .refresh svg.spin {
    animation: spin 0.6s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .filters {
    display: flex;
    gap: 8px;
    margin-bottom: 8px;
  }
  .search {
    flex: 1;
    min-width: 0;
  }
  .type-dd {
    flex: none;
    position: relative;
  }
  .type-trigger {
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 150px;
    padding: 6px 9px;
  }
  .type-trigger-icon {
    flex: none;
    display: inline-flex;
  }
  .type-trigger-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }
  .type-caret {
    flex: none;
    width: 14px;
    height: 14px;
    opacity: 0.65;
  }
  .type-trigger.open .type-caret {
    transform: rotate(180deg);
  }
  .type-all-svg {
    width: 16px;
    height: 16px;
    display: block;
  }

  .presets {
    list-style: none;
    margin: 0;
    padding: 0;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .presets li {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 3px 4px;
    border-radius: 8px;
    border: 1px solid transparent;
  }
  .presets li.selected {
    border-color: var(--border);
    background: var(--panel-2);
  }
  .presets li.active .name,
  .presets li.selected .name {
    color: var(--accent);
    font-weight: 600;
  }
  .cat {
    flex: none;
    width: 26px;
    height: 26px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--border);
    background: var(--panel-2);
    padding: 0;
    line-height: 0;
    border-radius: 6px;
    color: var(--text);
    overflow: hidden;
  }
  .cat:hover:not(:disabled) {
    background: #2b3038;
    border-color: #3a4150;
  }
  .cat.empty {
    color: var(--muted);
  }

  .cat-menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text);
  }
  .cat-menu-icon {
    flex: none;
    display: inline-flex;
  }

  .name {
    flex: 1;
    text-align: left;
    background: transparent;
    border: none;
    padding: 5px 8px;
  }
  /* Version badge: stays small and gray even on the accent-colored
     active/selected row. */
  .name .ver {
    margin-left: 6px;
    font-size: 10px;
    font-weight: 400;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .name:hover {
    background: var(--panel-2);
  }
  /* The row's variant dropdown trigger ("<variant> pads ▾"). A sibling of the
     name button, so the name's own text stays clean for scanning and tests. */
  .variant-trigger {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    max-width: 45%;
    padding: 3px 6px;
    border: 1px solid var(--border);
    background: var(--panel-2);
    border-radius: 6px;
    font-size: 11px;
    color: var(--muted);
  }
  .variant-trigger:hover:not(:disabled),
  .variant-trigger.open {
    background: #2b3038;
    border-color: #3a4150;
    color: var(--text);
  }
  .variant-current {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .variant-trigger .type-caret {
    width: 12px;
    height: 12px;
  }
  .variant-trigger.open .type-caret {
    transform: rotate(180deg);
  }
  .variant-row {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .variant-item {
    flex: 1;
    min-width: 0;
    color: var(--text);
  }
  .variant-action {
    flex: none;
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: none;
    background: transparent;
    border-radius: 5px;
    font-size: 12px;
    color: var(--muted);
    opacity: 0;
  }
  .variant-row:hover .variant-action,
  .variant-action:focus-visible {
    opacity: 1;
  }
  .variant-action:hover:not(:disabled) {
    background: var(--panel-2);
    color: var(--text);
  }
  .danger.variant-action:hover:not(:disabled) {
    color: var(--danger);
  }
  .variant-rename {
    flex: 1;
    margin: 2px;
    font-size: 13px;
  }
  .row-actions {
    display: flex;
    gap: 4px;
  }
  .row-actions .icon {
    width: 26px;
    height: 26px;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: 1px solid var(--border);
    background: var(--panel-2);
    border-radius: 6px;
    line-height: 0;
    font-size: 13px;
    color: var(--muted);
  }
  .row-actions .icon:hover:not(:disabled) {
    background: #2b3038;
    border-color: #3a4150;
    color: var(--text);
  }
  .row-actions .danger.icon:hover:not(:disabled) {
    border-color: var(--danger);
    color: var(--danger);
  }
  .rename-input {
    flex: 1;
    margin: 1px 0;
  }
  .empty {
    color: var(--muted);
    padding: 10px 8px;
  }

  .new-row {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }
  .new-btn {
    flex: 1;
    min-width: 0;
  }
  /* "Pad roll" / "Tip roll" / "+ sub-preset": a variant of the selected preset. */
  .variant-btn {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
  }
  .create {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 12px;
  }
  .create input {
    width: 100%;
  }
  .create-actions {
    display: flex;
    gap: 8px;
  }
  .create-actions .primary,
  .create-actions .capture-btn {
    flex: 1;
  }
  .create-cancel {
    flex: none;
  }

  @media (max-width: 820px) {
    .presets {
      flex: none;
      overflow: visible;
    }
  }
</style>
