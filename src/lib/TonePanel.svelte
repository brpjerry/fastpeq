<script lang="ts">
  import type { Tone, ApoStatus } from "./api";
  import { getToneStep } from "./prefs.svelte";
  import Knob from "./Knob.svelte";
  import Switch from "./Switch.svelte";

  let {
    tone = $bindable(),
    status,
    toneFlat,
    disabled = false,
    onPushTone,
    onResetTone,
  }: {
    tone: Tone;
    status: ApoStatus | null;
    toneFlat: boolean;
    /** Tone is inert (hardware-only offload keeps Equalizer APO flat, so the
     * overlay has nowhere to run). Values stay visible for when it returns. */
    disabled?: boolean;
    onPushTone: () => void;
    onResetTone: () => void;
  } = $props();

  const inert = $derived(disabled || !status?.installed);

  function setKnob(which: "bass" | "mid" | "treble", v: number) {
    if (inert) return;
    tone[which] = v;
    onPushTone();
  }
</script>

<section class="panel tone-panel">
  <div class="tone-head">
    <h2>Tone</h2>
    <span class="tone-sub">Global · layered over the active preset</span>
    <button class="tone-reset" onclick={onResetTone} disabled={toneFlat || inert}>
      Reset
    </button>
  </div>
  {#if disabled}
    <p class="tone-off-hint">Off in Hardware Only mode — Equalizer APO stays flat.</p>
  {/if}
  <div class="tone-body">
    <div class="knobs">
      <Knob label="Bass" value={tone.bass} step={getToneStep()} disabled={inert} onInput={(v) => setKnob("bass", v)} />
      <Knob label="Mids" value={tone.mid} step={getToneStep()} disabled={inert} onInput={(v) => setKnob("mid", v)} />
      <Knob label="Treble" value={tone.treble} step={getToneStep()} disabled={inert} onInput={(v) => setKnob("treble", v)} />
    </div>
    <div class="switches">
      <Switch
        label="Invert polarity"
        disabled={inert}
        checked={tone.invert}
        onChange={(v) => {
          tone.invert = v;
          onPushTone();
        }}
      />
      <Switch
        label="Switch L / R"
        disabled={inert}
        checked={tone.swap}
        onChange={(v) => {
          tone.swap = v;
          onPushTone();
        }}
      />
    </div>
  </div>
</section>

<style>
  .tone-panel {
    flex: none;
  }
  .tone-head {
    display: flex;
    align-items: baseline;
    gap: 10px;
    margin-bottom: 4px;
  }
  .tone-head h2 {
    margin: 0;
    font-size: 15px;
    color: var(--muted);
    font-weight: 600;
  }
  .tone-sub {
    flex: 1;
    color: var(--muted);
    font-size: 12px;
  }
  .tone-reset {
    padding: 3px 10px;
    font-size: 12px;
  }
  .tone-off-hint {
    margin: 0 0 6px;
    color: var(--muted);
    font-size: 12px;
  }
  .tone-body {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 44px;
    padding: 2px 0;
  }
  .knobs {
    display: flex;
    gap: 36px;
  }
  .switches {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  @media (min-width: 1080px) {
    .tone-panel {
      order: 1;
      width: 150px;
      padding: 12px 8px;
      overflow-y: auto;
    }
    .tone-head {
      justify-content: space-between;
    }
    .tone-sub {
      display: none;
    }
    .tone-body {
      flex-direction: column;
      align-items: stretch;
      gap: 18px;
    }
    .knobs {
      flex-direction: column;
      align-items: center;
      gap: 16px;
    }
    .switches {
      align-items: center;
      gap: 16px;
    }
    .switches :global(.switch) {
      flex-direction: column;
      gap: 6px;
      text-align: center;
    }
  }
</style>
