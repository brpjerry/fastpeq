<script lang="ts">
  // The curve editor's tool row beneath the graph hint: target selector +
  // visibility, compensate, and measurement import — all per preset. Reads/writes
  // its per-preset state via presetView; the editor passes the effective
  // compensate/canCompensate it already derives plus the import/clear actions.
  import { getTargets, getTarget } from "./targets.svelte";
  import {
    getTargetId,
    setTargetId,
    setCompensate,
    getShowTargetRef,
    setShowTargetRef,
    getShowMeasRef,
    setShowMeasRef,
    getTargetOffset,
    setTargetOffset,
    getTargetAlignFreq,
    setTargetAlignFreq,
  } from "./presetView.svelte";
  import Switch from "./Switch.svelte";
  import SelectMenu from "./SelectMenu.svelte";
  import type { MeasPoint } from "./measurement";

  let {
    name,
    compensate,
    canCompensate,
    measurement,
    measName,
    onImport,
    onClear,
    onAlign,
  }: {
    name: string;
    compensate: boolean;
    canCompensate: boolean;
    measurement: MeasPoint[];
    measName: string;
    onImport: () => void;
    onClear: () => void;
    onAlign: () => void;
  } = $props();

  // Offset/match only do something for a real (non-flat) target, so the trace
  // controls only appear then — keeps the primary row uncluttered for Flat.
  const adjustable = $derived(getTarget(getTargetId(name)).points.length > 0);
  const clampFreq = (v: number) => Math.max(20, Math.min(20000, v || 1000));
</script>

<div class="graph-tools">
  <p class="graph-hint">Drag a handle to set frequency &amp; gain · scroll over a handle to change Q</p>
  <div class="meas-tools">
    <div class="target-group">
      <Switch
        compact
        disabled={compensate}
        title={compensate
          ? "Compensating — the target is the reference (flat line)"
          : "Show the target dashed line"}
        checked={compensate || getShowTargetRef(name)}
        onChange={(v) => setShowTargetRef(name, v)}
      />
      <span class="target-select" title="Reference target curve (add targets in Settings)">
        Target
        <SelectMenu
          value={getTargetId(name)}
          options={getTargets().map((t) => ({ value: t.id, label: t.name }))}
          onChange={(v) => setTargetId(name, v)}
        />
      </span>
    </div>
    <Switch
      compact
      label="Compensate"
      disabled={!canCompensate}
      title={canCompensate
        ? "Show the response as deviation from the target (flat = on target)"
        : "Select a non-flat target and show it to compensate"}
      checked={compensate}
      onChange={(v) => setCompensate(name, v)}
    />
    <!-- Measurement switch (no label) + label + selector, kept on one line. -->
    <div class="meas-group">
      <Switch
        compact
        disabled={measurement.length === 0}
        title={measurement.length
          ? "Show the raw measurement dashed line (the FR trace keeps the measurement either way)"
          : "Import a measurement to enable"}
        checked={measurement.length > 0 && getShowMeasRef(name)}
        onChange={(v) => setShowMeasRef(name, v)}
      />
      {#if measurement.length}
        <span class="meas-name" title={measName}>{measName}</span>
        <button onclick={onClear}>Clear</button>
      {:else}
        <button onclick={onImport}>Import REW…</button>
      {/if}
    </div>
  </div>
  {#if adjustable}
    <!-- Target trace controls — a manual offset and an align-at-frequency Match,
         only shown for a non-flat target. -->
    <div class="target-adjust">
      <label class="adj-group" title="Shift the whole target trace up or down">
        Target offset
        <input
          class="num"
          type="number"
          step="0.5"
          value={getTargetOffset(name)}
          onchange={(e) => setTargetOffset(name, Number(e.currentTarget.value) || 0)}
        />
        <small>dB</small>
      </label>
      <span class="adj-group">
        <label title="The frequency Align pins the target to the response at">
          Align at
          <input
            class="num"
            type="number"
            min="20"
            max="20000"
            step="10"
            value={getTargetAlignFreq(name)}
            onchange={(e) => setTargetAlignFreq(name, clampFreq(Number(e.currentTarget.value)))}
          />
          <small>Hz</small>
        </label>
        <button onclick={onAlign} title="Shift the target so it meets the response at this frequency">
          Align
        </button>
      </span>
    </div>
  {/if}
</div>

<style>
  .graph-hint {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
  }
  /* Hint centered on its own line above the controls so the controls (which
     wrap) can never squish the instruction text. */
  .graph-tools {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }
  .meas-tools {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    flex-wrap: wrap;
  }
  /* A bare toggle stays beside its labeled control as one unit when the row wraps. */
  .target-group,
  .meas-group {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex-wrap: nowrap;
    white-space: nowrap;
  }
  .meas-tools button {
    padding: 3px 10px;
    font-size: 12px;
  }
  .meas-name {
    font-size: 12px;
    color: var(--accent);
    max-width: 240px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .target-select {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    color: var(--muted);
  }
  /* Second, contextual row: the target trace adjustments (offset + match). */
  .target-adjust {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 18px;
    flex-wrap: wrap;
  }
  .adj-group,
  .adj-group label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
  }
  .target-adjust .num {
    width: 58px;
    padding: 2px 5px;
    font-size: 12px;
  }
  .target-adjust button {
    padding: 3px 10px;
    font-size: 12px;
  }
</style>
