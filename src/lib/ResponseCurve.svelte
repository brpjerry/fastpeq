<script lang="ts">
  import { responseCurve, FREQS, inChannel, balanceTrim, type CurveFilter } from "./eq";
  import { freqToX, dbToY, dbGridLines, pathFrom, type PlotBox } from "./graph";

  let {
    filters,
    preamp = 0,
    balance = 0,
  }: { filters: CurveFilter[]; preamp?: number; balance?: number } = $props();

  const W = 600;
  const H = 190;
  const padL = 30;
  const padR = 8;
  const padT = 8;
  const padB = 18;
  const dbMax = 24;

  const box: PlotBox = { padL, padT, plotW: W - padL - padR, plotH: H - padT - padB };
  const xOf = (f: number) => freqToX(f, box);
  const yOf = (db: number) => dbToY(db, preamp, dbMax, box);
  const pathFor = (curve: number[]) => pathFrom(curve, FREQS, preamp, dbMax, box);

  // Only show two traces once something is actually left/right-scoped.
  const trim = $derived(balanceTrim(balance));
  const stereo = $derived(
    filters.some((f) => f.channel.kind !== "both") || balance !== 0,
  );
  const leftPath = $derived(
    pathFor(
      responseCurve(
        filters.filter((f) => inChannel(f.channel, "left")),
        preamp + trim.left,
      ),
    ),
  );
  const rightPath = $derived(
    pathFor(
      responseCurve(
        filters.filter((f) => inChannel(f.channel, "right")),
        preamp + trim.right,
      ),
    ),
  );

  const gridF = [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000];
  const gridDb = $derived(dbGridLines(preamp, dbMax, 12));
  const labF = [
    { f: 100, t: "100" },
    { f: 1000, t: "1k" },
    { f: 10000, t: "10k" },
  ];
</script>

<svg viewBox="0 0 {W} {H}" class="curve">
  <rect x="0" y="0" width={W} height={H} class="bg" rx="8" />
  {#each gridF as f}
    <line x1={xOf(f)} y1={padT} x2={xOf(f)} y2={H - padB} class="grid" />
  {/each}
  {#each gridDb as db}
    <line x1={padL} y1={yOf(db)} x2={W - padR} y2={yOf(db)} class={db === 0 ? "axis" : "grid"} />
    <text x={padL - 4} y={yOf(db) + 3} class="lbl" text-anchor="end">{db > 0 ? "+" + db : db}</text>
  {/each}
  {#each labF as l}
    <text x={xOf(l.f)} y={H - 6} class="lbl" text-anchor="middle">{l.t}</text>
  {/each}

  {#if stereo}
    <path d={rightPath} class="resp right" />
    <path d={leftPath} class="resp left" />
    <line x1={padL + 6} y1={padT + 9} x2={padL + 18} y2={padT + 9} class="resp left" />
    <text x={padL + 21} y={padT + 12} class="lbl">L</text>
    <line x1={padL + 36} y1={padT + 9} x2={padL + 48} y2={padT + 9} class="resp right" />
    <text x={padL + 51} y={padT + 12} class="lbl">R</text>
  {:else}
    <path d={leftPath} class="resp left" />
  {/if}
</svg>

<style>
  .curve {
    width: 100%;
    height: auto;
    display: block;
  }
  .bg {
    fill: #181b21;
    stroke: var(--border);
  }
  .grid {
    stroke: #2a2f38;
    stroke-width: 1;
  }
  .axis {
    stroke: #3a4150;
    stroke-width: 1;
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
  .lbl {
    fill: var(--muted);
    font-size: 10px;
  }
</style>
