<script lang="ts">
  // Sliding toggle matching the tone-control switches, with a compact variant
  // for dense control rows.
  let {
    checked,
    onChange,
    label = "",
    compact = false,
    disabled = false,
    title = "",
  }: {
    checked: boolean;
    onChange: (value: boolean) => void;
    label?: string;
    compact?: boolean;
    disabled?: boolean;
    title?: string;
  } = $props();
</script>

<label class="switch" class:compact {title}>
  <input type="checkbox" {checked} {disabled} onchange={(e) => onChange(e.currentTarget.checked)} />
  <span class="track"><span class="thumb"></span></span>
  {#if label}<span class="sw-label">{label}</span>{/if}
</label>

<style>
  .switch {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    user-select: none;
    font-size: 13px;
    color: var(--muted);
  }
  .switch input {
    position: absolute;
    opacity: 0;
    width: 0;
    height: 0;
  }
  .track {
    position: relative;
    flex: none;
    width: 36px;
    height: 20px;
    border-radius: 10px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    transition:
      background 0.15s ease,
      border-color 0.15s ease;
  }
  .thumb {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--muted);
    transition:
      transform 0.15s ease,
      background 0.15s ease;
  }
  .switch input:checked + .track {
    background: var(--accent);
    border-color: var(--accent);
  }
  .switch input:checked + .track .thumb {
    transform: translateX(16px);
    background: #fff;
  }
  .switch input:focus-visible + .track {
    box-shadow: 0 0 0 2px var(--accent);
  }
  .switch input:disabled + .track {
    opacity: 0.5;
  }
  .switch:has(input:disabled) {
    cursor: not-allowed;
    color: var(--muted);
    opacity: 0.7;
  }

  /* Compact: smaller track/thumb for tight rows like the curve-editor tools. */
  .switch.compact {
    gap: 6px;
    font-size: 12px;
  }
  .switch.compact .track {
    width: 28px;
    height: 16px;
    border-radius: 8px;
  }
  .switch.compact .thumb {
    width: 11px;
    height: 11px;
  }
  .switch.compact input:checked + .track .thumb {
    transform: translateX(12px);
  }
</style>
