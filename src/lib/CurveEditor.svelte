<script lang="ts">
  // Large, interactive response graph for the expanded view. Renders the same
  // biquad curve as ResponseCurve but adds a draggable handle per band: drag to
  // set frequency (x) and gain (y), scroll over a handle to change Q. It mutates
  // the band objects in place (they are the parent's $state) and calls onChange
  // so the live-apply throttle fires, exactly like the sliders do.
  import {
    responseCurve,
    inChannel,
    balanceTrim,
    kindHasGain,
    kindHasQ,
    type CurveFilter,
  } from "./eq";
  import { sampleAt, type MeasPoint } from "./measurement";
  import {
    freqToX,
    xToFreq,
    dbToY,
    yToGain,
    dbGridLines,
    pathFrom,
    F_MIN,
    F_MAX,
    type PlotBox,
  } from "./graph";
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
    bands,
    preamp = 0,
    balance = 0,
    view = "both",
    measurement = [],
    hoveredId = null,
    filterShapes = false,
    onChange,
    onHover,
  }: {
    bands: Band[];
    preamp?: number;
    balance?: number;
    view?: "both" | "left" | "right";
    measurement?: MeasPoint[];
    hoveredId?: number | null;
    filterShapes?: boolean;
    onChange: () => void;
    onHover?: (id: number | null) => void;
  } = $props();

  // Handles are limited to the channel list in view; the curve traces below
  // still use every band, so the graph reflects the real per-channel response.
  const inView = (c: Channel) =>
    view === "left" ? c.kind === "left" : view === "right" ? c.kind === "right" : c.kind === "both" || c.kind === "other";
  const handleBands = $derived(bands.filter((b) => inView(b.channel)));

  // Measured pixel size; the viewBox matches it 1:1 so pointer coords map directly.
  let w = $state(0);
  let h = $state(0);
  let svgEl = $state<SVGSVGElement | null>(null);
  let dragId = $state<number | null>(null);
  let cursorX = $state<number | null>(null); // pointer x for the freq crosshair

  const dbMax = 30; // vertical range; matches the gain slider's ±30
  const padL = 42;
  const padR = 16;
  const padT = 16;
  const padB = 26;

  const ready = $derived(w > 40 && h > 40);
  const plotW = $derived(Math.max(1, w - padL - padR));
  const plotH = $derived(Math.max(1, h - padT - padB));
  const box = $derived<PlotBox>({ padL, padT, plotW, plotH });

  const xOf = (f: number) => freqToX(f, box);
  // The view is centred on the preamp: handles stay put (drawn at preamp+gain)
  // while the curve stays centred and the 0 line / scale shift instead.
  const yOf = (db: number) => dbToY(db, preamp, dbMax, box);
  const freqAt = (x: number) => xToFreq(x, box);
  const gainAt = (y: number) => yToGain(y, dbMax, box);

  const clampGain = (g: number) => Math.max(-dbMax, Math.min(dbMax, g));
  const clampFreq = (f: number) => Math.max(10, Math.min(24000, f));
  const round1 = (v: number) => Math.round(v * 10) / 10;

  const trim = $derived(balanceTrim(balance));
  const stereo = $derived(bands.some((b) => b.channel.kind !== "both") || balance !== 0);

  // Sample the curve at ~one point per horizontal pixel so the line stays crisp
  // as the graph grows — a fixed grid looks coarse on a large plot.
  const freqs = $derived.by(() => {
    const n = Math.max(120, Math.min(1600, Math.round(plotW)));
    return Array.from({ length: n }, (_, i) => F_MIN * Math.pow(F_MAX / F_MIN, i / (n - 1)));
  });

  const pathFor = (curve: number[]) => pathFrom(curve, freqs, preamp, dbMax, box);
  const sideFilters = (side: "left" | "right"): CurveFilter[] =>
    bands.filter((b) => inChannel(b.channel, side));
  // The imported measurement, sampled onto the plot grid. When present the
  // filter traces become "measurement + filters" — i.e. the corrected response.
  const measCurve = $derived(ready && measurement.length ? sampleAt(measurement, freqs) : null);
  const withMeas = (resp: number[]): number[] =>
    measCurve ? resp.map((v, i) => v + measCurve[i]) : resp;
  const leftPath = $derived(
    ready ? pathFor(withMeas(responseCurve(sideFilters("left"), preamp + trim.left, freqs))) : "",
  );
  const rightPath = $derived(
    ready ? pathFor(withMeas(responseCurve(sideFilters("right"), preamp + trim.right, freqs))) : "",
  );
  // The reference gets the preamp too, so it sits at the same baseline as the
  // result traces and the gap between them is purely the filter shaping.
  const measPath = $derived(measCurve ? pathFor(measCurve.map((v) => v + preamp)) : "");

  // Full 1–9-per-decade log grid; the 1-2-5 lines (labelled) draw brighter than
  // the minor lines in between, giving a denser but still readable frequency grid.
  const gridF = (() => {
    const out: number[] = [];
    for (const dec of [10, 100, 1000, 10000]) {
      for (let m = 1; m <= 9; m++) {
        const f = dec * m;
        if (f >= 20 && f <= 20000) out.push(f);
      }
    }
    out.push(20000);
    return out;
  })();
  const gridDb = $derived(dbGridLines(preamp, dbMax, 10));
  const labF = [
    { f: 20, t: "20" },
    { f: 50, t: "50" },
    { f: 100, t: "100" },
    { f: 200, t: "200" },
    { f: 500, t: "500" },
    { f: 1000, t: "1k" },
    { f: 2000, t: "2k" },
    { f: 5000, t: "5k" },
    { f: 10000, t: "10k" },
    { f: 20000, t: "20k" },
  ];
  const majorF = new Set(labF.map((l) => l.f));

  // One band's own response shape (preamp-centred), so the trace passes through
  // its handle. Drawn instead of the dashed stem when filter shapes are on.
  const shapePath = (b: Band) =>
    pathFor(
      responseCurve(
        [
          {
            enabled: true,
            kind: b.kind,
            freq: b.freq,
            gain: kindHasGain(b.kind) ? b.gain : null,
            q: kindHasQ(b.kind) ? b.q : null,
            channel: b.channel,
          },
        ],
        preamp,
        freqs,
      ),
    );

  const handleColor = (b: Band) => (b.channel.kind === "right" ? "#e0a458" : "var(--accent)");
  // Handles sit at the filter's output level (preamp + gain), so with the
  // preamp-centred view a flat filter lands on the centre line.
  const handleY = (b: Band) => yOf(preamp + (kindHasGain(b.kind) ? clampGain(b.gain) : 0));

  function ptToData(e: PointerEvent) {
    const rect = svgEl?.getBoundingClientRect();
    if (!rect) return null;
    return { x: e.clientX - rect.left, y: e.clientY - rect.top };
  }
  function onDown(e: PointerEvent, b: Band) {
    e.preventDefault();
    dragId = b.id;
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
  }
  function onMove(e: PointerEvent, b: Band) {
    if (dragId !== b.id) return;
    const p = ptToData(e);
    if (!p) return;
    cursorX = p.x; // keep the crosshair with the drag (capture hides svg moves)
    b.freq = Math.round(clampFreq(freqAt(p.x)));
    if (kindHasGain(b.kind)) b.gain = round1(clampGain(gainAt(p.y)));
    onChange();
  }
  function onUp(e: PointerEvent, b: Band) {
    if (dragId === b.id) dragId = null;
    (e.currentTarget as Element).releasePointerCapture?.(e.pointerId);
  }
  function onWheel(e: WheelEvent, b: Band) {
    if (!kindHasQ(b.kind)) return;
    e.preventDefault();
    b.q = Math.max(0.1, Math.min(36, round1(b.q + (e.deltaY < 0 ? 0.1 : -0.1))));
    onChange();
  }
  function onCursorMove(e: PointerEvent) {
    const p = ptToData(e);
    cursorX = p ? p.x : null;
  }
  function onCursorLeave() {
    cursorX = null;
  }

  const fmtFreq = (f: number) =>
    f >= 1000 ? (f / 1000).toFixed(f % 1000 === 0 ? 0 : 1) + "k" : String(Math.round(f));

  function freqLabel(f: number): string {
    if (f >= 10000) return (f / 1000).toFixed(1) + " kHz";
    if (f >= 1000) return (f / 1000).toFixed(2) + " kHz";
    return Math.round(f) + " Hz";
  }
  // Crosshair readout: a vertical line + frequency label that track the pointer.
  const cursor = $derived.by(() => {
    if (!ready || cursorX === null || cursorX < padL || cursorX > w - padR) return null;
    const text = freqLabel(freqAt(cursorX));
    const width = text.length * 6.3 + 12;
    const lx = Math.max(padL + width / 2, Math.min(w - padR - width / 2, cursorX));
    return { x: cursorX, text, lx, width };
  });
