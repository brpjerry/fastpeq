<script lang="ts">
  // One row of the Hotkeys page: a drag handle, the modifier + key that form the
  // combo, the action, and the action's principal (preset / tone control / none),
  // plus a remove button. Every control is fixed-width so rows don't reflow as
  // values change. Reorder is pointer-driven by the parent via onDragStart.
  import SelectMenu from "./SelectMenu.svelte";
  import PresetPicker from "./PresetPicker.svelte";
  import { validKey, type Hotkey, type HotkeyAction, type HotkeyMod, type ToneControl } from "./hotkeys.svelte";

  let {
    hotkey,
    index,
    presets,
    categories,
    failed = false,
    dragging = false,
    onUpdate,
    onRemove,
    onDragStart,
  }: {
    hotkey: Hotkey;
    index: number;
    presets: string[];
    categories: Record<string, string>;
    failed?: boolean;
    dragging?: boolean;
    onUpdate: (patch: Partial<Hotkey>) => void;
    onRemove: () => void;
    onDragStart: (index: number, e: PointerEvent) => void;
  } = $props();

  const MODS = [
    { value: "ctrl-alt", label: "Ctrl + Alt" },
    { value: "ctrl-shift", label: "Ctrl + Shift" },
  ];
  const ACTIONS = [
    { value: "preset", label: "Switch preset" },
    { value: "bypass", label: "Bypass" },
    { value: "tone-up", label: "Tone up" },
    { value: "tone-down", label: "Tone down" },
    { value: "tone-reset", label: "Reset tone" },
  ];
  const TONES = [
    { value: "bass", label: "Bass" },
    { value: "mid", label: "Mids" },
    { value: "treble", label: "Treble" },
  ];

  // Switching action seeds a sensible default principal for the new action.
  function changeAction(action: string) {
    const patch: Partial<Hotkey> = { action: action as HotkeyAction };
    if (action === "preset" && !hotkey.preset) patch.preset = presets[0];
    if ((action === "tone-up" || action === "tone-down") && !hotkey.tone) patch.tone = "bass";
    onUpdate(patch);
  }
</script>

<li class="hk-row" class:failed class:dragging>
  <span
    class="drag"
    role="button"
    tabindex="-1"
    onpointerdown={(e) => onDragStart(index, e)}
    title="Drag to reorder"
    aria-label="Reorder"
  >
    <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor" aria-hidden="true">
      <circle cx="9" cy="6" r="1.4" /><circle cx="15" cy="6" r="1.4" />
      <circle cx="9" cy="12" r="1.4" /><circle cx="15" cy="12" r="1.4" />
      <circle cx="9" cy="18" r="1.4" /><circle cx="15" cy="18" r="1.4" />
    </svg>
  </span>

  <SelectMenu
    value={hotkey.mod}
    options={MODS}
    onChange={(v) => onUpdate({ mod: v as HotkeyMod })}
    width={112}
    minWidth={112}
  />
  <input
    class="key-input"
    class:invalid={hotkey.key !== "" && !validKey(hotkey.key)}
    maxlength="1"
    placeholder="?"
    value={hotkey.key}
    oninput={(e) => {
      const v = e.currentTarget.value.toUpperCase();
      e.currentTarget.value = v;
      onUpdate({ key: v });
    }}
    aria-label="Hotkey key"
    title="A single letter or digit"
  />

  <span class="arrow" aria-hidden="true">→</span>

  <SelectMenu value={hotkey.action} options={ACTIONS} onChange={changeAction} width={132} minWidth={132} />

  <span class="principal">
    {#if hotkey.action === "preset"}
      <PresetPicker
        value={hotkey.preset ?? ""}
        {presets}
        {categories}
        onChange={(v) => onUpdate({ preset: v })}
        width={282}
      />
    {:else if hotkey.action === "tone-up" || hotkey.action === "tone-down"}
      <SelectMenu
        value={hotkey.tone ?? "bass"}
        options={TONES}
        onChange={(v) => onUpdate({ tone: v as ToneControl })}
        width={110}
        minWidth={110}
      />
    {:else}
      <span class="none">—</span>
    {/if}
  </span>

  <span class="hk-tail">
    {#if failed}
      <span class="warn" title="This combo couldn't be registered — it may already be in use by another app.">⚠</span>
    {/if}
    <button class="danger icon hk-remove" onclick={onRemove} title="Remove hotkey" aria-label="Remove hotkey">
      &#10005;
    </button>
  </span>
</li>

<style>
  .hk-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .hk-row.failed {
    border-color: var(--danger);
  }
  .hk-row.dragging {
    opacity: 0.6;
    background: var(--panel-2);
  }
  .drag {
    flex: none;
    display: inline-flex;
    cursor: grab;
    color: var(--muted);
    padding: 2px;
    touch-action: none;
  }
  .drag:active {
    cursor: grabbing;
  }
  .key-input {
    flex: none;
    width: 34px;
    text-align: center;
    text-transform: uppercase;
    font-variant-numeric: tabular-nums;
    padding: 3px 4px;
  }
  .key-input.invalid {
    border-color: var(--danger);
  }
  .arrow {
    color: var(--muted);
    flex: none;
  }
  .principal {
    flex: none;
    width: 290px;
    display: inline-flex;
    align-items: center;
  }
  .none {
    color: var(--muted);
    font-size: 12px;
  }
  .hk-tail {
    margin-left: auto;
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .warn {
    color: var(--danger);
    cursor: help;
  }
</style>
