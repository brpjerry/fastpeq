<script lang="ts">
  // Read-only preview of a preset, opened by right-clicking its name in the
  // preset list. It floats over the three-pane window (the panes behind are
  // dimmed + blurred) and shows the preset exactly as saved: its response curve
  // on top, its enabled filters below. Nothing here edits — the filter rows are
  // plain readouts (the gain bar is a handle-less slider, not an input), so
  // opening a preview can never touch the live config.
  //
  // The only controls are Back (or Escape), Apply, and Delete; the last two hand
  // straight back to the App's own apply/remove paths, so a delete follows the
  // same archival + undo flow as the list's X button.
  import * as api from "./api";
  import ResponseCurve from "./ResponseCurve.svelte";
  import { FILTER_TYPES, kindHasGain, kindHasQ, parseConfigEq, type CurveFilter } from "./eq";

  let {
    name,
    busy = false,
    onClose,
    onApply,
    onDelete,
  }: {
    name: string;
    /** An App-level action is in flight — the write controls gray out. */
    busy?: boolean;
    onClose: () => void;
    /** Apply the preset (and leave the preview). */
    onApply: (name: string) => void;
    /** Delete the preset — the same archival path as the list's X button. */
    onDelete: (name: string) => void;
  } = $props();

  let filters = $state<CurveFilter[]>([]);
  let preamp = $state(0);
  let balance = $state(0);
  let err = $state("");
  let loading = $state(true);

  // Read the preset file straight from the backend and split it the same way the
  // editor does, so the preview can't drift from what opening it would show.
  $effect(() => {
    const wanted = name;
    let cancelled = false;
    loading = true;
    err = "";
    api
      .getPreset(wanted)
      .then((cfg) => {
        if (cancelled) return;
        const parsed = parseConfigEq(cfg);
        filters = parsed.filters;
        preamp = parsed.preamp;
        balance = parsed.balance;
        loading = false;
      })
      .catch((e) => {
        if (cancelled) return;
        err = String(e);
        filters = [];
        preamp = 0;
        balance = 0;
        loading = false;
      });
    return () => {
      cancelled = true;
    };
  });

  // Only the filters that are actually on get listed (a disabled band does
  // nothing, and this view is about what the preset sounds like).
  const shown = $derived(filters.filter((f) => f.enabled));

  const token = (kind: string) => FILTER_TYPES.find((t) => t.value === kind)?.token ?? kind;
  const label = (kind: string) => FILTER_TYPES.find((t) => t.value === kind)?.label ?? kind;

  // The gain bar spans the editor's own ±30 dB range and fills from 0 dB, so a
  // boost and a cut of the same size read as mirrored bars.
  const GAIN_SPAN = 30;
  const pct = (db: number) => ((Math.max(-GAIN_SPAN, Math.min(GAIN_SPAN, db)) + GAIN_SPAN) / (2 * GAIN_SPAN)) * 100;
  const barLeft = (db: number) => Math.min(50, pct(db));
  const barWidth = (db: number) => Math.abs(pct(db) - 50);

  const fmtGain = (db: number) => `${db > 0 ? "+" : ""}${db.toFixed(1)}`;
</script>

<svelte:window
  onkeydown={(e) => {
    if (e.key === "Escape") onClose();
  }}
/>

<!-- Clicking the backdrop (never the pane itself, which stops at the target
     check) closes the preview, alongside Back and Escape. -->
<div
  class="preview-backdrop"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) onClose();
  }}
