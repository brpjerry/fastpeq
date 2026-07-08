<script lang="ts">
  import { onDestroy } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import * as api from "./api";
  import type { Channel, Config, Line } from "./types";
  import ResponseCurve from "./ResponseCurve.svelte";
  import CurveEditor from "./CurveEditor.svelte";
  import ToneGenerator from "./ToneGenerator.svelte";
  import GraphTools from "./GraphTools.svelte";
  import FloatingMenu from "./FloatingMenu.svelte";
  import { anchorBelow, type Anchor } from "./floating";
  import { longDate, timeAgo } from "./time";
  import { createHistory, type Snapshot } from "./history.svelte";
  import PreampRow from "./PreampRow.svelte";
  import FilterList from "./FilterList.svelte";
  import { kindHasGain, kindHasQ, defaultQ, balanceTrim, toneFilters, peakGainDb, loudnessDb, parseConfigEq, bandInView, type BandView, type EngineFilter, type CurveFilter, type EditorBand } from "./eq";
  import { parseRew, normalize, downsample, type MeasPoint } from "./measurement";
  import { getFilterShapes, getToneHeadroom, getAutoPreamp, setAutoPreamp as saveAutoPreamp } from "./prefs.svelte";
  import { getTarget } from "./targets.svelte";
  import { createTrailingThrottle } from "./throttle";
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
  } from "./preset-view.svelte";
  import { alignOffset } from "./curve";

  let {
    name,
    reloadToken,
    onApplied,
    onSaved,
    tone = { bass: 0, mid: 0, treble: 0, invert: false, swap: false },
    bypassed = false,
    forceAutoPreamp = false,
    offloadActive = false,
    hardwareOnly = false,
    hwUserPregain = true,
    hwCommitToApply = false,
    hwCommitDelay = 500,
  }: {
    name: string;
    reloadToken: number;
    onApplied: (name: string) => void;
    /** A save landed (it may have minted a history revision — the parent
     * refreshes the preset list's version badges). */
    onSaved?: () => void;
    tone?: api.Tone;
    bypassed?: boolean;
    /** Hardware offload's Min. APO preamp mode forces Auto Preamp on (and locked). */
    forceAutoPreamp?: boolean;
    /** Whether EQ offload to a hardware device is currently on (drives the per-band
     * "→ hardware" indicator). */
    offloadActive?: boolean;
    /** Hardware Only offload is engaged (implies `offloadActive`): APO is flat, so
     * bands that don't fit on the device are muted — the curve, the clip check,
     * and the APO preamp all exclude them (and the inert tone overlay). */
    hardwareOnly?: boolean;
    /** Whether the offload device's pregain is host-adjustable. When it isn't
     * (the device headrooms itself), the Device preamp slider is hidden and the
     * device stage always shows the auto value. */
    hwUserPregain?: boolean;
    /** Whether the offload device only takes effect on a flash commit (the DHA15).
     * Its live RAM writes do nothing, so a commit on release latches the change. */
    hwCommitToApply?: boolean;
    /** How long (ms) to freeze edits after a flash commit — the device's audio drops
     * out while it applies. */
    hwCommitDelay?: number;
  } = $props();

  // Which band indices are currently sent to the hardware device. Queried from the
  // backend (the source of truth across all selection modes) and refreshed,
  // debounced, whenever the bands change while offload is on.
  let hwBandIdx = $state<Set<number>>(new Set());
  $effect(() => {
    // Track the band shape so a freq/gain/Q/type/enable change re-queries.
    const sig = JSON.stringify(
      bands.map((b) => [b.enabled, b.kind, b.freq, b.gain, b.q, b.channel.kind]),
    );
    void sig;
    if (!offloadActive) {
      hwBandIdx = new Set();
      return;
    }
    const cfg = buildConfig(false);
    let cancelled = false;
    const t = setTimeout(() => {
      api
        .offloadSelection(cfg)
        .then((idx) => {
          if (cancelled) return;
          const next = new Set(idx);
          // A fresh Set is built each query, so compare contents, not identity.
          // Bail when unchanged: reassigning a new reference re-runs everything
          // downstream, and because `apoPreamp` feeds the config this same effect
          // re-reads, that self-retriggers into a perpetual ~120 ms poll.
          if (next.size === hwBandIdx.size && [...next].every((i) => hwBandIdx.has(i))) {
            return;
          }
          hwBandIdx = next;
          // Re-assert the live config once the split is known so the split-aware APO
          // preamp / device pregain land. On a remount (e.g. returning from Settings)
          // `hwBandIdx` starts empty, so the earlier apply wrote the whole-preset
          // preamp onto APO instead of the split; this fires whenever offloading (not
          // just Auto) so the APO slider and config.txt can't diverge.
          reassertLive();
        })
        .catch(() => {});
    }, 120);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  });

  // The shared editable-band shape (see EditorBand in eq.ts).
  type Band = EditorBand;

  let bands = $state<Band[]>([]);
  let totalPreamp = $state(0); // master preamp — the single source of truth (see below)
  let balance = $state(0); // dB: <0 left louder, 0 centered, >0 right louder
  let hadPreamp = $state(false);
  let expanded = $state(false); // full-window graph + handle editing
  let view = $state<BandView>("both"); // which band list is shown
  let hoveredId = $state<number | null>(null); // graph handle under the cursor → row highlight
  let rawLines = $state<string[]>([]); // preserved verbatim (comments, Device:, etc.)
  let err = $state("");
  let loading = $state(true);
  let dirty = $state(false); // live changes not yet saved to the preset file
  let busy = $state(false);
  let nextId = 0;

  // Auto-preamp: when on, hold the preamp at the lowest value that keeps the
  // peak boost from clipping (the preamp slider is disabled, the EQ math drives
  // it). Uses the same bands + tone-overlay set as the clip warning, so with it
  // on the warning never fires. Persisted with the other UI prefs.
  const autoPreamp = $derived(getAutoPreamp());

  // Loudness-matching compare session flags (see the A/B compare section for
  // the machinery): armed on the first Compare entry, `matchOff` = the user's
  // per-session opt-out via the Auto switch.
  let matchArmed = $state(false);
  let matchOff = $state(false);

  // Auto Preamp is on by the user's toggle, because hardware offload's Min. APO
  // preamp mode forces it, or because a loudness-matching compare session is
  // running (both sides audition on their anti-clip preamp — unless the user
  // opted out via the switch). None of the forced states overwrite the user's
  // stored preference — they just override the effective behavior while active.
  const effectiveAuto = $derived(
    matchArmed ? forceAutoPreamp || !matchOff : forceAutoPreamp || autoPreamp,
  );

  // Auto-preamp value for a band set: the lowest (≤ 0) master preamp that keeps the
  // summed boost — those bands plus the global tone overlay — from clipping.
  // `bal` defaults to the working edit's balance; the compare matcher passes the
  // saved version's own.
  function computeAutoPreamp(
    forBands: CurveFilter[] = bands as CurveFilter[],
    bal: number = balance,
  ): number {
    const bandsPeak = peakGainDb(forBands, 0, bal);
    const combinedPeak = peakGainDb(
      [...forBands, ...toneFilters(tone.bass, tone.mid, tone.treble)] as CurveFilter[],
      0,
      bal,
    );
    const requiredPeak = Math.max(bandsPeak + getToneHeadroom(), combinedPeak);
    return Math.round(Math.min(0, -requiredPeak) * 10) / 10;
  }

  // Hybrid offload: some bands run on the device and the rest in APO, so rows
  // label their engine and the list offers APO-only / HW-only display filters.
  // (Hardware Only and no-offload each have a single engine — neither applies.)
  const hybrid = $derived(offloadActive && !hardwareOnly);
  // The engine display filter (the "APO only" / "HW only" buttons under the
  // list). Cleared when hybrid ends — its buttons disappear with it, and a stale
  // filter would silently hide bands.
  let engine = $state<EngineFilter>("all");
  $effect(() => {
    if (!hybrid) engine = "all";
  });

  // Hardware offload splits the preamp into two stages: APO (the software remainder)
  // and the device's pregain (the offloaded bands). `hwBandIdx` (from the backend)
  // says which bands run on the device.
  const hwBands = $derived(bands.filter((_, i) => hwBandIdx.has(i)) as CurveFilter[]);
  const softwareBands = $derived(bands.filter((_, i) => !hwBandIdx.has(i)) as CurveFilter[]);

  // Hardware Only: an enabled band the device didn't take is muted — nothing
  // runs it (APO is flat). Ids feed the graphs and the band list so the muted
  // bands read as inert everywhere.
  const mutedIds = $derived(
    hardwareOnly
      ? new Set(bands.filter((b, i) => b.enabled && !hwBandIdx.has(i)).map((b) => b.id))
      : new Set<number>(),
  );
  // What actually plays: with Hardware Only, muted bands drop out of the curve
  // and the clip math exactly as they drop out of the sound.
  const effectiveBands = $derived(
    hardwareOnly ? bands.map((b) => (mutedIds.has(b.id) ? { ...b, enabled: false } : b)) : bands,
  );

  const round1 = (v: number) => Math.round(v * 10) / 10;

  // ── Preamp: one source of truth (`totalPreamp`) ──────────────────────────────
  // `totalPreamp` is the preset's master preamp — the overall attenuation. Every
  // displayed/applied preamp derives from it: under offload it splits into the
  // device pregain (headroom for the offloaded boosts) + the APO software preamp
  // (the remainder). A manual slider edit folds back into `totalPreamp` so Save
  // round-trips it; the Auto toggle and offload just re-split the same value.
  //
  // The lowest (≤ 0) preamp each stage needs to avoid clipping: the device needs
  // `deviceMin` for its offloaded boosts; the APO stage needs `apoAntiClip` for the
  // software remainder (or the whole EQ off offload).
  function computeHwPregain(): number {
    return round1(Math.min(0, -Math.max(0, peakGainDb(hwBands, 0, balance))));
  }
  const deviceMin = $derived(computeHwPregain());
  const apoAntiClip = $derived(
    computeAutoPreamp((offloadActive ? softwareBands : bands) as CurveFilter[]),
  );

  // The device's share while honoring the total: its required headroom plus half of
  // any excess attenuation (the other half goes to APO — the two split spare headroom
  // evenly); a shortfall stays on the device so it can't clip. Overridden when the
  // user drags the Device slider (Auto off), then re-cleared when Auto turns off.
  let deviceManual = $state<number | null>(null);
  const deviceEvenSplit = $derived.by(() => {
    const extra = totalPreamp - (apoAntiClip + deviceMin); // < 0 → spare headroom to share
    return extra < 0 ? round1(deviceMin + extra / 2) : deviceMin;
  });
  const hwPregain = $derived.by(() => {
    if (!offloadActive) return 0;
    if (effectiveAuto || !hwUserPregain) return deviceMin; // auto minimizes to the headroom
    return deviceManual ?? deviceEvenSplit;
  });

  // The APO software preamp: the remainder (total − device), but never less
  // attenuation than the anti-clip value, so the software stage can't clip (that
  // clamp isn't written back to `totalPreamp`). Auto minimizes it; Hardware Only pins
  // it flat; off offload it's simply the master preamp.
  const apoPreamp = $derived.by(() => {
    if (hardwareOnly) return 0; // APO is flat; matching can't trim here (device-only)
    const base = !offloadActive
      ? effectiveAuto
        ? apoAntiClip
        : totalPreamp
      : effectiveAuto
        ? apoAntiClip
        : Math.min(round1(totalPreamp - hwPregain), apoAntiClip);
    // The compare matcher's extra attenuation on the working edit (0 when the
    // saved side is the louder one, or matching is off).
    return round1(base - (matchInfo?.aOffset ?? 0));
  });

  // The combined attenuation actually applied — both stages under offload, else just
  // the APO preamp. Drives the curve + clip check.
  const livePreamp = $derived(offloadActive ? apoPreamp + hwPregain : apoPreamp);
  // The device pregain to send with a live preview (`null` = no offload).
  const livePregain = $derived(offloadActive ? hwPregain : null);

  // Master preamp slider (off offload): sets the total directly.
  function setMasterPreamp(v: number) {
    totalPreamp = v;
    schedule();
  }
  // APO slider: pin the device where it is and hand the change to the total, so the
  // APO stage lands exactly on `v` (the two stages still sum to the total).
  function setApoPreamp(v: number) {
    const dev = hwPregain;
    deviceManual = dev;
    totalPreamp = round1(v + dev);
    schedule();
  }
  // Device slider: shift the total by the same amount so the APO stage holds steady,
  // and remember the override.
  function setDevicePreamp(v: number) {
    totalPreamp = round1(totalPreamp + (v - hwPregain));
    deviceManual = v;
    schedule();
  }
  // Auto off recomputes the split from the total: drop the device override so both
  // stages derive from `totalPreamp` again (with the even split).
  function setAutoPreamp(v: boolean) {
    // During a matching session the switch is the opt-out: off disables both
    // the forced Auto and the loudness offset (raw preamps on both sides), on
    // re-engages them. The user's stored preference is never touched.
    if (matchArmed) {
      matchOff = !v;
      if (comparing) {
        const cfg = matchOff ? savedConfig : auditionConfig();
        if (cfg) api.applyLive(cfg).catch((e) => (err = String(e)));
      } else {
        schedule();
      }
      return;
    }
    deviceManual = null;
    saveAutoPreamp(v);
    schedule();
  }

  // Possible clipping when the summed boost — the active bands plus the global
  // tone overlay, on whichever channel ends up louder — tops 0 dB. Past that the
  // signal can exceed full scale and Equalizer APO clips unless the preamp pulls
  // it back. Balance only attenuates, so it never raises this peak. In Hardware
  // Only mode the muted bands and the inert tone overlay are excluded — only
  // what the device actually runs can clip.
  const clipPeak = $derived(
    peakGainDb(
      hardwareOnly
        ? (effectiveBands as CurveFilter[])
        : ([...bands, ...toneFilters(tone.bass, tone.mid, tone.treble)] as CurveFilter[]),
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
    const off = alignOffset(effectiveBands as CurveFilter[], livePreamp, measurement, targetBase, getTargetAlignFreq(name));
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

  // Live-apply throttle: at most one write to config.txt per 75 ms while
  // dragging, with a guaranteed trailing write so the final value always lands.
  const applyThrottle = createTrailingThrottle(() => {
    api
      .applyLive(buildConfig(false), livePregain) // live preview -> config.txt only
      .then(() => {
        err = "";
        onApplied(name); // keep the active highlight on the preset being edited
      })
      .catch((e) => (err = String(e)));
  }, 75);

  // Device-side state (the offloaded bands + pregain) — tells whether a press
  // actually changed what the device runs.
  const deviceStateSig = $derived(
    JSON.stringify([livePregain, hwBands.map((b) => [b.kind, b.freq, b.gain, b.q])]),
  );

  // A commit-to-apply device (the DHA15) only latches EQ/pregain on a flash commit,
  // and its audio drops out while flashing — so its live RAM writes during a drag do
  // nothing, and we hold off and flash once, on mouse release. While the flash is in
  // flight `committing` grays the write controls for the device's commit delay so a
  // fresh edit can't pile onto the one being latched (or write over the dropout).
  let committing = $state(false);
  let commitLock: ReturnType<typeof setTimeout> | null = null;
  function flashDevice() {
    if (!hwCommitToApply || !offloadActive || loading || comparing) return;
    committing = true;
    api.applyLive(buildConfig(false), livePregain, true).catch((e) => (err = String(e)));
    if (commitLock) clearTimeout(commitLock);
    commitLock = setTimeout(() => (committing = false), hwCommitDelay);
  }

  // Flash on mouse release, and only when the press actually changed the device state
  // (a drag of the device pregain or an offloaded band). Capturing at pointerdown
  // means a plain click, a software-only edit, or a preset load never flashes — and
  // nothing fires while the button is held, so the controls never freeze mid-drag.
  let sigAtDown = "";
  $effect(() => {
    const down = () => (sigAtDown = deviceStateSig);
    const up = () => {
      if (deviceStateSig !== sigAtDown) flashDevice();
    };
    window.addEventListener("pointerdown", down);
    window.addEventListener("pointerup", up);
    return () => {
      window.removeEventListener("pointerdown", down);
      window.removeEventListener("pointerup", up);
    };
  });

  async function load(presetName: string) {
    err = "";
    loading = true;
    dirty = false;
    comparing = false; // a fresh preset is live; nothing to compare against yet
    matchArmed = false; // ...and any matching session ends with it
    matchOff = false;
    totalPreamp = 0;
    deviceManual = null;
    balance = 0;
    hadPreamp = false;
    try {
      const cfg = await api.getPreset(presetName);
      const parsed = parseConfigEq(cfg); // shared with the A/B-compare ghost
      totalPreamp = parsed.preamp; // the preset's master preamp is the total (req 2)
      balance = parsed.balance;
      hadPreamp = parsed.hadPreamp;
      bands = parsed.filters.map((f) => ({ ...f, id: nextId++ }));
      // A saved-but-unchanged restore leaves its version tag in the preset
      // file — pick it up so the association survives an app restart.
      const { tag, rest } = extractTag(parsed.raw);
      rawLines = rest;
      restoredTag = tag;
      tagBaseKey = tag ? eqKey() : "";
      savedConfig = cfg; // the loaded file is the "saved" baseline (B) to compare against
    } catch (e) {
      err = String(e);
      bands = [];
      rawLines = [];
      restoredTag = "";
      tagBaseKey = "";
      savedConfig = null;
    } finally {
      loading = false;
      resetHistory(); // start a fresh undo history at the loaded state

      // Push the live config so it reflects the derived preamp. Needed whenever the
      // applied preamp isn't the preset's raw master value: Auto (the anti-clip
      // value) or offload (the split — the preset's preamp lives on APO until we
      // re-assert the device/APO split, else the two desync). Skipped when the
      // load failed (bands/rawLines were just reset).
      if (!err) reassertLive();
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
  const hist = createHistory(restoreSnap, () => comparing);

  const canUndo = $derived(hist.canUndo);
  const canRedo = $derived(hist.canRedo);

  function snapState(): Snapshot {
    return {
      key: JSON.stringify({ bands, totalPreamp, balance }),
      bands: $state.snapshot(bands) as Band[],
      totalPreamp,
      balance,
    };
  }
  function resetHistory() {
    hist.reset(snapState());
  }
  function restoreSnap(s: Snapshot) {
    bands = s.bands.map((b) => ({ ...b })); // fresh copies so later edits don't touch history
    totalPreamp = s.totalPreamp;
    balance = s.balance;
    schedule();
  }
  function undo() {
    hist.undo(snapState());
  }
  function redo() {
    hist.redo(snapState());
  }

  // Append the current state as a new history entry if it differs from the top —
  // used both by the debounced recorder and eagerly before an undo/redo, so the
  // latest edit is always captured even if its coalesce window hasn't elapsed.
  // Restoring sets state back to an existing entry whose key then matches, so
  // undo/redo never record themselves.
  function flushHistory() {
    hist.flush(snapState());
  }

  // A burst of edits coalesces into one entry once it settles. JSON.stringify
  // reads every field, registering the effect's dependencies.
  $effect(() => {
    JSON.stringify({ bands, totalPreamp, balance });
    if (loading) return;
    const t = setTimeout(flushHistory, 400);
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

  // Loudness-matching session (state flags declared up top, near effectiveAuto):
  // louder reads as "better", so an unmatched A/B is biased. Entering Compare
  // arms the session — both sides audition on their anti-clip Auto preamp, and
  // the louder side (by A-weighted power mean of its response) is attenuated
  // by the difference. The session outlives individual A⇄B flips (the button)
  // and ends on Esc, Save, or a preset (re)load; the Auto switch turns red
  // showing the audible side's extra offset, and toggling it off opts out
  // (raw preamps) for the session.

  const canCompare = $derived(dirty && savedConfig !== null);
  // Graph-ready filters/preamp/balance of the saved version — parsed exactly
  // like load(), via the shared parseConfigEq.
  const savedCurve = $derived(savedConfig ? parseConfigEq(savedConfig) : null);

  // The matcher: each side's anti-clip preamp plus the extra attenuation the
  // louder side needs (attenuation-only, so matching can never clip).
  const matchInfo = $derived.by(() => {
    if (!matchArmed || matchOff || !savedCurve) return null;
    const aBands = effectiveBands as CurveFilter[];
    const autoA = computeAutoPreamp(aBands);
    const autoB = computeAutoPreamp(savedCurve.filters, savedCurve.balance);
    const la = loudnessDb(aBands, autoA);
    const lb = loudnessDb(savedCurve.filters, autoB);
    const x = round1(Math.abs(la - lb));
    return {
      aOffset: la > lb ? x : 0,
      bOffset: lb > la ? x : 0,
      bPreamp: round1(autoB - (lb > la ? x : 0)),
    };
  });
  // The extra offset on the side currently audible — the switch label's "−X dB".
  const matchOffset = $derived(
    matchInfo ? (comparing ? matchInfo.bOffset : matchInfo.aOffset) : 0,
  );

  // The faded ghost trace passed to the graphs — only while actually comparing,
  // at the preamp it is actually auditioned with.
  const compareRef = $derived(
    comparing && savedCurve
      ? matchInfo
        ? { ...savedCurve, preamp: matchInfo.bPreamp }
        : savedCurve
      : null,
  );

  // The saved version as auditioned: its master preamp replaced by the matched
  // anti-clip value (balance trims and everything else stay).
  function auditionConfig(): Config | null {
    if (!savedConfig || !matchInfo) return savedConfig;
    const lines: Line[] = savedConfig.lines.filter(
      (l) => !(l.kind === "Preamp" && l.value.channel.kind === "both"),
    );
    lines.unshift({
      kind: "Preamp",
      value: { gain: matchInfo.bPreamp, channel: { kind: "both" } },
    });
    return { lines };
  }

  function setCompare(on: boolean) {
    if (on === comparing || (on && !canCompare)) return;
    if (on && !matchArmed) {
      matchArmed = true; // first entry arms the matching session
      matchOff = false;
    }
    comparing = on;
    if (comparing) {
      const cfg = auditionConfig();
      if (cfg) api.applyLive(cfg).catch((e) => (err = String(e))); // hear the saved version
    } else {
      schedule(); // back to the working edit (still matched while armed)
    }
  }
  const toggleCompare = () => setCompare(!comparing);
  // Esc ends the whole session, not just the B audition.
  function exitCompare() {
    setCompare(false);
    matchArmed = false;
    matchOff = false;
  }

  // ── Version tags ────────────────────────────────────────────────────────────
  // A revision can carry a user-given name as a `# fastpeq:tag=` comment that
  // rides with its content. Restoring a tagged version brings the tag along
  // into the live config.txt; it stays with the content through an unchanged
  // save, and when a *changed* save displaces that content, the tag goes back
  // to the version's snapshot (the backend keeps it) and is scrubbed from the
  // live config. `tagBaseKey` is the content signature the tag describes, so
  // the editor knows whether the working state still IS the tagged version.
  const TAG_PREFIX = "# fastpeq:tag=";
  let restoredTag = $state("");
  let tagBaseKey = $state("");

  // Content identity for the tag (preamp excluded — it's derived, like the
  // history normal form).
  const eqKey = () =>
    JSON.stringify({
      b: bands.map((b) => [b.enabled, b.kind, b.freq, b.gain, b.q, b.channel.kind]),
      balance,
      raw: rawLines,
    });

  /** Split a parsed config's raw lines into its tag and the rest. */
  function extractTag(raw: string[]): { tag: string; rest: string[] } {
    const line = raw.find((l) => l.trim().startsWith(TAG_PREFIX));
    return {
      tag: line ? line.trim().slice(TAG_PREFIX.length).trim() : "",
      rest: raw.filter((l) => !l.trim().startsWith(TAG_PREFIX)),
    };
  }

  // ── History browser ─────────────────────────────────────────────────────────
  // Lists the preset's revisions. Rows are informational (version + creation
  // date); hearing an old version is what Restore is for — it live-loads into
  // the editor without writing anything, so there's no separate audition mode.
  let histOpen = $state(false);
  let histList = $state<api.Revision[]>([]);
  let histAnchor = $state<Anchor | null>(null);
  let histBtn = $state<HTMLButtonElement | null>(null);

  const OP_LABEL: Record<api.RevisionOp, string> = {
    save: "overwritten by save",
    delete: "deleted",
    restore: "overwritten by restore",
  };

  async function toggleHistory() {
    if (histOpen) {
      closeHistory();
      return;
    }
    try {
      histList = await api.presetHistory(name);
      if (histBtn) histAnchor = anchorBelow(histBtn);
      histOpen = true;
    } catch (e) {
      err = String(e);
    }
  }
  function closeHistory() {
    histOpen = false;
    editingTag = null;
  }

  // Inline tag editing (the pencil): Enter/blur commits, Esc cancels.
  let editingTag = $state<{ id: string; value: string; prior: string } | null>(null);

  function focusTagInput(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  async function commitTagEdit() {
    const edit = editingTag;
    editingTag = null;
    if (!edit || edit.value.trim() === edit.prior) return;
    try {
      await api.setRevisionTag(name, edit.id, edit.value.trim());
      histList = await api.presetHistory(name); // pick up the new tag
    } catch (e) {
      err = String(e);
    }
  }

  /** Load a revision into the editor as an UNSAVED edit: it plays live and
   *  lights Save, but only reaches the preset file when Save is clicked —
   *  restoring never writes by itself. (Undo-delete still uses the backend
   *  `restore_revision`, where there is no editor state to load into.) */
  async function restoreRevision(rev: api.Revision) {
    try {
      const config = await api.getRevision(name, rev.id);
      // End any compare session — we're replacing the edit it compared.
      comparing = false;
      matchArmed = false;
      matchOff = false;
      histOpen = false;
      const parsed = parseConfigEq(config);
      bands = parsed.filters.map((f) => ({ ...f, id: nextId++ }));
      // The version's tag rides along: it goes into the live config.txt with
      // this content (buildConfig adds the comment line).
      const { tag, rest } = extractTag(parsed.raw);
      rawLines = rest;
      balance = parsed.balance;
      hadPreamp = parsed.hadPreamp; // snapshots carry no master preamp
      // Recomputed anti-clip value, like a saved restore would get — the
      // snapshot has no preamp of its own.
      totalPreamp = computeAutoPreamp(parsed.filters as CurveFilter[], parsed.balance);
      deviceManual = null;
      restoredTag = tag;
      tagBaseKey = tag ? eqKey() : "";
      schedule(); // live + dirty — Save persists it (Ctrl+Z can take it back)
    } catch (e) {
      err = String(e);
    }
  }

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
    // The version tag rides with its content: always into the live config.txt;
    // into the preset file only while the content still IS the tagged version
    // (a changed save leaves the tag on the displaced snapshot instead — see
    // the version-tags section above).
    if (restoredTag && (!forSave || eqKey() === tagBaseKey)) {
      lines.push({ kind: "Raw", value: `${TAG_PREFIX}${restoredTag}` });
    }
    for (const r of rawLines) lines.push({ kind: "Raw", value: r });
    // Save writes the master preamp (the source of truth — req 6). A live preview
    // under offload writes only the APO-stage preamp; the device pregain rides
    // alongside via `apply_live`'s `pregain` argument.
    const p = forSave ? totalPreamp : offloadActive ? apoPreamp : livePreamp;
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

  // Throttled with a trailing call so the final position always gets written.
  function schedule() {
    if (loading || comparing) return; // no live edits while auditioning the saved version
    dirty = true;
    applyThrottle.schedule();
  }

  // Re-assert the live config outside the dirty/throttle path: direct (never
  // dirties the preset), and only when a derived stage owns part of the output
  // — Auto Preamp or an offload split — so config.txt tracks a recomputed
  // preamp/pregain the user didn't edit. Shared by the on-load apply, the
  // offload-selection refresh, and the tone-change re-apply, so their guards
  // can't drift apart.
  function reassertLive() {
    if (loading || comparing) return;
    if (!effectiveAuto && !offloadActive) return;
    api.applyLive(buildConfig(false), livePregain).catch((e) => (err = String(e)));
  }

  // Auto Preamp's master gain has to account for the global tone overlay, but the
  // tone is driven by the global controls (TonePanel / hotkeys) → `set_tone`,
  // which re-lays tone over the *existing* config and never runs the editor's
  // commit. So when tone shifts, the auto preamp already in config.txt goes stale
  // and the EQ can clip until the next band edit or an Auto toggle re-writes it.
  // Re-apply on a tone change so config.txt tracks it — directly (not via
  // `schedule`), so it doesn't dirty the preset, exactly like the on-load apply.
  // Only bass/mid/treble feed the preamp; invert/swap don't change the peak, and
  // the headroom setting can only change while the editor is unmounted (Settings).
  // Seeded on the first (loading-guarded) run, so the initial tone never counts
  // as a change and double-applies on top of the on-load apply above.
  let lastToneSig = "";
  $effect(() => {
    const sig = `${tone.bass},${tone.mid},${tone.treble}`;
    const changed = sig !== lastToneSig;
    lastToneSig = sig;
    if (changed) reassertLive();
  });

  function changeKind(band: Band) {
    if (kindHasQ(band.kind) && (!band.q || band.q <= 0)) band.q = defaultQ(band.kind);
    schedule();
  }

  // Filters are grouped into three lists — both / left / right — selected by the
  // view toggle, optionally narrowed by the engine display filter while a hybrid
  // offload is on. The graph tracks that same selection (see `graphBands`), so it
  // plots exactly the filters currently listed. Device membership comes from
  // `hwBandIdx` (the backend's selection) — never re-derived here.
  const inView = (b: Band, i: number, v: BandView) =>
    bandInView(b.channel, hwBandIdx.has(i), v, engine);
  const shown = $derived(bands.filter((b, i) => inView(b, i, view)));
  // The graphs mirror the on-screen list: only the bands passing the current
  // channel view + engine filter feed the traces, so the L / R tabs and the
  // APO-only / HW-only buttons each redraw the curve to match what's listed.
  // Uses `effectiveBands` so Hardware Only's muted bands stay excluded.
  const graphBands = $derived(effectiveBands.filter((b, i) => inView(b, i, view)));
  function channelForView(v: BandView): Channel {
    if (v === "left") return { kind: "left" };
    if (v === "right") return { kind: "right" };
    return { kind: "both" };
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
    // Sort only the bands in the current list, leaving the others put (their
    // positions — and thus any order-sensitive offload selection — unchanged).
    const sorted = [...shown].sort((a, b) => a.freq - b.freq);
    let j = 0;
    bands = bands.map((b, i) => (inView(b, i, view) ? sorted[j++] : b));
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
      matchArmed = false; // A and B are one again — the matching session is over
      matchOff = false;
      err = "";
      onSaved?.();
      if (restoredTag && eqKey() !== tagBaseKey) {
        // The content moved on: the tag stayed on the displaced version's
        // snapshot (the backend record keeps it) — scrub it from the live
        // config.txt too, so the new content doesn't wear an old name.
        restoredTag = "";
        tagBaseKey = "";
        api.applyLive(buildConfig(false), livePregain).catch((e) => (err = String(e)));
      }
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
    applyThrottle.cancel();
    if (commitLock) clearTimeout(commitLock);
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
    bind:this={histBtn}
    class="icon-btn hist-btn"
    class:on={histOpen}
    onclick={toggleHistory}
    title="History — preview and restore earlier versions"
    aria-label="Preset history"
  >
    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
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
    {#if hybrid}
      <!-- Engine display filter: narrow the list to one offload stage. Click the
           active button again to show everything. -->
      <div class="seg engine-seg" role="group" aria-label="Filter by engine">
        <button
          class:sel={engine === "apo"}
          onclick={() => (engine = engine === "apo" ? "all" : "apo")}
          title="Show only the bands running in Equalizer APO (click again to show all)"
        >
          APO only
        </button>
        <button
          class:sel={engine === "hw"}
          onclick={() => (engine = engine === "hw" ? "all" : "hw")}
          title="Show only the bands running on the hardware device (click again to show all)"
        >
          HW only
        </button>
      </div>
    {/if}
  </div>
{/snippet}

<!-- The preamp/balance row, band list, and band actions — identical in the
     inline panel and the expanded overlay's side column. -->
{#snippet eqControls()}
  <PreampRow
    autoPreamp={effectiveAuto}
    lockedAuto={forceAutoPreamp}
    bind:balance
    {livePreamp}
    offload={offloadActive}
    userPregain={hwUserPregain}
    {apoPreamp}
    {hwPregain}
    matchArmed={matchArmed && !matchOff}
    {matchOffset}
    onSetPreamp={setMasterPreamp}
    onSetApo={setApoPreamp}
    onSetDevice={setDevicePreamp}
    onSchedule={schedule}
    onAutoPreampChange={setAutoPreamp}
  />
  <FilterList
    bind:bands
    bind:view
    bind:hoveredId
    offloadedIdx={hwBandIdx}
    {mutedIds}
    {hybrid}
    {engine}
    onSchedule={schedule}
    onChangeKind={changeKind}
    onRemoveBand={removeBand}
  />
  {@render bandActions()}
{/snippet}

{#if !expanded}
  <section class="panel" class:comparing class:committing>
    <div class="panel-head">
      <h2 title={name}>{name}</h2>
      <div class="actions">
        {@render headActions()}
      </div>
    </div>

    {#if err}<div class="err">{err}</div>{/if}

    <div class="graph-wrap">
      <ResponseCurve filters={graphBands} preamp={livePreamp} {balance} {measurement} target={targetPoints} {compensate} {showMeas} reference={compareRef} />
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

    {@render eqControls()}
  </section>
{/if}

{#if expanded}
  <div class="overlay" class:comparing class:committing>
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
        {@render eqControls()}
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
            bands={shown}
            preamp={livePreamp}
            {balance}
            {view}
            {measurement}
            target={targetPoints}
            {compensate}
            {showMeas}
            {showTarget}
            {hoveredId}
            {mutedIds}
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

<!-- History browser: revisions newest first; click to audition (loudness-
     matched), Restore to make one current again. -->
<FloatingMenu
  class="hist-menu"
  open={histOpen}
  anchor={histAnchor}
  onDismiss={closeHistory}
  ignore={histBtn}
  zIndex={120}
  maxHeight="60vh"
>
  {#if !histList.length}
    <div class="hist-empty">
      No history yet — versions appear when a save replaces this preset.
    </div>
  {/if}
  {#each histList as rev, i (rev.id)}
    <!-- Versions count up from the oldest snapshot (v1); the list is newest
         first. What displaced the version, and how long ago, live in the
         tooltip; the label carries its creation date. Rows are informational —
         Restore is the (non-destructive) way to hear a version. -->
    <div class="hist-row" title="{OP_LABEL[rev.op]} · {timeAgo(rev.savedAtMs)}">
      {#if editingTag?.id === rev.id}
        <input
          class="hist-tag-input"
          maxlength="60"
          placeholder="Name this version"
          bind:value={editingTag.value}
          use:focusTagInput
          onblur={commitTagEdit}
          onkeydown={(e) => {
            if (e.key === "Enter") commitTagEdit();
            else if (e.key === "Escape") {
              e.stopPropagation(); // don't also collapse/exit anything behind us
              editingTag = null;
            }
          }}
        />
      {:else}
        <span class="hist-item">
          <span class="hist-ver">v{histList.length - i}</span>
          {#if rev.tag}<span class="hist-tag">{rev.tag}</span>{/if}
          <span class="hist-what">{longDate(rev.savedAtMs)}</span>
        </span>
        <button
          class="hist-tag-btn"
          onclick={() => (editingTag = { id: rev.id, value: rev.tag ?? "", prior: rev.tag ?? "" })}
          title="Name this version"
          aria-label="Name this version"
        >
          &#9998;
        </button>
        <button
          class="hist-restore"
          onclick={() => restoreRevision(rev)}
          disabled={busy}
          title="Load this version into the editor — nothing is written until you click Save (Ctrl+Z takes it back)"
        >
          Restore
        </button>
      {/if}
    </div>
  {/each}
</FloatingMenu>

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
     the graph handles can't be dragged — only the live output is swapped. The same
     lock applies briefly while a commit-to-apply device flashes (`committing`), so a
     fresh edit can't pile another write onto the one being latched. */
  .panel.comparing :global(.preamp),
  .panel.committing :global(.preamp),
  .panel.comparing :global(.bands),
  .panel.committing :global(.bands),
  .panel.comparing .band-actions,
  .panel.committing .band-actions,
  .overlay.comparing .overlay-side,
  .overlay.committing .overlay-side {
    opacity: 0.5;
    pointer-events: none;
  }
  .overlay.comparing .graph-fit,
  .overlay.committing .graph-fit {
    pointer-events: none;
  }
  .clip {
    font-size: 12px;
    font-weight: 600;
    color: #e0a458;
    white-space: nowrap;
    cursor: help;
  }

  .hist-btn.on {
    color: var(--accent);
  }
  /* History rows: audition target + a per-row Restore. Class is :global-safe —
     the menu portals through FloatingMenu, outside this component's tree. */
  :global(.hist-menu) .hist-empty {
    padding: 8px 12px;
    font-size: 12px;
    color: var(--muted);
    max-width: 240px;
  }
  :global(.hist-menu) .hist-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 4px;
    border-radius: 6px;
  }
  :global(.hist-menu) .hist-item {
    flex: 1;
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 5px 8px;
    white-space: nowrap;
  }
  :global(.hist-menu) .hist-ver {
    font-size: 12px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  :global(.hist-menu) .hist-what {
    font-size: 11px;
    color: var(--muted);
  }
  /* The user's name for a version, between vX and the date. */
  :global(.hist-menu) .hist-tag {
    font-size: 12px;
    color: var(--accent);
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  :global(.hist-menu) .hist-tag-btn {
    flex: none;
    padding: 2px 6px;
    font-size: 12px;
    line-height: 1;
    color: var(--muted);
  }
  :global(.hist-menu) .hist-tag-btn:hover {
    color: var(--text);
  }
  :global(.hist-menu) .hist-tag-input {
    flex: 1;
    min-width: 180px;
    margin: 2px 4px;
    padding: 3px 6px;
    font-size: 12px;
  }
  :global(.hist-menu) .hist-restore {
    flex: none;
    padding: 2px 8px;
    font-size: 11px;
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


  .band-actions {
    display: flex;
    /* Wrap whole buttons to a new line when the row can't fit; without this the
       shrinkable engine filter (overflow: hidden) collapses to nothing instead
       of dropping to the next line. */
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 8px;
  }
  /* Smaller than the default button, with single-line labels, so the row stays
     compact and the labels don't wrap at the narrowest window width. Buttons
     hold their natural width (flex: none) and the row wraps them as whole units. */
  .band-actions button {
    flex: none;
    font-size: 11px;
    padding: 5px 9px;
    white-space: nowrap;
  }
  .add {
    align-self: flex-start;
  }
  /* Engine display filter, bottom-right of the band pane (hybrid offload only). */
  .engine-seg {
    margin-left: auto;
    /* Never shrink: keep the segment (and its clipped inner buttons) at full
       width so it stays fully visible, wrapping to the next line if need be. */
    flex: none;
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: 7px;
    overflow: hidden;
  }
  .engine-seg button {
    border: none;
    border-right: 1px solid var(--border);
    border-radius: 0;
    background: transparent;
    /* Match the reduced band-action buttons; a hair less vertical padding since
       the segment sits inside its own 1px border. */
    padding: 3px 8px;
    font-size: 11px;
    color: var(--muted);
    white-space: nowrap;
  }
  .engine-seg button:last-child {
    border-right: none;
  }
  .engine-seg button:hover:not(.sel) {
    background: var(--panel-2);
    color: var(--text);
  }
  .engine-seg button.sel {
    background: var(--accent);
    color: #fff;
  }

  /* In the stacked layout the page scrolls, so the list shows all bands
     instead of opening a second internal scrollbar. */
</style>