</script>

<div class="ce-wrap" bind:clientWidth={w} bind:clientHeight={h}>
  {#if ready}
    <svg
      bind:this={svgEl}
      viewBox="0 0 {w} {h}"
      class="ce-svg"
      preserveAspectRatio="none"
      role="application"
      aria-label="Response graph editor"
      onpointermove={onCursorMove}
      onpointerleave={onCursorLeave}
    >
      <rect x="0" y="0" width={w} height={h} class="bg" />
      {#each gridF as f}
        <line x1={xOf(f)} y1={padT} x2={xOf(f)} y2={h - padB} class={majorF.has(f) ? "grid" : "grid minor"} />
      {/each}
      {#each gridDb as db}
        <line x1={padL} y1={yOf(db)} x2={w - padR} y2={yOf(db)} class={db === 0 ? "axis" : "grid"} />
        <text x={padL - 6} y={yOf(db) + 3} class="lbl" text-anchor="end">{db > 0 ? "+" + db : db}</text>
      {/each}
      {#each labF as l}
        <text x={xOf(l.f)} y={h - 8} class="lbl" text-anchor="middle">{l.t}</text>
      {/each}

      {#if measCurve}
        <path d={measPath} class="resp reference" />
      {/if}
      {#if stereo}
        <path d={rightPath} class="resp right" />
        <path d={leftPath} class="resp left" />
      {:else}
        <path d={leftPath} class="resp left" />
      {/if}

      {#each handleBands as band, i (band.id)}
        {@const hx = xOf(band.freq)}
        {@const hy = handleY(band)}
        <g
          class="handle"
          class:off={!band.enabled}
          class:dragging={dragId === band.id}
          class:active={hoveredId === band.id}
          onpointerdown={(e) => onDown(e, band)}
          onpointermove={(e) => onMove(e, band)}
          onpointerup={(e) => onUp(e, band)}
          onpointerenter={() => onHover?.(band.id)}
          onpointerleave={() => onHover?.(null)}
          onwheel={(e) => onWheel(e, band)}
          role="slider"
          tabindex="-1"
          aria-label={`Band ${i + 1}`}
          aria-valuenow={band.freq}
        >
          {#if filterShapes}
            <path d={shapePath(band)} class="shape" style:stroke={handleColor(band)} />
          {:else if kindHasGain(band.kind)}
            <line x1={hx} y1={yOf(preamp)} x2={hx} y2={hy} class="stem" style:stroke={handleColor(band)} />
          {/if}
          <circle cx={hx} cy={hy} r="11" class="hit" />
          <circle cx={hx} cy={hy} r="7" class="dot" style:fill={handleColor(band)} />
          <text x={hx} y={hy} dy="0.4em" class="hnum" text-anchor="middle">{i + 1}</text>
          {#if dragId === band.id}
            <text x={hx} y={hy - 16} class="htip" text-anchor="middle">
              {fmtFreq(band.freq)} Hz{kindHasGain(band.kind) ? ` · ${band.gain.toFixed(1)} dB` : ""}{kindHasQ(
                band.kind,
              )
                ? ` · Q ${band.q.toFixed(1)}`
                : ""}
            </text>
          {/if}
        </g>
      {/each}

      {#if cursor}
        <line x1={cursor.x} y1={padT} x2={cursor.x} y2={h - padB} class="cursor-line" />
        <rect
          x={cursor.lx - cursor.width / 2}
          y={h - 18}
          width={cursor.width}
          height="14"
          rx="3"
          class="cursor-bg"
        />
        <text x={cursor.lx} y={h - 8} class="cursor-lbl" text-anchor="middle">{cursor.text}</text>
      {/if}
    </svg>
  {/if}
</div>

<style>
  .ce-wrap {
    position: relative;
    /* Largest 8:5 box that fits the size-containment parent (.graph-fit):
       width is capped by both the parent's width (100cqw) and 1.6× its height
       (160cqh), and aspect-ratio fixes the height. Scales with the pane, never
       overflows → no scrollbar. */
    width: min(100cqw, 160cqh);
    aspect-ratio: 8 / 5;
    max-width: 100%;
    max-height: 100%;
  }
  .ce-svg {
    display: block;
    width: 100%;
    height: 100%;
    user-select: none;
    touch-action: none;
  }
  .bg {
    fill: #181b21;
    stroke: var(--border);
  }
  .grid {
    stroke: #2a2f38;
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }
  .grid.minor {
    stroke: #21252c;
  }
  .axis {
    stroke: #3a4150;
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }
  .cursor-line {
    stroke: var(--text);
    stroke-width: 1;
    opacity: 0.35;
    pointer-events: none;
    vector-effect: non-scaling-stroke;
  }
  .cursor-bg {
    fill: #181b21;
    opacity: 0.92;
    pointer-events: none;
  }
  .cursor-lbl {
    fill: var(--text);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
  }
  .resp {
    fill: none;
    stroke-width: 2;
    vector-effect: non-scaling-stroke;
  }
  .resp.left {
    stroke: var(--accent);
  }
  .resp.right {
    stroke: #e0a458;
  }
  .resp.reference {
    stroke: var(--muted);
    stroke-width: 1.5;
    stroke-dasharray: 4 3;
    opacity: 0.65;
  }
  .lbl {
    fill: var(--muted);
    font-size: 11px;
  }
  .handle {
    cursor: pointer;
  }
  .handle.dragging {
    cursor: grabbing;
  }
  .handle.off {
    opacity: 0.4;
  }
  .hit {
    fill: transparent;
    pointer-events: all;
  }
  .dot {
    pointer-events: none;
    stroke: #11141a;
    stroke-width: 1.5;
  }
  /* Light up on direct hover, or when highlighted from the list (hover/edit). */
  .handle:hover .dot,
  .handle.active .dot {
    stroke: #fff;
    stroke-width: 2.5;
  }
  .stem {
    pointer-events: none;
    stroke-width: 1.5;
    stroke-dasharray: 2 3;
    opacity: 0.7;
    vector-effect: non-scaling-stroke;
  }
  /* Per-band filter shape passing through its handle. */
  .shape {
    fill: none;
    pointer-events: none;
    stroke-width: 1.5;
    opacity: 0.5;
    vector-effect: non-scaling-stroke;
  }
  .hnum {
    pointer-events: none;
    fill: #fff;
    font-size: 10px;
    font-weight: 600;
  }
  .htip {
    pointer-events: none;
    fill: var(--text);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    paint-order: stroke;
    stroke: #11141a;
    stroke-width: 3;
  }
</style>