>
  <div class="preview" role="dialog" aria-modal="true" aria-label="Preset preview: {name}">
    <div class="preview-head">
      <h2 title={name}>{name}</h2>
      <button class="back" onclick={onClose} title="Back (Esc)" aria-label="Close preview">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <line x1="19" y1="12" x2="5" y2="12" />
          <polyline points="12 19 5 12 12 5" />
        </svg>
      </button>
    </div>

    {#if err}<div class="err">{err}</div>{/if}

    <div class="preview-graph">
      <ResponseCurve {filters} {preamp} {balance} />
    </div>

    <div class="preview-filters">
      {#each shown as f, i (i)}
        <div class="prow">
          {#if f.channel.kind === "left" || f.channel.kind === "right"}
            <span class="chan" title="{f.channel.kind === 'left' ? 'Left' : 'Right'} channel only">
              {f.channel.kind === "left" ? "L" : "R"}
            </span>
          {/if}
          <span class="tok" title={label(f.kind)}>{token(f.kind)}</span>
          <span class="num freq">{f.freq}<small>Hz</small></span>
          <span class="bar" aria-hidden="true">
            <span class="bar-track"></span>
            {#if kindHasGain(f.kind)}
              <span
                class="bar-fill"
                style="left:{barLeft(f.gain ?? 0)}%; width:{barWidth(f.gain ?? 0)}%"
              ></span>
            {/if}
          </span>
          <span class="num gain">
            {#if kindHasGain(f.kind)}{fmtGain(f.gain ?? 0)}<small>dB</small>{:else}<small class="na">—</small>{/if}
          </span>
          <span class="num q">
            {#if kindHasQ(f.kind)}<small>Q</small>{(f.q ?? 0).toFixed(2)}{:else}<small class="na">—</small>{/if}
          </span>
        </div>
      {:else}
        <div class="none">
          {loading ? "Loading…" : err ? "Couldn't read this preset." : "No filters are on in this preset."}
        </div>
      {/each}
    </div>

    <div class="preview-actions">
      <button
        class="primary"
        onclick={() => onApply(name)}
        disabled={busy || loading || !!err}
        title="Load this preset into the live config and the editor"
      >
        Apply
      </button>
      <button
        class="danger"
        onclick={() => onDelete(name)}
        disabled={busy}
        title="Delete this preset (undoable for a few seconds)"
      >
        Delete
      </button>
    </div>
  </div>
</div>

<style>
  /* Sits over the whole three-pane window; the panes behind are blurred (not
     dimmed — a tint would leave the window's own title bar standing out) so the
     preview reads as the only thing in focus. */
  .preview-backdrop {
    position: fixed;
    inset: 0;
    z-index: 200;
    display: grid;
    place-items: center;
    padding: 24px;
    backdrop-filter: blur(5px);
  }
  .preview {
    width: min(620px, 100%);
    max-height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    gap: 10px;
    padding: 12px 14px 14px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 11px;
    box-shadow: 0 18px 50px rgba(0, 0, 0, 0.55);
  }

  .preview-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .preview-head h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Same skin as the header's back-out-of-settings button. */
  .back {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 6px;
    border-radius: 8px;
    color: var(--text);
  }
  .back svg {
    width: 18px;
    height: 18px;
    display: block;
  }

  .preview-graph {
    flex: none;
  }

  .preview-filters {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .prow {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 8px;
    border-bottom: 1px solid var(--border);
  }
  .prow:last-child {
    border-bottom: none;
  }
  /* Left/right-only filters would otherwise be indistinguishable from their
     opposite-channel twin. */
  .chan {
    flex: none;
    width: 15px;
    text-align: center;
    font-size: 10px;
    font-weight: 700;
    color: var(--muted);
  }
  .tok {
    flex: none;
    width: 34px;
    padding: 2px 0;
    border: 1px solid var(--border);
    border-radius: 5px;
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.4px;
    line-height: 1.3;
    text-align: center;
    color: var(--muted);
  }
  .num {
    flex: none;
    display: inline-flex;
    align-items: baseline;
    gap: 3px;
    font-size: 12px;
    color: var(--text);
    font-variant-numeric: tabular-nums;
    justify-content: flex-end;
  }
  .num small {
    font-size: 11px;
    color: var(--muted);
  }
  .num small.na {
    color: var(--border);
  }
  .freq {
    width: 62px;
  }
  .gain {
    width: 58px;
  }
  .q {
    width: 52px;
  }

  /* A slider read-only: the same track the gain slider draws, filled from 0 dB,
     with no drag handle to invite an edit. */
  .bar {
    position: relative;
    flex: 1;
    min-width: 60px;
    height: 20px;
    display: block;
  }
  .bar-track,
  .bar-fill {
    position: absolute;
    top: 50%;
    transform: translateY(-50%);
    height: 4px;
    border-radius: 2px;
  }
  .bar-track {
    left: 0;
    right: 0;
    background: var(--panel-2);
    border: 1px solid var(--border);
  }
  /* One color for both directions: which side of centre the bar fills already
     says cut or boost, and the graph's second color means "right channel". */
  .bar-fill {
    background: var(--accent);
    min-width: 2px;
  }

  .none {
    color: var(--muted);
    padding: 12px 8px;
  }

  .preview-actions {
    flex: none;
    display: flex;
    gap: 8px;
  }
  .preview-actions button {
    flex: 1;
  }
</style>
