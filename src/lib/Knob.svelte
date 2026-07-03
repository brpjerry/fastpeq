<script lang="ts">
  // A circular tone knob. Drag up/down (or scroll) to change the value;
  // double-click resets to 0. The arc fills from the 12-o'clock centre toward
  // the current value, so boost (right) and cut (left) read at a glance.
  let {
    value,
    min = -12,
    max = 12,
    step = 0.5,
    label,
    unit = "dB",
    disabled = false,
    onInput,
  }: {
    value: number;
    min?: number;
    max?: number;
    step?: number;
    label: string;
    unit?: string;
    /** Inert: still shows its value, but ignores all input (dimmed). */
    disabled?: boolean;
    onInput: (v: number) => void;
  } = $props();

  const SWEEP = 135; // degrees of travel each side of the top
  const CX = 40;
  const CY = 40;
  const R = 28;

  let dragging = $state(false);
  let startY = 0;
  let startVal = 0;

  const clamp = (v: number) => Math.max(min, Math.min(max, v));
  const snap = (v: number) => Math.round(v / step) * step;
  const angleOf = (v: number) => ((v - min) / (max - min)) * (SWEEP * 2) - SWEEP;

  // Point on the dial at `deg` (0 = straight up, clockwise positive).
  function pt(r: number, deg: number) {
    const a = (deg * Math.PI) / 180;
    return { x: CX + r * Math.sin(a), y: CY - r * Math.cos(a) };
  }
  function arc(r: number, a0: number, a1: number) {
    const p0 = pt(r, a0);
    const p1 = pt(r, a1);
    const large = Math.abs(a1 - a0) > 180 ? 1 : 0;
    const sweep = a1 >= a0 ? 1 : 0;
    return `M ${p0.x.toFixed(2)} ${p0.y.toFixed(2)} A ${r} ${r} 0 ${large} ${sweep} ${p1.x.toFixed(2)} ${p1.y.toFixed(2)}`;
  }

  const a = $derived(angleOf(value));
  const needle = $derived(pt(R - 4, a));
  const display = $derived((value > 0 ? "+" : "") + value.toFixed(1));

  function commit(v: number) {
    if (disabled) return;
    const nv = clamp(snap(v));
    if (nv !== value) onInput(nv);
  }
  function onDown(e: PointerEvent) {
    e.preventDefault();
    if (disabled) return;
    dragging = true;
    startY = e.clientY;
    startVal = value;
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
  }
  function onMove(e: PointerEvent) {
    if (!dragging) return;
    const dy = startY - e.clientY; // drag up = increase
    commit(startVal + (dy / 150) * (max - min));
  }
  function onUp(e: PointerEvent) {
    dragging = false;
    (e.currentTarget as Element).releasePointerCapture(e.pointerId);
  }
  function onWheel(e: WheelEvent) {
    e.preventDefault();
    commit(value + (e.deltaY < 0 ? step : -step));
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === "ArrowUp" || e.key === "ArrowRight") {
      e.preventDefault();
      commit(value + step);
    } else if (e.key === "ArrowDown" || e.key === "ArrowLeft") {
      e.preventDefault();
      commit(value - step);
    }
  }
</script>

<div class="knob" class:disabled>
  <svg
    viewBox="0 0 80 80"
    class="dial"
    class:dragging
    role="slider"
    tabindex={disabled ? -1 : 0}
    aria-label={label}
    aria-valuenow={value}
    aria-valuemin={min}
    aria-valuemax={max}
    aria-disabled={disabled}
    onpointerdown={onDown}
    onpointermove={onMove}
    onpointerup={onUp}
    onwheel={onWheel}
    onkeydown={onKey}
    ondblclick={() => commit(0)}
    oncontextmenu={(e) => {
      e.preventDefault();
      commit(0);
    }}
  >
    <path d={arc(R, -SWEEP, SWEEP)} class="track" />
    {#if value !== 0}
      <path d={arc(R, 0, a)} class="fill" />
    {/if}
    <circle cx={CX} cy={CY} r={R - 8} class="body" />
    <line x1={CX} y1={CY} x2={needle.x} y2={needle.y} class="needle" />
  </svg>
  <div class="name">{label}</div>
  <div class="val">{display} <small>{unit}</small></div>
</div>

<style>
  .knob {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    user-select: none;
  }
  .dial {
    width: 64px;
    height: 64px;
    display: block;
    cursor: ns-resize;
    touch-action: none;
    outline: none;
  }
  .knob.disabled {
    opacity: 0.45;
  }
  .knob.disabled .dial {
    cursor: default;
  }
  .dial:focus-visible {
    border-radius: 50%;
    box-shadow: 0 0 0 2px var(--accent);
  }
  .track {
    fill: none;
    stroke: var(--border);
    stroke-width: 5;
    stroke-linecap: round;
  }
  .fill {
    fill: none;
    stroke: var(--accent);
    stroke-width: 5;
    stroke-linecap: round;
  }
  .body {
    fill: var(--panel-2);
    stroke: var(--border);
    stroke-width: 1;
  }
  .dial.dragging .body {
    fill: #2b3038;
  }
  .needle {
    stroke: var(--accent);
    stroke-width: 2.5;
    stroke-linecap: round;
  }
  .name {
    font-size: 12px;
    color: var(--muted);
    margin-top: 2px;
  }
  .val {
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .val small {
    color: var(--muted);
  }
</style>
