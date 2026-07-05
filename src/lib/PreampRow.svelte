<script lang="ts">
  import Switch from "./Switch.svelte";
  import { dismissable } from "./dismiss";

  let {
    manualPreamp = $bindable(),
    livePreamp,
    autoPreamp = $bindable(),
    lockedAuto = false,
    balance = $bindable(),
    offload = false,
    userPregain = true,
    apoPreamp = 0,
    hwPregain = 0,
    apoManual = $bindable(0),
    hwManual = $bindable(0),
    onSchedule,
    onAutoPreampChange,
  }: {
    manualPreamp: number;
    livePreamp: number;
    autoPreamp: boolean;
    /** When set, Auto Preamp is forced on and the toggle is disabled (e.g. by
     * hardware offload's Min. APO preamp mode). */
    lockedAuto?: boolean;
    balance: number;
    /** Hardware offload active → split the preamp into APO + device sliders. */
    offload?: boolean;
    /** Whether the offload device's pregain is host-adjustable; when it isn't
     * (the device headrooms itself), the Device row is hidden. */
    userPregain?: boolean;
    /** Effective APO-stage preamp / device pregain shown on the two sliders. */
    apoPreamp?: number;
    hwPregain?: number;
    /** Manual (Auto-off) values the two offload sliders write back. */
    apoManual?: number;
    hwManual?: number;
    onSchedule: () => void;
    onAutoPreampChange: (v: boolean) => void;
  } = $props();

  const BAL_MAX = 30;
  const PRE_MIN = -30;
  const PRE_MAX = 6;
  /* The device pregain is input headroom only — it may attenuate the offloaded
     bands but never boost, regardless of any headroom the device reserves. So
     its slider/field cap at 0, unlike the software preamp (which allows +boost). */
  const DEV_PRE_MAX = 0;
  let showBalance = $state(false);
  let chanBtn = $state<HTMLButtonElement | null>(null);

  /* Typed dB values land on the same manual state the sliders write. */
  function preampDb(v: string): number | null {
    const db = Number(v);
    if (v.trim() === "" || !Number.isFinite(db)) return null;
    return Math.max(PRE_MIN, Math.min(PRE_MAX, db));
  }
  function setApoDb(v: string) {
    const db = preampDb(v);
    if (db === null) return;
    apoManual = db;
    onSchedule();
  }
  function setManualDb(v: string) {
    const db = preampDb(v);
    if (db === null) return;
    manualPreamp = db;
    onSchedule();
  }
  function setDeviceDb(v: string) {
    const db = Number(v);
    if (v.trim() === "" || !Number.isFinite(db)) return;
    hwManual = Math.max(PRE_MIN, Math.min(DEV_PRE_MAX, db));
    onSchedule();
  }
  /** One-decimal display value (sums of rounded stages can carry float dust). */
  function r1(v: number): number {
    return Math.round(v * 10) / 10;
  }

  function setBalanceDb(v: string) {
    const db = Number(v);
    if (!Number.isFinite(db)) return;
    balance = Math.max(-BAL_MAX, Math.min(BAL_MAX, db));
    onSchedule();
  }
  function centerBalance() {
    balance = 0;
    onSchedule();
  }
  function balanceLabel(b: number): string {
    if (b === 0) return "Bal";
    const v = Math.abs(b);
    return (b > 0 ? "R" : "L") + (Number.isInteger(v) ? String(v) : v.toFixed(1));
  }
</script>

