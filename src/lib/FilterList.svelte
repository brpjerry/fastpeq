<script lang="ts">
  import BandRow from "./BandRow.svelte";
  import { bandInView, type BandView } from "./eq";
  import type { EditorBand } from "./history.svelte";

  let {
    bands = $bindable(),
    view = $bindable(),
    hoveredId = $bindable(),
    offloadedIdx = new Set<number>(),
    mutedIds = new Set<number>(),
    hybrid = false,
    onSchedule,
    onChangeKind,
    onRemoveBand,
  }: {
    bands: EditorBand[];
    view: BandView;
    hoveredId: number | null;
    /** Band indices currently sent to the hardware device — the backend's
     * selection verbatim; membership is never re-derived here (it may be
     * mode-driven today, user-assigned tomorrow). */
    offloadedIdx?: Set<number>;
    /** Band ids muted by Hardware Only offload — enabled but running nowhere
     * (they didn't fit on the device and APO stays flat). */
    mutedIds?: Set<number>;
    /** A hybrid offload mode is on: the L+R list splits into APO and HW views,
     * and rows label where they run. */
    hybrid?: boolean;
    onSchedule: () => void;
    onChangeKind: (band: EditorBand) => void;
    onRemoveBand: (id: number) => void;
  } = $props();

  const inView = (b: EditorBand, i: number, v: BandView) =>
    bandInView(b.channel, offloadedIdx.has(i), v);

  const shown = $derived(bands.filter((b, i) => inView(b, i, view)));
  const count = (v: BandView) => bands.filter((b, i) => inView(b, i, v)).length;
  const counts = $derived({
    both: count("both"),
    apo: count("apo"),
    hw: count("hw"),
    left: count("left"),
    right: count("right"),
  });

  // Tab caption: the view name plus its band count when nonzero.
  const label = (name: string, n: number) => (n ? `${name} · ${n}` : name);

  function emptyMsg(v: BandView): string {
    if (v === "left") return "No left-only filters yet.";
    if (v === "right") return "No right-only filters yet.";
    if (v === "apo") return "No filters in Equalizer APO — they all fit on the device.";
    if (v === "hw") return "No filters on the hardware device yet.";
    return "No filters yet — add a band to start shaping the curve.";
  }
</script>

<div class="bands-head">
  <div class="seg view-seg" role="group" aria-label="Channel filter list">
    {#if hybrid}
      <button class:sel={view === "apo"} onclick={() => (view = "apo")} title="Both-channel filters running in Equalizer APO">
        {label("L+R APO", counts.apo)}
      </button>
      <button class:sel={view === "hw"} onclick={() => (view = "hw")} title="Both-channel filters running on the hardware device">
        {label("L+R HW", counts.hw)}
      </button>
    {:else}
      <button class:sel={view === "both"} onclick={() => (view = "both")} title="Filters applied to both channels">
        {label("L+R", counts.both)}
      </button>
    {/if}
    <button class:sel={view === "left"} onclick={() => (view = "left")} title="Left-channel-only filters">
      {label("L", counts.left)}
    </button>
    <button class:sel={view === "right"} onclick={() => (view = "right")} title="Right-channel-only filters">
      {label("R", counts.right)}
    </button>
  </div>
</div>

<div class="bands">
  {#each bands as band, i (band.id)}
    {#if inView(band, i, view)}
      <BandRow
        bind:band={bands[i]}
        hovered={hoveredId === band.id}
        offloaded={offloadedIdx.has(i)}
        muted={mutedIds.has(band.id)}
        {hybrid}
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
    white-space: nowrap;
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
