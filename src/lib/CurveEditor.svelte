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
    type BandView,
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
  import { gapDb, targetValueAt, compensateCurve } from "./curve";
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
    target = [],
    compensate = false,
    showMeas = true,
    showTarget = true,
    hoveredId = null,
    mutedIds = new Set<number>(),
    filterShapes = false,
    reference = null,
    onChange,
    onHover,
  }: {
    bands: Band[];
    preamp?: number;
    balance?: number;
    view?: BandView;
    measurement?: MeasPoint[];
    target?: MeasPoint[];
    compensate?: boolean;
    showMeas?: boolean;
    showTarget?: boolean;
    hoveredId?: number | null;
    /** Band ids muted by Hardware Only offload: still editable (dimmed handle),
     * but excluded from the response traces — they run nowhere. */
    mutedIds?: Set<number>;
    filterShapes?: boolean;
    // A second response (the saved version during A/B compare) drawn as a faded
    // ghost so the difference from the working edit is visible, not just audible.
    reference?: { filters: CurveFilter[]; preamp: number; balance: number } | null;
    onChange: () => void;
    onHover?: (id: number | null) => void;
  } = $props();

  // Handles are limited to the channel list in view; the curve traces below
  // still use every band, so the graph reflects the real per-channel response.
  // The hybrid APO/HW split views both map to the both-channel handles — where a
  // band runs shouldn't gate dragging it on the graph.
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
    bands.filter((b) => !mutedIds.has(b.id) && inChannel(b.channel, side));
  // The imported measurement, sampled onto the plot grid. When present the
  // filter traces become "measurement + filters" — i.e. the corrected response.
  const measCurve = $derived(ready && measurement.length ? sampleAt(measurement, freqs) : null);
  const withMeas = (resp: number[]): number[] =>
    measCurve ? resp.map((v, i) => v + measCurve[i]) : resp;

  // The selected target, sampled onto the grid. With "compensate" on, every
  // trace is shown as deviation from the target (flat = on target), so the
  // target itself collapses to the centre line and isn't drawn.
  const targetCurve = $derived(ready && target.length ? sampleAt(target, freqs) : null);
  const compCurve = $derived(compensate && targetCurve ? targetCurve : null);
  const compensated = (resp: number[]): number[] =>
    compCurve ? compensateCurve(resp, compCurve) : resp;

  // The target line as displayed: the target curve normally, but a flat
  // centerline when compensating (the target collapses to flat) or when the
  // Flat target is selected.
  const targetLine = $derived.by<number[] | null>(() => {
    if (!ready) return null;
    if (!compensate && targetCurve) return targetCurve.map((v) => v + preamp);
    return freqs.map(() => preamp);
  });

  const leftPath = $derived(
    ready ? pathFor(compensated(withMeas(responseCurve(sideFilters("left"), preamp + trim.left, freqs)))) : "",
  );
  const rightPath = $derived(
    ready ? pathFor(compensated(withMeas(responseCurve(sideFilters("right"), preamp + trim.right, freqs)))) : "",
  );
  // The reference gets the preamp too, so it sits at the same baseline as the
  // result traces and the gap between them is purely the filter shaping.
  const measPath = $derived(measCurve ? pathFor(compensated(measCurve.map((v) => v + preamp))) : "");
  const targetPath = $derived(targetLine ? pathFor(targetLine) : "");
  // The saved-version ghost during A/B compare, through the same meas/compensate
  // transforms so it lines up with the working trace.
  const refPath = $derived.by(() => {
    if (!ready || !reference) return "";
    const rt = balanceTrim(reference.balance);
    return pathFor(
      compensated(
        withMeas(
          responseCurve(
            reference.filters.filter((f) => inChannel(f.channel, "left")),
            reference.preamp + rt.left,
            freqs,
          ),
        ),
      ),
    );
  });

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

  const handleColor = (b: Band) => (b.channel.kind === "right" ? "var(--chan-right)" : "var(--accent)");
  // Handles sit at the filter's output level (preamp + gain), so with the
  // preamp-centred view a flat filter lands on the centre line.
  const handleY = (b: Band) => yOf(preamp + (kindHasGain(b.kind) ? clampGain(b.gain) : 0));

  function ptToData(e: PointerEvent) {
    const rect = svgEl?.getBoundingClientRect();
    if (!rect) return null;
    return { x: e.clientX - rect.left, y: e.clientY - rect.top };
  }
  // Drag via window-level listeners rather than setPointerCapture (a captured
  // pointer renders the cursor pixelated at 1x on high-DPI WebView). No cursor
  // override while dragging — the hover cursor just stays as-is.
  function onDown(e: PointerEvent, b: Band) {
    e.preventDefault();
    dragId = b.id;
    const p = ptToData(e);
    if (p) cursorX = p.x;
  }
  function onDragMove(e: PointerEvent) {
    const b = dragId === null ? undefined : bands.find((x) => x.id === dragId);
    if (!b) return;
    const p = ptToData(e);
    if (!p) return;
    cursorX = p.x;
    b.freq = Math.round(clampFreq(freqAt(p.x)));
    if (kindHasGain(b.kind)) b.gain = round1(clampGain(gainAt(p.y)));
    onChange();
  }
  function onDragEnd() {
    dragId = null;
  }
  // Attach the move/up listeners and the grab cursor only while a drag is live;
  // the cleanup runs on drag end (dragId → null) and on unmount.
  $effect(() => {
    if (dragId === null) return;
    window.addEventListener("pointermove", onDragMove);
    window.addEventListener("pointerup", onDragEnd);
    window.addEventListener("pointercancel", onDragEnd);
    return () => {
      window.removeEventListener("pointermove", onDragMove);
      window.removeEventListener("pointerup", onDragEnd);
      window.removeEventListener("pointercancel", onDragEnd);
    };
  });
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
  // Crosshair readout: a vertical line + frequency label that track the pointer,
  // plus the dB gap from each FR trace to the target (only when the target line
  // is actually drawn).
  const cursor = $derived.by(() => {
    if (!ready || cursorX === null || cursorX < padL || cursorX > w - padR) return null;
    const f = freqAt(cursorX);
    const text = freqLabel(f);
    const width = text.length * 6.3 + 12;
    const lx = Math.max(padL + width / 2, Math.min(w - padR - width / 2, cursorX));

    // Absolute dB gap from the FR trace to the target, shown only when the
    // target is enabled. Works in compensate mode too — the value is the same;
    // the target line just sits flat on the centerline there.
    let gap: string | null = null;
    let ly = padT + 12;
    if (showTarget) {
      gap =
        gapDb(sideFilters("left"), preamp + trim.left, measurement, target, preamp, f).toFixed(1) +
        " dB";
      // Sit at the crosshair–target intersection; compensating flattens the
      // target onto the centerline.
      const lineVal = compensate ? preamp : targetValueAt(target, preamp, f);
      ly = Math.max(padT + 12, Math.min(h - padB - 4, yOf(lineVal) - 5));
    }
    return { x: cursorX, text, lx, width, gap, ly };
  });
