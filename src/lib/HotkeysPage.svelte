<script lang="ts">
  // The Hotkeys page (peer of Settings): the editable list of global hotkeys plus
  // a "New" button. Reads/writes the hotkeys store directly; App passes the preset
  // names (for the "switch preset" principal) and the ids that failed to register.
  import { getHotkeys, addHotkey, updateHotkey, removeHotkey, moveHotkey } from "./hotkeys.svelte";
  import HotkeyRow from "./HotkeyRow.svelte";

  let { presets, failedIds = [] }: { presets: string[]; failedIds?: string[] } = $props();

  // HTML5 drag reorder: the dragged row's index is held here until a drop.
  let dragFrom = $state<number | null>(null);
  function onDragStart(i: number) {
    dragFrom = i;
  }
  function onDrop(i: number) {
    if (dragFrom !== null) moveHotkey(dragFrom, i);
    dragFrom = null;
  }
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
      <ul class="hk-list">
        {#each getHotkeys() as h, i (h.id)}
          <HotkeyRow
            hotkey={h}
            index={i}
            {presets}
            failed={failedIds.includes(h.id)}
            onUpdate={(patch) => updateHotkey(h.id, patch)}
            onRemove={() => removeHotkey(h.id)}
            {onDragStart}
            {onDrop}
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
