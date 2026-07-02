<script lang="ts">
  // Hardware EQ offload settings. A single 5-way control chooses the EQ routing:
  // "APO Only" (offload off) through the offload band-selection modes to "Hardware
  // Only". Offload follows the active output device — it engages only while the
  // current default output is a device fastpeq supports (e.g. the Moondrop DHA15);
  // any other output → no offload. Detected devices are listed for reference.
  import { onMount } from "svelte";
  import * as api from "./api";

  let { onChanged }: { onChanged?: () => void } = $props();

  let devices = $state<api.HardwareDevice[]>([]);
  let status = $state<api.HardwareStatus | null>(null);
  let busy = $state(false);
  let error = $state("");

  // Reconcile with the active output and refresh, on demand (no polling). Also
  // re-lists devices, since this is only called at moments a device may have
  // changed (open, refresh, return-to-window).
  async function load() {
    try {
      [devices, status] = await Promise.all([api.listHardwareDevices(), api.refreshHardware()]);
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  // Cheap status-only refresh (no device re-list) after our own actions.
  async function refreshStatus() {
    try {
      status = await api.refreshHardware();
    } catch (e) {
      error = String(e);
    }
  }

  const enabled = $derived(status?.enabled ?? false); // any mode other than APO Only
  const active = $derived(status?.active ?? false); // offload actually engaged
  const activeId = $derived(active ? (status?.device?.id ?? null) : null);
  const mode = $derived<api.OffloadMode>(status?.mode ?? "apo-only");
  const bandCount = $derived(status?.max_filters ?? devices[0]?.max_filters ?? 8);
  const MODES: { id: api.OffloadMode; label: string; desc: string }[] = $derived([
    { id: "apo-only", label: "APO Only", desc: "Every band stays in Equalizer APO — nothing is offloaded." },
    {
      id: "first-x",
      label: `First ${bandCount}`,
      desc: "The first bands in the preset, in order.",
    },
    {
      id: "largest-change",
      label: "Biggest effect",
      desc: "The bands that change the sound the most (largest area under the bell/shelf).",
    },
    {
      id: "minimize-preamp",
      label: "Min. APO Preamp",
      desc: "The boosts — so Equalizer APO's preamp stays near 0 and the device handles the headroom.",
    },
    {
      id: "hardware-only",
      label: "Hardware Only",
      desc: "Everything runs on the device where it fits (Equalizer APO stays flat).",
    },
  ]);
  const modeDesc = $derived(MODES.find((m) => m.id === mode)?.desc ?? "");

  async function changeMode(m: api.OffloadMode) {
    if (m === mode || busy) return;
    busy = true;
    try {
      await api.setOffloadMode(m);
      // Reconcile (opens/closes the device off the UI thread) and refresh status.
      await refreshStatus();
      onChanged?.();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    load();
    // Refresh when the user returns to the window — catches an output-device change
    // made outside fastpeq. No polling.
    const onFocus = () => refreshStatus();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  });
</script>

<section class="settings-section">
  <h3>Hardware EQ offload</h3>
  <p class="hint">
    Route a preset's EQ between Equalizer APO and a DAC/amp that does parametric EQ in hardware.
    Offload follows your <strong>active output device</strong> — it engages only while that output
    is a supported device.
  </p>

  {#if error}
    <p class="hw-error">{error}</p>
  {/if}

  <div class="seg">
    {#each MODES as m}
      <button
        class="seg-btn"
        class:sel={mode === m.id}
        disabled={busy}
        onclick={() => changeMode(m.id)}>{m.label}</button
      >
    {/each}
  </div>
  <p class="hint mode-desc">{modeDesc}</p>

  <p class="hint status-line">
    {#if !enabled}
      Off — every band goes to Equalizer APO.
    {:else if active && status?.device}
      Offloading to <strong>{status.device.name}</strong>{#if status.version}
        · firmware {status.version}{/if}.
    {:else}
      On, but the active output isn't a supported device — nothing is offloaded.
    {/if}
  </p>
  {#if enabled && status?.error}
    <p class="hw-error">Device error: {status.error}</p>
  {/if}

  {#if devices.length}
    <p class="hw-list-label">Supported devices detected</p>
    <ul class="hw-list">
      {#each devices as dev (dev.id)}
        <li class:target={activeId === dev.id}>
          <div class="hw-info">
            <span class="hw-name">{dev.name}</span>
            <span class="hw-sub">
              {#if activeId === dev.id}
                Active output · offloading
              {:else}
                runs up to {dev.max_filters} bands
              {/if}
            </span>
          </div>
          {#if activeId === dev.id}<span class="hw-chip">ON</span>{/if}
        </li>
      {/each}
    </ul>
  {:else}
    <p class="hint empty">No supported hardware EQ device detected.</p>
  {/if}

  <div class="settings-actions">
    <button onclick={load} disabled={busy}>Refresh</button>
  </div>
</section>

<style>
  .hw-list {
    list-style: none;
    margin: 0 0 12px;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .hw-list li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .hw-list li.target {
    border-color: var(--accent);
  }
  .hw-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .hw-name {
    font-size: 13px;
    font-weight: 600;
  }
  .hw-sub {
    font-size: 12px;
    color: var(--muted);
  }
  .hw-list-label,
  .status-line {
    margin: 0 0 8px;
  }
  .empty {
    margin: 0 0 12px;
  }
  .hw-error {
    margin: 0 0 12px;
    color: #f0a0a0;
    font-size: 13px;
  }
  .hw-chip {
    flex: none;
    padding: 1px 7px;
    border-radius: 5px;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.4px;
    background: var(--accent);
    color: #fff;
  }
  .seg {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .seg-btn.sel {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .seg-btn.sel:hover:not(:disabled) {
    background: var(--accent-2);
    border-color: var(--accent-2);
  }
  .mode-desc {
    margin: 8px 0;
    color: var(--muted);
    font-size: 12px;
  }
</style>