<div class="preamp-block">
  <div class="prows">
    <div class="preamp">
      {#if offload}
        <span class="plabel" title="Equalizer APO preamp — applied to the bands kept in software">APO</span>
        <input
          type="range"
          min="-30"
          max="6"
          step="0.1"
          value={apoPreamp}
          oninput={(e) => {
            apoManual = Number(e.currentTarget.value);
            onSchedule();
          }}
          disabled={autoPreamp}
        />
        <span class="pval">
          <input
            type="number"
            min={PRE_MIN}
            max={PRE_MAX}
            step="0.1"
            value={r1(apoPreamp)}
            disabled={autoPreamp}
            onchange={(e) => setApoDb(e.currentTarget.value)}
          />
          <small>dB</small>
        </span>
      {:else}
        <span class="plabel">Preamp</span>
        <input
          type="range"
          min="-30"
          max="6"
          step="0.1"
          value={livePreamp}
          oninput={(e) => {
            manualPreamp = Number(e.currentTarget.value);
            onSchedule();
          }}
          disabled={autoPreamp}
        />
        <span class="pval">
          <input
            type="number"
            min={PRE_MIN}
            max={PRE_MAX}
            step="0.1"
            value={r1(livePreamp)}
            disabled={autoPreamp}
            onchange={(e) => setManualDb(e.currentTarget.value)}
          />
          <small>dB</small>
        </span>
      {/if}
    </div>
    {#if offload && userPregain}
      <div class="preamp device">
        <span
          class="plabel"
          title="Hardware device pregain — headroom for the offloaded bands (attenuation only, never boosts)">Device</span
        >
        <input
          type="range"
          min={PRE_MIN}
          max={DEV_PRE_MAX}
          step="0.1"
          value={hwPregain}
          oninput={(e) => {
            hwManual = Number(e.currentTarget.value);
            onSchedule();
          }}
          disabled={autoPreamp}
        />
        <span class="pval">
          <input
            type="number"
            min={PRE_MIN}
            max={DEV_PRE_MAX}
            step="0.1"
            value={r1(hwPregain)}
            disabled={autoPreamp}
            onchange={(e) => setDeviceDb(e.currentTarget.value)}
          />
          <small>dB</small>
        </span>
      </div>
    {/if}
  </div>
  <div class="pside">
    <Switch
      compact
      label="Auto"
      checked={autoPreamp}
      disabled={lockedAuto}
      onChange={onAutoPreampChange}
      title={lockedAuto
        ? "Auto Preamp is managed by hardware offload"
        : "Automatically set the preamp so the EQ never clips"}
    />
    <div class="balance-wrap">
      <button
        bind:this={chanBtn}
        class="chan"
        class:on={balance !== 0}
        onclick={() => (showBalance = !showBalance)}
        title="Channel balance">{balanceLabel(balance)}</button
      >
      {#if showBalance}
        <div class="bal-pop" use:dismissable={{ onDismiss: () => (showBalance = false), ignore: chanBtn }}>
          <div class="bal-slider">
            <small>L</small>
            <input
              type="range"
              min={-BAL_MAX}
              max={BAL_MAX}
              step="0.5"
              bind:value={balance}
              oninput={onSchedule}
              oncontextmenu={centerBalance}
              title="Right-click to reset to center"
            />
            <small>R</small>
          </div>
          <div class="bal-foot">
            <label class="bal-input" title="Balance: + right louder, − left louder">
              <input
                type="number"
                min={-BAL_MAX}
                max={BAL_MAX}
                step="0.5"
                value={balance}
                onchange={(e) => setBalanceDb(e.currentTarget.value)}
              />
              <small>dB</small>
            </label>
            <span class="bal-hint">+R&nbsp;/&nbsp;−L</span>
            <button class="bal-center" onclick={centerBalance} disabled={balance === 0}>Center</button>
          </div>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  /* Slider rows in one column, with the Auto/balance controls beside them —
     centered on the column, i.e. on the midline between the two offload rows. */
  .preamp-block {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 8px 0 6px;
  }
  .prows {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .preamp {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .preamp input[type="range"] {
    flex: 1;
  }
  .pside {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .plabel {
    color: var(--muted);
    width: 54px;
    font-size: 12px;
  }
  /* Editable dB value, same idiom as a band row's gain field. */
  .pval {
    display: flex;
    align-items: center;
    gap: 3px;
    color: var(--muted);
    font-size: 11px;
  }
  .pval input[type="number"] {
    width: 46px; /* e.g. -12.3 */
    flex: none;
    /* Match a band row's gain/freq field height (BandRow's `.band input`), so the
       preamp fields don't tower over the filter list below them. */
    padding: 2px 5px;
    font-size: 12px;
  }
  .pval small {
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .chan {
    flex: none;
    min-width: 36px;
    padding: 2px 5px;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    border-radius: 6px;
  }
  .chan.on {
    border-color: var(--accent);
    color: var(--accent);
  }

  .balance-wrap {
    position: relative;
    flex: none;
  }
  .bal-pop {
    position: absolute;
    right: 0;
    top: calc(100% + 6px);
    z-index: 51;
    width: 232px;
    padding: 10px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.45);
  }
  .bal-slider {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .bal-slider input[type="range"] {
    flex: 1;
  }
  .bal-slider small {
    color: var(--muted);
    font-size: 11px;
    width: 10px;
    text-align: center;
  }
  .bal-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-top: 8px;
  }
  .bal-input {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: var(--muted);
    font-size: 11px;
  }
  .bal-input input {
    width: 48px;
    padding: 2px 5px;
    font-size: 12px;
  }
  .bal-hint {
    font-size: 11px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .bal-center {
    padding: 2px 8px;
    font-size: 11px;
  }
</style>
