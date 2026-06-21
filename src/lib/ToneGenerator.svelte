<script lang="ts">
  // A sine-wave test-tone generator for the curve editor. Uses the Web Audio
  // API: an oscillator → gain → output, started/stopped by the play button and
  // updated live as the sliders move. Volume is capped (configurable in
  // settings) to avoid an accidental blast.
  import { onDestroy } from "svelte";
  import { getToneVolumeCap } from "./prefs.svelte";

  const F_LO = 20;
  const F_HI = 20000;

  const cap = $derived(getToneVolumeCap());
  let playing = $state(false);
  // Frequency is driven by a 0..1 log position so the slider feels even.
  let pos = $state(Math.log(1000 / F_LO) / Math.log(F_HI / F_LO));
  let volume = $state(0.1); // 10% by default
  const freq = $derived(Math.round(F_LO * Math.pow(F_HI / F_LO, pos)));
  const effVol = $derived(Math.min(volume, cap)); // honour the cap for audio + readout

  let ctx: AudioContext | null = null;
  let osc: OscillatorNode | null = null;
  let gain: GainNode | null = null;

  function start() {
    if (!ctx) {
      ctx = new AudioContext();
      gain = ctx.createGain();
      gain.connect(ctx.destination);
    }
    void ctx.resume(); // clear the autoplay-suspended state (we're in a click)
    gain!.gain.value = effVol;
    osc = ctx.createOscillator();
    osc.type = "sine";
    osc.frequency.value = freq;
    osc.connect(gain!);
    osc.start();
    playing = true;
  }
  function stop() {
    if (osc) {
      try {
        osc.stop();
      } catch {
        /* already stopped */
      }
      osc.disconnect();
      osc = null;
    }
    playing = false;
  }
  function toggle() {
    if (playing) stop();
    else start();
  }

  // Track the sliders into the live tone (smoothed to avoid clicks).
  $effect(() => {
    const f = freq;
    if (osc && ctx) osc.frequency.setTargetAtTime(f, ctx.currentTime, 0.02);
  });
  $effect(() => {
    const v = effVol;
    if (gain && ctx) gain.gain.setTargetAtTime(v, ctx.currentTime, 0.01);
  });

  onDestroy(() => {
    stop();
    void ctx?.close();
  });

  const freqText = (f: number) =>
    f >= 1000 ? (f / 1000).toFixed(f % 1000 === 0 ? 0 : 2) + " kHz" : Math.round(f) + " Hz";
</script>

<div class="gen">
  <button class="play" class:on={playing} onclick={toggle} title={playing ? "Stop tone" : "Play sine tone"}>
    {playing ? "■" : "▶"}
  </button>
  <span class="lbl">Tone</span>
  <input class="freq" type="range" min="0" max="1" step="0.001" bind:value={pos} />
  <span class="val freq-val">{freqText(freq)}</span>
  <span class="sep"></span>
  <span class="lbl">Vol</span>
  <input class="vol" type="range" min="0" max={cap} step="0.01" bind:value={volume} />
  <span class="val vol-val">{Math.round(effVol * 100)}%</span>
</div>

<style>
  .gen {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--muted);
  }
  .play {
    flex: none;
    width: 28px;
    height: 22px;
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    line-height: 1;
  }
  .play.on {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .play.on:hover:not(:disabled) {
    background: var(--accent-2);
    border-color: var(--accent-2);
  }
  .lbl {
    flex: none;
  }
  .freq {
    flex: 1;
    min-width: 120px;
  }
  .vol {
    flex: none;
    width: 96px;
  }
  .val {
    flex: none;
    text-align: right;
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .freq-val {
    width: 60px;
  }
  .vol-val {
    width: 34px;
  }
  .sep {
    flex: none;
    width: 1px;
    height: 18px;
    background: var(--border);
    margin: 0 2px;
  }
</style>
