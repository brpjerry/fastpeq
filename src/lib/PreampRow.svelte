<script lang="ts">
  import Switch from "./Switch.svelte";
  import { dismissable } from "./dismiss";

  let {
    manualPreamp = $bindable(),
    livePreamp,
    autoPreamp = $bindable(),
    balance = $bindable(),
    onSchedule,
    onAutoPreampChange,
  }: {
    manualPreamp: number;
    livePreamp: number;
    autoPreamp: boolean;
    balance: number;
    onSchedule: () => void;
    onAutoPreampChange: (v: boolean) => void;
  } = $props();

  const BAL_MAX = 30;
  let showBalance = $state(false);
  let chanBtn = $state<HTMLButtonElement | null>(null);

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

<div class="preamp">
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
  <span class="pval">{livePreamp.toFixed(1)} dB</span>
  <Switch
    compact
    label="Auto"
    checked={autoPreamp}
    onChange={onAutoPreampChange}
    title="Automatically set the preamp so the EQ never clips"
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

<style>
  .preamp {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 8px 0 6px;
  }
  .preamp input[type="range"] {
    flex: 1;
  }
  .plabel {
    color: var(--muted);
    width: 54px;
    font-size: 12px;
  }
  .pval {
    width: 60px;
    text-align: right;
    font-variant-numeric: tabular-nums;
    font-size: 12px;
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
