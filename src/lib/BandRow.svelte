<script lang="ts">
  // One row of the band list: enable toggle, type picker, frequency, gain
  // (slider + number, only for gain types), and Q (only for Q types), plus a
  // remove button. Extracted from Editor; it mutates the passed `band` proxy in
  // place (so edits flow straight back) and emits semantic callbacks for the
  // throttled apply, the type change (which may seed a default Q), removal, and
  // graph-row hover linking.
  import TypeSelect from "./TypeSelect.svelte";
  import { kindHasGain, kindHasQ } from "./eq";
  import type { Channel, FilterKind } from "./types";

  type Band = {
    id: number;
    enabled: boolean;
    kind: FilterKind;
    freq: number;
    gain: number;
    q: number;
    channel: Channel;
  };

  let {
    band = $bindable(),
    hovered,
    onChange,
    onChangeKind,
    onRemove,
    onHover,
  }: {
    band: Band;
    hovered: boolean;
    onChange: () => void;
    onChangeKind: () => void;
    onRemove: () => void;
    onHover: (hovered: boolean) => void;
  } = $props();
</script>

<div
  class="band"
  class:off={!band.enabled}
  class:hover={hovered}
  onmouseenter={() => onHover(true)}
  onmouseleave={() => onHover(false)}
  role="presentation"
>
  <input type="checkbox" bind:checked={band.enabled} onchange={onChange} title="Enable / disable" />
  <TypeSelect
    value={band.kind}
    onChange={(v) => {
      band.kind = v;
      onChangeKind();
    }}
  />
  <span class="field freq">
    <input type="number" min="10" max="24000" step="1" bind:value={band.freq} onchange={onChange} />
    <small>Hz</small>
  </span>
  {#if kindHasGain(band.kind)}
    <span class="field gain">
      <input
        type="range"
        min="-30"
        max="30"
        step="0.1"
        tabindex="-1"
        bind:value={band.gain}
        oninput={onChange}
        oncontextmenu={() => {
          band.gain = 0;
          onChange();
        }}
        title="Right-click to reset to 0 dB"
      />
      <input type="number" min="-30" max="30" step="0.1" bind:value={band.gain} onchange={onChange} />
      <small>dB</small>
    </span>
  {/if}
  {#if kindHasQ(band.kind)}
    <span class="field q">
      <small>Q</small>
      <input type="number" min="0.1" max="36" step="0.1" bind:value={band.q} onchange={onChange} />
    </span>
  {/if}
  <button class="danger remove" onclick={onRemove} title="Remove band">
    &#10005;
  </button>
</div>

<style>
  .band {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 6px;
    border-bottom: 1px solid var(--border);
  }
  .band:last-child {
    border-bottom: none;
  }
  .band:hover {
    background: var(--panel-2);
  }
  /* Highlighted from the graph: hovering a handle marks its row. */
  .band.hover {
    background: var(--panel-2);
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .band.off {
    opacity: 0.45;
  }
  .band input {
    padding: 2px 5px;
    font-size: 12px;
  }
  .field {
    display: flex;
    align-items: center;
    gap: 3px;
    color: var(--muted);
    font-size: 11px;
  }
  /* Widths fit each field's longest value; no spinner arrows to leave room for. */
  .field input[type="number"] {
    width: 46px;
  }
  .field.freq input[type="number"] {
    width: 50px; /* up to 5 digits, e.g. 20000 */
  }
  .field.q input[type="number"] {
    width: 40px; /* e.g. 12.5 */
  }
  .field.gain {
    flex: 1;
    min-width: 84px;
  }
  .field.gain input[type="range"] {
    flex: 1;
    min-width: 50px;
    /* Keep the slider no taller than the other controls so hiding it (for
       no-gain filter types) doesn't change the row height. */
    height: 20px;
    padding: 0;
  }
  .field.gain input[type="number"] {
    width: 46px; /* e.g. -12.3 */
    flex: none;
  }
  .field small {
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .remove {
    width: 20px;
    height: 20px;
    padding: 0;
    font-size: 12px;
    line-height: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
</style>
