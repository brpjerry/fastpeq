<script lang="ts">
  import BandRow from "./BandRow.svelte";
  import type { EditorBand } from "./history.svelte";
  import type { Channel } from "./types";

  let {
    bands = $bindable(),
    view = $bindable(),
    hoveredId = $bindable(),
    offloadedIdx = new Set<number>(),
    onSchedule,
    onChangeKind,
    onRemoveBand,
  }: {
    bands: EditorBand[];
    view: "both" | "left" | "right";
    hoveredId: number | null;
    /** Band indices currently sent to the hardware device (shown with a chip). */
    offloadedIdx?: Set<number>;
    onSchedule: () => void;
    onChangeKind: (band: EditorBand) => void;
    onRemoveBand: (id: number) => void;
  } = $props();

  function inView(c: Channel, v: "both" | "left" | "right"): boolean {
    if (v === "left") return c.kind === "left";
    if (v === "right") return c.kind === "right";
    return c.kind === "both" || c.kind === "other";
  }

  const shown = $derived(bands.filter((b) => inView(b.channel, view)));
  const counts = $derived({
    both: bands.filter((b) => inView(b.channel, "both")).length,
    left: bands.filter((b) => b.channel.kind === "left").length,
    right: bands.filter((b) => b.channel.kind === "right").length,
  });

  function emptyMsg(v: "both" | "left" | "right"): string {
    if (v === "left") return "No left-only filters yet.";
    if (v === "right") return "No right-only filters yet.";
    return "No filters yet — add a band to start shaping the curve.";
  }
</script>

<div class="bands-head">
  <div class="seg view-seg" role="group" aria-label="Channel filter list">
    <button class:sel={view === "both"} onclick={() => (view = "both")} title="Filters applied to both channels">
      L+R{#if counts.both} · {counts.both}{/if}
    </button>
    <button class:sel={view === "left"} onclick={() => (view = "left")} title="Left-channel-only filters">
      L{#if counts.left} · {counts.left}{/if}
    </button>
    <button class:sel={view === "right"} onclick={() => (view = "right")} title="Right-channel-only filters">
      R{#if counts.right} · {counts.right}{/if}
    </button>
  </div>
</div>

<div class="bands">
  {#each bands as band, i (band.id)}
    {#if inView(band.channel, view)}
      <BandRow
        bind:band={bands[i]}
        hovered={hoveredId === band.id}
        offloaded={offloadedIdx.has(i)}
        onChange={onSchedule}
        onChangeKind={() => onChangeKind(band)}
        onRemove={() => onRemoveBand(band.id)}
        onHover={(active) => (hoveredId = active ? band.id : null)}
      />
    {/if}
  {/each}
  {#if !shown.length}
    <div class="none">{emptyMsg(view)}</div>
  {/if}
</div>

<style>
  .bands-head {
    display: flex;
    align-items: center;
    margin-bottom: 6px;
  }
  .view-seg {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: 7px;
    overflow: hidden;
  }
  .view-seg button {
    border: none;
    border-right: 1px solid var(--border);
    border-radius: 0;
    background: transparent;
    padding: 4px 12px;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .view-seg button:last-child {
    border-right: none;
  }
  .view-seg button:hover:not(.sel) {
    background: var(--panel-2);
    color: var(--text);
  }
  .view-seg button.sel {
    background: var(--accent);
    color: #fff;
  }
  .bands {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .none {
    color: var(--muted);
    padding: 12px 6px;
  }
  @media (max-width: 820px) {
    .bands {
      flex: none;
      overflow: visible;
    }
  }
</style>
