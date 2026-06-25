<script lang="ts">
  // The Hotkeys page (peer of Settings): the editable list of global hotkeys plus
  // a "New" button. Reads/writes the hotkeys store directly; App passes the preset
  // names (for the "switch preset" principal) and the ids that failed to register.
  import { onDestroy } from "svelte";
  import { getHotkeys, addHotkey, updateHotkey, removeHotkey, moveHotkey } from "./hotkeys.svelte";
  import HotkeyRow from "./HotkeyRow.svelte";

  let {
    presets,
    categories,
    failedIds = [],
  }: { presets: string[]; categories: Record<string, string>; failedIds?: string[] } = $props();

  // Pointer-driven reorder: drag the handle and rows reflow live as the pointer
  // crosses each row's midpoint. (Native HTML5 DnD is unreliable inside WebView2.)
  let listEl = $state<HTMLUListElement | null>(null);
  let dragId = $state<string | null>(null);

  function onDragStart(index: number, e: PointerEvent) {
    e.preventDefault();
    dragId = getHotkeys()[index]?.id ?? null;
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }
  function onMove(e: PointerEvent) {
    if (dragId === null || !listEl) return;
    const rows = [...listEl.querySelectorAll<HTMLElement>(".hk-row")];
    const from = getHotkeys().findIndex((h) => h.id === dragId);
    if (from < 0) return;
    let to = rows.length - 1;
    for (let i = 0; i < rows.length; i++) {
      const r = rows[i].getBoundingClientRect();
      if (e.clientY < r.top + r.height / 2) {
        to = i;
        break;
      }
    }
    if (to !== from) moveHotkey(from, to);
  }
  function onUp() {
    dragId = null;
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
  }
  onDestroy(onUp);
</script>

<section class="panel hotkeys-page">
  <div class="panel-head">
    <h2>Hotkeys</h2>
  </div>
  <div class="hotkeys-body">
    <p class="hint">
      Global shortcuts that work anywhere in Windows, even when fastpeq is minimized.
      Each is a modifier plus a single key — Ctrl+Alt and Ctrl+Shift are the safest
      choices. A combo already used by another app shows a ⚠ and won't fire.
    </p>

    {#if getHotkeys().length}
      <ul class="hk-list" bind:this={listEl}>
        {#each getHotkeys() as h, i (h.id)}
          <HotkeyRow
            hotkey={h}
            index={i}
            {presets}
            {categories}
            failed={failedIds.includes(h.id)}
            dragging={h.id === dragId}
            onUpdate={(patch) => updateHotkey(h.id, patch)}
            onRemove={() => removeHotkey(h.id)}
            {onDragStart}
          />
        {/each}
      </ul>
    {:else}
      <p class="empty">No hotkeys yet — add one to get started.</p>
    {/if}

    <div class="hk-actions">
      <button class="primary" onclick={addHotkey}>+ New hotkey</button>
    </div>
  </div>
</section>

<style>
  .hotkeys-page {
    flex: 1;
  }
  .hotkeys-body {
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding-top: 4px;
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 13px;
    max-width: 640px;
  }
  .hk-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .empty {
    color: var(--muted);
  }
  .hk-actions {
    display: flex;
  }
</style>
