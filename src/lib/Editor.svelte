<script lang="ts">
  import { onDestroy } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import * as api from "./api";
  import type { Channel, Config, FilterKind, Line } from "./types";
  import ResponseCurve from "./ResponseCurve.svelte";
  import CurveEditor from "./CurveEditor.svelte";
  import ToneGenerator from "./ToneGenerator.svelte";
  import Switch from "./Switch.svelte";
  import GraphTools from "./GraphTools.svelte";
  import BandRow from "./BandRow.svelte";
  import { kindHasGain, kindHasQ, defaultQ, balanceTrim, balanceFromTrim, toneFilters, peakGainDb, type CurveFilter } from "./eq";
  import { parseRew, normalize, downsample, type MeasPoint } from "./measurement";
  import { dismissable } from "./dismiss";
  import { getFilterShapes } from "./prefs.svelte";
  import { getTarget } from "./targets.svelte";
  import {
    getTargetId,
    getCompensate,
    getShowMeasRef,
    setShowMeasRef,
    getShowTargetRef,
    getMeasurement,
    setMeasurement,
    clearMeasurement as clearSavedMeasurement,
    getTargetOffset,
    setTargetOffset,
    getTargetAlignFreq,
  } from "./presetView.svelte";
  import { alignOffset } from "./curve";
  import { getToneHeadroom } from "./prefs.svelte";

  let {
    name,
    reloadToken,
    onApplied,
    tone = { bass: 0, mid: 0, treble: 0, invert: false, swap: false },
    bypassed = false,
  }: {
    name: string;
    reloadToken: number;
    onApplied: (name: string) => void;
    tone?: api.Tone;
    bypassed?: boolean;
  } = $props();

  // Editable band: gain/q kept as plain numbers; nulled out per-type on save.
  type Band = {
    id: number;
    enabled: boolean;
    kind: FilterKind;
    freq: number;
    gain: number;
    q: number;
    channel: Channel;
  };

  let bands = $state<Band[]>([]);
  let manualPreamp = $state(0);
  let balance = $state(0); // dB: <0 left louder, 0 centered, >0 right louder
  let hadPreamp = $state(false);
  let showBalance = $state(false);
  let chanBtn = $state<HTMLButtonElement | null>(null);
  let expanded = $state(false); // full-window graph + handle editing
  let view = $state<"both" | "left" | "right">("both"); // which channel list is shown
  let hoveredId = $state<number | null>(null); // graph handle under the cursor → row highlight
  const BAL_MAX = 30; // balance range, dB of cut on the quieter side at full
  let rawLines = $state<string[]>([]); // preserved verbatim (comments, Device:, etc.)
  let err = $state("");
  let loading = $state(true);
  let dirty = $state(false); // live changes not yet saved to the preset file
  let busy = $state(false);
  let nextId = 0;

  // Auto-preamp: when on, hold the preamp at the lowest value that keeps the
  // peak boost from clipping (the preamp slider is disabled, the EQ math drives
  // it). Uses the same bands + tone-overlay set as the clip warning, so with it
  // on the warning never fires.
  let autoPreamp = $state(
    typeof localStorage !== "undefined" ? localStorage.getItem("fastpeq:autoPreamp") === "true" : false,
  );
  $effect(() => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem("fastpeq:autoPreamp", String(autoPreamp));
    }
  });

  function computeAutoPreamp(): number {
    const bandsPeak = peakGainDb(bands as CurveFilter[], 0, balance);
    const combinedPeak = peakGainDb(
      [...bands, ...toneFilters(tone.bass, tone.mid, tone.treble)] as CurveFilter[],
      0,
      balance,
    );
    const requiredPeak = Math.max(bandsPeak + getToneHeadroom(), combinedPeak);
    return Math.round(Math.min(0, -requiredPeak) * 10) / 10;
  }

  const livePreamp = $derived(autoPreamp ? computeAutoPreamp() : manualPreamp);

  // Possible clipping when the summed boost — the active bands plus the global
  // tone overlay, on whichever channel ends up louder — tops 0 dB. Past that the
  // signal can exceed full scale and Equalizer APO clips unless the preamp pulls
  // it back. Balance only attenuates, so it never raises this peak.
  const clipPeak = $derived(
    peakGainDb(
      [...bands, ...toneFilters(tone.bass, tone.mid, tone.treble)] as CurveFilter[],
      livePreamp,
      balance,
    ),
  );
  const clipping = $derived(clipPeak > 0.05);

  // The per-preset target curve (Flat by default), shown on the graph as a
  // reference. Reactive to the selected target and the current preset. A manual
  // dB offset (set directly or by the "Align" action) shifts the whole trace; it
  // bakes into the points so the gap readout and compensation pick it up too.
  const targetBase = $derived(getTarget(getTargetId(name)).points);
  const targetOffset = $derived(getTargetOffset(name));
  const targetPoints = $derived(
    targetOffset && targetBase.length
      ? targetBase.map((p) => ({ freq: p.freq, spl: p.spl + targetOffset }))
      : targetBase,
  );

  // Shift the target so its line meets the current response at the saved align
  // frequency — the standard "align at a reference frequency" for headphone EQ.
  function alignTarget() {
    const off = alignOffset(bands as CurveFilter[], livePreamp, measurement, targetBase, getTargetAlignFreq(name));
    setTargetOffset(name, Math.round(off * 10) / 10);
  }

  // Imported FR measurement, saved per preset and auto-loaded whenever this
  // preset is shown again. The traces become "measurement + filters".
  const savedMeas = $derived(getMeasurement(name));
  const measurement = $derived<MeasPoint[]>(savedMeas?.points ?? []);
  const measName = $derived(savedMeas?.name ?? "");

  // Independent per-preset visibility of the dashed reference lines.
  const showMeas = $derived(getShowMeasRef(name));
  const showTarget = $derived(getShowTargetRef(name));
  // Effective compensation: only meaningful with the target shown and a
  // non-flat target selected (otherwise there's nothing to compensate against).
  const canCompensate = $derived(showTarget && targetPoints.length > 0);
  const compensate = $derived(getCompensate(name) && canCompensate);

  // Live-apply throttle: at most one write to config.txt per THROTTLE ms while
  // dragging, with a guaranteed trailing write so the final value always lands.
  const THROTTLE = 75;
  let lastApply = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  async function load(presetName: string) {
    err = "";
    loading = true;
    dirty = false;
    comparing = false; // a fresh preset is live; nothing to compare against yet
    manualPreamp = 0;
    balance = 0;
    hadPreamp = false;
    const nextBands: Band[] = [];
    const raw: string[] = [];
    try {
      const cfg = await api.getPreset(presetName);
      for (const line of cfg.lines) {
        if (line.kind === "Preamp") {
          // A both-channel preamp is the master gain; a one-sided preamp is a
          // balance trim, folded back into the balance slider.
          const ch = line.value.channel;
          if (ch.kind === "left" || ch.kind === "right") {
            balance = balanceFromTrim(ch.kind, line.value.gain);
          } else {
            manualPreamp = line.value.gain;
            hadPreamp = true;
          }
        } else if (line.kind === "Filter") {
          const f = line.value;
          nextBands.push({
            id: nextId++,
            enabled: f.enabled,
            kind: f.kind,
            freq: f.freq,
            gain: f.gain ?? 0,
            q: f.q ?? defaultQ(f.kind),
            channel: f.channel,
          });
        } else {
          raw.push(line.value);
        }
      }
      bands = nextBands;
      rawLines = raw;
      savedConfig = cfg; // the loaded file is the "saved" baseline (B) to compare against
    } catch (e) {
      err = String(e);
      bands = [];
      rawLines = [];
      savedConfig = null;
    } finally {
      loading = false;
      resetHistory(); // start a fresh undo history at the loaded state
      
      // If Auto Preamp is globally enabled, pushing it to the live config doesn't dirty the preset
      if (autoPreamp && !comparing) {
        api.applyLive(buildConfig(false)).catch((e) => (err = String(e)));
      }
    }
  }

  // Reload when the preset changes, or when the parent bumps reloadToken (e.g.
  // re-clicking the active preset to revert). Loading is programmatic, so it
  // never triggers the change handlers below (no accidental apply).
  $effect(() => {
    void reloadToken;
    load(name);
  });

  // ── Undo / redo ────────────────────────────────────────────────────────────
  // History of the editable state (bands + manualPreamp + balance). A burst of edits —
  // e.g. a slider drag — coalesces into one entry once it settles, so a single
  // undo steps back a whole gesture rather than one pixel of movement.
  type Snapshot = { key: string; bands: Band[]; manualPreamp: number; balance: number };
  let history = $state<Snapshot[]>([]);
  let histIndex = $state(-1);
  const HIST_MAX = 100;
  const HIST_COALESCE = 400;

  const canUndo = $derived(histIndex > 0);
  const canRedo = $derived(histIndex < history.length - 1);

  function snapState(): Snapshot {
    return {
      key: JSON.stringify({ bands, manualPreamp, balance }),
      bands: $state.snapshot(bands) as Band[],
      manualPreamp,
      balance,
    };
  }
  function resetHistory() {
    history = [snapState()];
    histIndex = 0;
  }
  function restoreSnap(s: Snapshot) {
    bands = s.bands.map((b) => ({ ...b })); // fresh copies so later edits don't touch history
    manualPreamp = s.manualPreamp;
    balance = s.balance;
    schedule();
  }
  function undo() {
    if (comparing) return; // editing is locked while comparing
    flushHistory(); // capture an in-flight edit so it's undoable right away
    if (histIndex <= 0) return;
    restoreSnap(history[--histIndex]);
  }
  function redo() {
    if (comparing) return;
    flushHistory();
    if (histIndex >= history.length - 1) return;
    restoreSnap(history[++histIndex]);
  }

  // Append the current state as a new history entry if it differs from the top —
  // used both by the debounced recorder and eagerly before an undo/redo, so the
  // latest edit is always captured even if its coalesce window hasn't elapsed.
  // Restoring sets state back to an existing entry whose key then matches, so
  // undo/redo never record themselves.
  function flushHistory() {
    const snap = snapState();
    if (history[histIndex]?.key === snap.key) return;
    history = [...history.slice(0, histIndex + 1), snap];
    if (history.length > HIST_MAX) history = history.slice(history.length - HIST_MAX);
    histIndex = history.length - 1;
  }

  // A burst of edits coalesces into one entry once it settles. JSON.stringify
  // reads every field, registering the effect's dependencies.
  $effect(() => {
    JSON.stringify({ bands, manualPreamp, balance });
    if (loading) return;
    const t = setTimeout(flushHistory, HIST_COALESCE);
    return () => clearTimeout(t);
  });

  // Ctrl+Z / Ctrl+Y (or Ctrl+Shift+Z). Skipped only while a real text field is
  // focused (the preset search / rename boxes) so their native text undo still
  // works; the editor's own number/range controls fall through to editor undo.
  function isTextEntry(el: Element | null): boolean {
    if (el instanceof HTMLTextAreaElement) return true;
    if (el instanceof HTMLInputElement) {
      return ["text", "search", "email", "url", "tel", "password"].includes(el.type);
    }
    return false;
  }
  // ── A/B compare ────────────────────────────────────────────────────────────
  // Hold the last-saved version (B) so the live output can flip between it and
  // the working edit (A) to hear the difference; editing is locked while on B.
  let savedConfig = $state<Config | null>(null);
  let comparing = $state(false);

  const canCompare = $derived(dirty && savedConfig !== null);
  const savedCurve = $derived(savedConfig ? configToCurve(savedConfig) : null);
  // The faded ghost trace passed to the graphs — only while actually comparing.
  const compareRef = $derived(comparing ? savedCurve : null);

  // A Config as graph-ready filters/preamp/balance (mirrors the parse in load()).
  function configToCurve(cfg: Config): { filters: CurveFilter[]; preamp: number; balance: number } {
    let p = 0;
    let bal = 0;
    const filters: CurveFilter[] = [];
    for (const line of cfg.lines) {
      if (line.kind === "Preamp") {
        const ch = line.value.channel;
        if (ch.kind === "left" || ch.kind === "right") bal = balanceFromTrim(ch.kind, line.value.gain);
        else p = line.value.gain;
      } else if (line.kind === "Filter") {
        const f = line.value;
        filters.push({
          enabled: f.enabled,
          kind: f.kind,
          freq: f.freq,
          gain: f.gain ?? 0,
          q: f.q ?? defaultQ(f.kind),
          channel: f.channel,
        });
      }
    }
    return { filters, preamp: p, balance: bal };
  }

  function setCompare(on: boolean) {
    if (on === comparing || (on && !canCompare)) return;
    comparing = on;
    if (comparing && savedConfig) {
      api.applyLive(savedConfig).catch((e) => (err = String(e))); // hear the saved version
    } else {
      schedule(); // back to the working edit
    }
  }
  const toggleCompare = () => setCompare(!comparing);
  const exitCompare = () => setCompare(false);

  // Ctrl+Z / Ctrl+Y undo-redo and Ctrl+` to toggle compare, skipped while a real
  // text field is focused so their native behaviour still works. (Esc is handled
  // on <svelte:window> alongside collapse.)
  $effect(() => {
    function onKey(e: KeyboardEvent) {
      if (!(e.ctrlKey || e.metaKey)) return;
      if (isTextEntry(document.activeElement)) return;
      const k = e.key.toLowerCase();
      if (k === "`") {
        e.preventDefault();
        toggleCompare();
      } else if (k === "z" && !e.shiftKey) {
        e.preventDefault();
        undo();
      } else if (k === "y" || (k === "z" && e.shiftKey)) {
        e.preventDefault();
        redo();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  function buildConfig(forSave = false): Config {
    const lines: Line[] = [];
    for (const r of rawLines) lines.push({ kind: "Raw", value: r });
    const p = (forSave && autoPreamp) ? manualPreamp : livePreamp;
    if (hadPreamp || p !== 0) {
      lines.push({ kind: "Preamp", value: { gain: p, channel: { kind: "both" } } });
    }
    // Balance is a one-sided preamp trim on the attenuated channel.
    if (balance !== 0) {
      const trim = balanceTrim(balance);
      if (balance > 0) {
        lines.push({ kind: "Preamp", value: { gain: trim.left, channel: { kind: "left" } } });
      } else {
        lines.push({ kind: "Preamp", value: { gain: trim.right, channel: { kind: "right" } } });
      }
    }
    bands.forEach((b, i) => {
      lines.push({
        kind: "Filter",
        value: {
          enabled: b.enabled,
          kind: b.kind,
          freq: b.freq,
          gain: kindHasGain(b.kind) ? b.gain : null,
          q: kindHasQ(b.kind) ? b.q : null,
          index: i + 1,
          channel: b.channel,
        },
      });
    });
    return { lines };
  }

  async function commit() {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    lastApply = Date.now();
    try {
      await api.applyLive(buildConfig(false)); // live preview -> config.txt only
      err = "";
      onApplied(name); // keep the active highlight on the preset being edited
    } catch (e) {
      err = String(e);
    }
  }

  // Throttle with a trailing call so the final position always gets written.
  function schedule() {
    if (loading || comparing) return; // no live edits while auditioning the saved version
    dirty = true;
    const elapsed = Date.now() - lastApply;
    if (timer !== null) clearTimeout(timer);
    if (elapsed >= THROTTLE) {
      commit();
    } else {
      timer = setTimeout(commit, THROTTLE - elapsed);
    }
  }

  function changeKind(band: Band) {
    if (kindHasQ(band.kind) && (!band.q || band.q <= 0)) band.q = defaultQ(band.kind);
    schedule();
  }

  // Filters are grouped into three lists — both / left / right — selected by the
  // view toggle. The graph always uses the full set, so it shows the real
  // per-channel response no matter which list is on screen.
  function inView(c: Channel, v: "both" | "left" | "right"): boolean {
    if (v === "left") return c.kind === "left";
    if (v === "right") return c.kind === "right";
    return c.kind === "both" || c.kind === "other"; // unmodeled specs ride along here
  }
  const shown = $derived(bands.filter((b) => inView(b.channel, view)));
  const counts = $derived({
    both: bands.filter((b) => inView(b.channel, "both")).length,
    left: bands.filter((b) => b.channel.kind === "left").length,
    right: bands.filter((b) => b.channel.kind === "right").length,
  });
  function channelForView(v: "both" | "left" | "right"): Channel {
    if (v === "left") return { kind: "left" };
    if (v === "right") return { kind: "right" };
    return { kind: "both" };
  }
  function emptyMsg(v: "both" | "left" | "right"): string {
    if (v === "left") return "No left-only filters yet.";
    if (v === "right") return "No right-only filters yet.";
    return "No filters yet — add a band to start shaping the curve.";
  }

  // Manual entry in dB: + = right louder, − = left louder.
  function setBalanceDb(v: string) {
    const db = Number(v);
    if (!Number.isFinite(db)) return;
    balance = Math.max(-BAL_MAX, Math.min(BAL_MAX, db));
    schedule();
  }
  function centerBalance() {
    balance = 0;
    schedule();
  }
  function balanceLabel(b: number): string {
    if (b === 0) return "Bal";
    const v = Math.abs(b);
    return (b > 0 ? "R" : "L") + (Number.isInteger(v) ? String(v) : v.toFixed(1));
  }

  function addBand() {
    // New bands join the list currently in view, taking that channel.
    bands.push({
      id: nextId++,
      enabled: true,
      kind: "Peak",
      freq: 1000,
      gain: 0,
      q: 1,
      channel: channelForView(view),
    });
    schedule();
  }

  function removeBand(id: number) {
    bands = bands.filter((b) => b.id !== id);
    schedule();
  }

  // Gain filters left at 0 dB do nothing; this clears them out across all
  // channels in one go (a common tidy-up after dialing in a curve).
  const isFlat = (b: Band) => kindHasGain(b.kind) && b.gain === 0;
  const flatCount = $derived(bands.filter(isFlat).length);
  function removeZeroGain() {
    bands = bands.filter((b) => !isFlat(b));
    schedule();
  }

  function sortBands() {
    // Sort only the bands in the current list, leaving the other channels put.
    const sorted = [...shown].sort((a, b) => a.freq - b.freq);
    let i = 0;
    bands = bands.map((b) => (inView(b.channel, view) ? sorted[i++] : b));
    schedule();
  }

  // Persist the current state to the preset file (separate from the live config).
  async function save() {
    busy = true;
    try {
      const config = buildConfig(true);
      await api.savePreset(name, config);
      savedConfig = config; // the new baseline for A/B compare
      dirty = false;
      err = "";
    } catch (e) {
      err = String(e);
    } finally {
      busy = false;
    }
  }

  function collapse() {
    expanded = false;
    hoveredId = null;
  }

  // Import a REW measurement to overlay as a reference; the filter traces then
  // show "measurement + filters" (the corrected response).
  async function importMeasurement() {
    try {
      const picked = await openDialog({
        multiple: false,
        title: "Import REW measurement",
        filters: [{ name: "Measurement (text)", extensions: ["txt"] }],
      });
      if (!picked || Array.isArray(picked)) return;
      const points = downsample(normalize(parseRew(await api.readTextFile(picked))));
      if (!points.length) {
        err = "No measurement data found in that file.";
        return;
      }
      setMeasurement(name, { name: picked.split(/[\\/]/).pop() ?? "measurement", points });
      setShowMeasRef(name, true); // a fresh import is shown by default
      err = "";
    } catch (e) {
      err = String(e);
    }
  }
  function clearMeasurement() {
    clearSavedMeasurement(name);
  }

  onDestroy(() => {
    if (timer !== null) clearTimeout(timer);
  });
</script>

<svelte:window
  onkeydown={(e) => {
    if (e.key !== "Escape") return;
    if (comparing) exitCompare();
    else if (expanded) collapse();
  }}
/>

{#snippet headActions()}
  <span
    class="live"
    class:error={!!err}
    class:comparing={comparing && !err}
    class:bypassed={bypassed && !err && !comparing}
    title={comparing
      ? "Hearing the saved version — toggle Compare off to return to your edit"
      : bypassed
        ? "Filters are bypassed — preamp kept, EQ off"
        : "Changes apply to Equalizer APO instantly"}
  >
    {err ? "● error" : comparing ? "● saved" : bypassed ? "● bypassed" : "● live"}
  </span>
  {#if clipping}
    <span
      class="clip"
      title={`Possible clipping — combined boost peaks at +${clipPeak.toFixed(1)} dB. Lower the preamp by ~${clipPeak.toFixed(1)} dB (or cut a band) to stay under 0 dB.`}
    >
      ▲ clip
    </span>
  {/if}
  <button class="icon-btn undo-btn" onclick={undo} disabled={!canUndo || comparing} title="Undo (Ctrl+Z)" aria-label="Undo">
    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M9 14L4 9l5-5" />
      <path d="M4 9h11a5 5 0 0 1 0 10h-1" />
    </svg>
  </button>
  <button class="icon-btn redo-btn" onclick={redo} disabled={!canRedo || comparing} title="Redo (Ctrl+Y)" aria-label="Redo">
    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M15 14l5-5-5-5" />
      <path d="M20 9H9a5 5 0 0 0 0 10h1" />
    </svg>
  </button>
  <button
    class="compare-btn"
    class:on={comparing}
    onclick={toggleCompare}
    disabled={!canCompare}
    title={canCompare
      ? "Compare with the saved version (Ctrl+`)"
      : "No unsaved changes to compare against"}
  >
    {comparing ? "Comparing saved" : "Compare"}
  </button>
  <button class="primary" onclick={save} disabled={!dirty || busy || comparing} title="Write changes to the preset file">
    {dirty ? "Save" : "Saved"}
  </button>
{/snippet}

{#snippet preampRow()}
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
        schedule();
      }}
      disabled={autoPreamp}
    />
    <span class="pval">{livePreamp.toFixed(1)} dB</span>
    <Switch
      compact
      label="Auto"
      checked={autoPreamp}
      onChange={(v) => {
        autoPreamp = v;
        if (!comparing) api.applyLive(buildConfig(false)).catch((e) => (err = String(e)));
      }}
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
              oninput={schedule}
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
{/snippet}

{#snippet bandsHead()}
  <div class="bands-head">
    <div class="seg view-seg" role="group" aria-label="Channel filter list">
      <button class:sel={view === "both"} onclick={() => (view = "both")} title="Filters applied to both channels">
        L+R{counts.both ? ` · ${counts.both}` : ""}
      </button>
      <button class:sel={view === "left"} onclick={() => (view = "left")} title="Left-channel-only filters">
        L{counts.left ? ` · ${counts.left}` : ""}
      </button>
      <button class:sel={view === "right"} onclick={() => (view = "right")} title="Right-channel-only filters">
        R{counts.right ? ` · ${counts.right}` : ""}
      </button>
    </div>
  </div>
{/snippet}

{#snippet bandsBody()}
  {#each bands as band, i (band.id)}
    {#if inView(band.channel, view)}
      <BandRow
        bind:band={bands[i]}
        hovered={band.id === hoveredId}
        onChange={schedule}
        onChangeKind={() => changeKind(band)}
        onRemove={() => removeBand(band.id)}
        onHover={(h) => (hoveredId = h ? band.id : null)}
      />
    {/if}
  {/each}
  {#if shown.length === 0}
    <p class="none">{emptyMsg(view)}</p>
  {/if}
{/snippet}

{#snippet bandActions()}
  <div class="band-actions">
    <button class="add" onclick={addBand}>+ Add band</button>
    <button onclick={sortBands} disabled={shown.length < 2}>Sort by Hz</button>
    <button
      class="clear-flat"
      onclick={removeZeroGain}
      disabled={flatCount === 0}
      title="Remove every gain filter sitting at 0 dB (they have no effect)"
    >
      Remove 0 dB{flatCount ? ` · ${flatCount}` : ""}
    </button>
  </div>
{/snippet}

{#if !expanded}
  <section class="panel" class:comparing>
    <div class="panel-head">
      <h2 title={name}>{name}</h2>
      <div class="actions">
        {@render headActions()}
      </div>
    </div>

    {#if err}<div class="err">{err}</div>{/if}

    <div class="graph-wrap">
      <ResponseCurve filters={bands} preamp={livePreamp} {balance} {measurement} target={targetPoints} {compensate} {showMeas} reference={compareRef} />
      <button
        class="icon-btn expand-btn"
        onclick={() => (expanded = true)}
        title="Expand graph"
        aria-label="Expand graph"
      >
        <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" />
        </svg>
      </button>
    </div>

    {@render preampRow()}

    {@render bandsHead()}
    <div class="bands">
      {@render bandsBody()}
    </div>

    {@render bandActions()}
  </section>
{/if}

{#if expanded}
  <div class="overlay" class:comparing>
    <div class="overlay-head">
      <h2 title={name}>{name}</h2>
      <div class="actions">
        {@render headActions()}
        <button
          class="icon-btn"
          onclick={collapse}
          title="Collapse (Esc)"
          aria-label="Collapse graph"
        >
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5" />
          </svg>
        </button>
      </div>
    </div>

    {#if err}<div class="err">{err}</div>{/if}

    <div class="overlay-body">
      <aside class="overlay-side">
        {@render preampRow()}
        {@render bandsHead()}
        <div class="bands">
          {@render bandsBody()}
        </div>
        {@render bandActions()}
      </aside>
      <div class="overlay-graph">
        <GraphTools
          {name}
          {compensate}
          {canCompensate}
          {measurement}
          {measName}
          onImport={importMeasurement}
          onClear={clearMeasurement}
          onAlign={alignTarget}
        />
        <div class="graph-fit">
          <CurveEditor
            {bands}
            preamp={livePreamp}
            {balance}
            {view}
            {measurement}
            target={targetPoints}
            {compensate}
            {showMeas}
            {showTarget}
            {hoveredId}
            filterShapes={getFilterShapes()}
            reference={compareRef}
            onChange={schedule}
            onHover={(id) => (hoveredId = id)}
          />
        </div>
        <ToneGenerator />
      </div>
    </div>
  </div>
{/if}

<style>
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .live {
    font-size: 12px;
    color: #5bb85f;
    white-space: nowrap;
  }
  .live.error {
    color: var(--danger);
  }
  .live.bypassed {
    color: var(--muted);
  }
  .live.comparing {
    color: var(--accent);
  }
  /* A/B compare toggle; reads as "armed" (accent fill) while comparing. */
  .compare-btn {
    font-size: 12px;
    padding: 3px 10px;
    white-space: nowrap;
  }
  .compare-btn.on {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .compare-btn.on:hover:not(:disabled) {
    background: var(--accent-2);
    border-color: var(--accent-2);
  }
  /* While comparing, the EQ controls are locked (dimmed, non-interactive) and
     the graph handles can't be dragged — only the live output is swapped. */
  .panel.comparing .preamp,
  .panel.comparing .bands,
  .panel.comparing .band-actions,
  .overlay.comparing .overlay-side {
    opacity: 0.5;
    pointer-events: none;
  }
  .overlay.comparing .graph-fit {
    pointer-events: none;
  }
  .clip {
    font-size: 12px;
    font-weight: 600;
    color: #e0a458;
    white-space: nowrap;
    cursor: help;
  }

  /* Square icon button (expand / collapse). */
  .icon-btn {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 5px;
    line-height: 0;
    color: var(--muted);
  }
  .icon-btn:hover:not(:disabled) {
    color: var(--text);
  }

  /* The inline graph, with the expand button floated in its corner. */
  .graph-wrap {
    position: relative;
  }
  .expand-btn {
    position: absolute;
    top: 8px;
    right: 8px;
    background: rgba(20, 23, 29, 0.7);
    backdrop-filter: blur(2px);
  }

  /* Full-window expanded view: band list on the left, big graph on the right. */
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 100;
    background: var(--bg);
    display: flex;
    flex-direction: column;
    padding: 12px 16px 16px;
    gap: 10px;
  }
  .overlay-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
  }
  .overlay-head h2 {
    margin: 0;
    font-size: 16px;
    color: var(--text);
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .overlay-body {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: 14px;
  }
  .overlay-side {
    flex: none;
    width: 430px;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  /* The dense band row is tuned for the wider inline panel; let the gain slider
     give up more space so it still fits the narrower side column. The row lives
     in BandRow now, so :global reaches its .field.gain through the boundary. */
  .overlay-side :global(.field.gain) {
    min-width: 56px;
  }
  .overlay-side :global(.field.gain input[type="range"]) {
    min-width: 38px;
  }
  .overlay-graph {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  /* Holds the big graph at a fixed 8:5 aspect ratio: size containment lets the
     graph itself scale to the largest 8:5 box that fits this area (via cqw/cqh
     in CurveEditor), so it grows/shrinks with the pane but never overflows. */
  .graph-fit {
    flex: 1;
    min-height: 0;
    container-type: size;
    display: grid;
    place-items: center;
  }

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

  /* Balance popover anchored under the preamp's balance button. */
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

  /* Channel-list toggle above the band list. */
  .bands-head {
    display: flex;
    align-items: center;
    margin-bottom: 6px;
  }
  .view-seg {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: 7px;
    overflow: hidden;
  }
  .view-seg button {
    border: none;
    border-right: 1px solid var(--border);
    border-radius: 0;
    background: transparent;
    padding: 4px 12px;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .view-seg button:last-child {
    border-right: none;
  }
  .view-seg button:hover:not(.sel) {
    background: var(--panel-2);
    color: var(--text);
  }
  .view-seg button.sel {
    background: var(--accent);
    color: #fff;
  }

  /* Dense, table-like list: one bordered container, thin row separators, and an
     internal scroll so the curve and controls stay put. */
  .bands {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .none {
    color: var(--muted);
    padding: 12px 6px;
  }
  .band-actions {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
  .add {
    align-self: flex-start;
  }

  /* In the stacked layout the page scrolls, so the list shows all bands
     instead of opening a second internal scrollbar. */
  @media (max-width: 820px) {
    .bands {
      flex: none;
      overflow: visible;
    }
  }
</style>
