<script lang="ts">
  // One row of the band list: status toggle, type picker, frequency, gain
  // (slider + number, only for gain types), and Q (only for Q types), plus a
  // remove button. Extracted from Editor; it mutates the passed `band` proxy in
  // place (so edits flow straight back) and emits semantic callbacks for the
  // throttled apply, the type change (which may seed a default Q), removal, and
  // graph-row hover linking.
  //
  // The status toggle replaces a plain enable checkbox: same click-to-toggle
  // behavior, but its fixed-width label also says where the band runs. With a
  // hybrid offload split it reads APO / HW / OFF; otherwise ON / OFF (a single
  // engine — Equalizer APO alone, or Hardware Only — needs no distinction).
  // `offloaded` is the backend's word on device membership, passed through
  // untouched — this row never infers it from mode, order, or position.
  import TypeSelect from "./TypeSelect.svelte";
  import { DEFAULT_GAIN_LIMIT, clampGainTo, kindHasGain, kindHasQ } from "./eq";
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
    gainLimit = DEFAULT_GAIN_LIMIT,
    offloaded = false,
    muted = false,
    hybrid = false,
    onChange,
    onChangeKind,
    onRemove,
    onHover,
  }: {
    band: Band;
    hovered: boolean;
    /** How far this band's gain may be edited, in dB either way (Settings).
     * Only edits are held to it — a gain loaded from a preset is left alone
     * until it is edited. */
    gainLimit?: number;
    /** This band is currently sent to the hardware device (backend-decided). */
    offloaded?: boolean;
    /** Enabled but running nowhere: Hardware Only offload left it off the device
     * and Equalizer APO stays flat. */
    muted?: boolean;
    /** A hybrid offload mode is on (some bands on the device, the rest in APO),
     * so the status label distinguishes APO from HW. */
    hybrid?: boolean;
    onChange: () => void;
    onChangeKind: () => void;
    onRemove: () => void;
    onHover: (hovered: boolean) => void;
  } = $props();

  const status = $derived(
    !band.enabled ? "OFF" : hybrid ? (offloaded ? "HW" : "APO") : "ON",
  );
  const statusTitle = $derived(
    !band.enabled
      ? "Off — click to enable"
      : muted
        ? "Enabled, but doesn't fit on the device — muted while Hardware Only keeps Equalizer APO flat. Click to disable."
        : status === "HW"
          ? "Runs on the hardware device — click to disable"
          : status === "APO"
            ? "Runs in Equalizer APO — click to disable"
            : "On — click to disable",
  );

  function toggle() {
    band.enabled = !band.enabled;
    onChange();
  }

  // The gain limit is enforced on edit, never on display: a preset written
  // before the limit (or under a wider one) keeps its gain, and the controls
  // stretch to show it, so nothing silently moves when a preset is loaded or
  // the setting is lowered. The first edit of that gain pulls it inside the
  // limit — which is also why the inputs' own min/max can't do this job alone
  // (typing past the limit, or dragging a stretched slider, still has to land
  // inside it). Only the gain is held to it: changing this band's frequency,
  // Q, or type leaves an over-limit gain where it is.
  const gainBound = $derived(Math.max(gainLimit, Math.abs(band.gain)));
  const overLimit = $derived(Math.abs(band.gain) > gainLimit);

  function changeGain() {
    band.gain = clampGainTo(band.gain, gainLimit);
    onChange();
  }
</script>

<div
  class="band"
  class:off={!band.enabled}
  class:hover={hovered}
  class:muted
  onmouseenter={() => onHover(true)}
  onmouseleave={() => onHover(false)}
  role="presentation"
>
  <button
    class="status"
    class:hw={status === "HW"}
    class:silent={muted}
    role="switch"
    aria-checked={band.enabled}
    aria-label="Enable band"
    title={statusTitle}
    onclick={toggle}
  >{status}</button>
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
    <span class="field gain" class:over={overLimit}>
      <input
        type="range"
        min={-gainBound}
        max={gainBound}
        step="0.1"
        tabindex="-1"
        bind:value={band.gain}
        oninput={changeGain}
        oncontextmenu={() => {
          band.gain = 0;
          onChange();
        }}
        title={overLimit
          ? `${band.gain.toFixed(1)} dB is past the ${gainLimit} dB limit — kept from the preset, but editing it pulls it back to ${gainLimit} dB. Right-click to reset to 0 dB.`
          : "Right-click to reset to 0 dB"}
      />
      <input
        type="number"
        min={-gainBound}
        max={gainBound}
        step="0.1"
        bind:value={band.gain}
        onchange={changeGain}
      />
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
  /* Fixed footprint no matter which state it shows, so toggling a band (or the
     offload session coming and going) never shifts the row's other fields. */
  .status {
    flex: none;
    width: 34px;
    padding: 2px 0;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: transparent;
    color: var(--muted);
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.4px;
    line-height: 1.3;
    text-align: center;
  }
  .status.hw {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  /* Enabled but silent (Hardware Only overflow): hollow, so it reads as inert. */
  .status.silent {
    border-style: dashed;
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
  /* A gain the preset brought in from above the limit: flagged (not corrected)
     until it's edited, so it never looks like a value the editor just chose. */
  .field.gain.over input[type="number"] {
    border-color: #e0a458;
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
  /* Enabled but silent: Hardware Only left it off the device and APO is flat. */
  .band.muted {
    opacity: 0.65;
  }
</style>