</script>

<div class="ce-wrap" bind:clientWidth={w} bind:clientHeight={h}>
  {#if ready}
    <svg
      bind:this={svgEl}
      viewBox="0 0 {w} {h}"
      width={w}
      height={h}
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

      <!-- Per-band shapes in one group: the group opacity flattens overlaps so
           crossing shapes don't darken where they stack. Drawn behind the traces. -->
      {#if filterShapes}
        <g class="shapes">
          {#each handleBands as band (band.id)}
            <path d={shapePath(band)} class="shape" style:stroke={handleColor(band)} />
          {/each}
        </g>
      {/if}

      {#if refPath}
        <path d={refPath} class="resp compare" />
      {/if}
      {#if showTarget && targetPath}
        <path d={targetPath} class="resp target" />
      {/if}
      {#if measCurve && showMeas}
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
          class:off={!band.enabled || mutedIds.has(band.id)}
          class:dragging={dragId === band.id}
          class:active={hoveredId === band.id}
          onpointerdown={(e) => onDown(e, band)}
          onpointerenter={() => onHover?.(band.id)}
          onpointerleave={() => onHover?.(null)}
          onwheel={(e) => onWheel(e, band)}
          role="slider"
          tabindex="-1"
          aria-label={`Band ${i + 1}`}
          aria-valuenow={band.freq}
        >
          {#if !filterShapes && kindHasGain(band.kind)}
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
        {#if cursor.gap}
          <text
            x={cursor.x + (cursor.x > w - 56 ? -8 : 8)}
            y={cursor.ly}
            class="delta-lbl"
            text-anchor={cursor.x > w - 56 ? "end" : "start"}>{cursor.gap}</text>
        {/if}
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
    /* Border on the wrap (an HTML element) rather than the SVG rect stroke,
       which sat on the viewBox edge and got half-clipped — vanishing at some
       scales. Clip the sub-pixel gap left by the integer-sized SVG. */
    border: 1px solid var(--border);
    overflow: hidden;
  }
  /* Sized to the integer client box (matching the viewBox) so the SVG renders at
     exactly 1:1 — a fractional scale otherwise pixelates the content and the
     cursor while dragging. */
  .ce-svg {
    display: block;
    user-select: none;
    touch-action: none;
  }
  .bg {
    fill: var(--graph-bg);
  }
  .grid {
    stroke: var(--graph-grid);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }
  .grid.minor {
    stroke: #21252c;
  }
  .axis {
    stroke: var(--graph-axis);
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
    fill: var(--graph-bg);
    opacity: 0.92;
    pointer-events: none;
  }
  .cursor-lbl {
    fill: var(--text);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
  }
  /* FR-to-target gap readout, in the target line's colour. */
  .delta-lbl {
    fill: var(--target);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
    paint-order: stroke;
    stroke: var(--label-outline);
    stroke-width: 3;
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
    stroke: var(--chan-right);
  }
  .resp.reference {
    stroke: var(--muted);
    stroke-width: 1.5;
    stroke-dasharray: 4 3;
    opacity: 0.65;
  }
  .resp.target {
    stroke: var(--target);
    stroke-width: 1.5;
    stroke-dasharray: 6 4;
    opacity: 0.8;
  }
  /* The A/B compare ghost (the saved version). */
  .resp.compare {
    stroke: var(--text);
    stroke-width: 1.5;
    stroke-dasharray: 6 4;
    opacity: 0.4;
  }
  .lbl {
    fill: var(--muted);
    font-size: 11px;
  }
  .handle {
    cursor: pointer;
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
    stroke: var(--label-outline);
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
  /* Per-band filter shapes. The group opacity (not per-path) keeps overlapping
     shapes from compounding into darker bands; kept low so they stay subtle. */
  .shapes {
    opacity: 0.25;
  }
  .shape {
    fill: none;
    pointer-events: none;
    stroke-width: 1.5;
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
    stroke: var(--label-outline);
    stroke-width: 3;
  }
</style>
