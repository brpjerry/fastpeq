<script lang="ts">
  import { onMount, tick } from "svelte";
  import { listen, emit } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import * as api from "./lib/api";
  import Editor from "./lib/Editor.svelte";
  import Settings from "./lib/Settings.svelte";
  import HotkeysPage from "./lib/HotkeysPage.svelte";
  import TonePanel from "./lib/TonePanel.svelte";
  import PresetsPanel, { scrollCurrentIntoView } from "./lib/PresetsPanel.svelte";
  import { starterConfig } from "./lib/starter";
  import { addTarget, initTargets } from "./lib/targets.svelte";
  import { renamePresetView, clearPresetView, initPresetView } from "./lib/preset-view.svelte";
  import { parseRew, normalize, downsample } from "./lib/measurement";
  import { getToneStep, defaultBandCount, initPrefs } from "./lib/prefs.svelte";
  import { initTheme } from "./lib/theme";
  import { createDebounce, createTrailingThrottle } from "./lib/throttle";
  import { getHotkeys, accelerators, initHotkeys } from "./lib/hotkeys.svelte";
  import { OSD_EVENT, payloadForHotkey } from "./lib/osd";

  let status = $state<api.ApoStatus | null>(null);
  let presets = $state<string[]>([]);
  let categories = $state<Record<string, string>>({});
  let active = $state<string | null>(null);
  let selected = $state<string | null>(null);
  let message = $state("");
  let busy = $state(false);
  let showSettings = $state(false);
  let showHotkeys = $state(false);
  let failedHotkeys = $state<string[]>([]); // binding ids that couldn't register
  let devices = $state<api.AudioDevice[]>([]); // audio outputs for the "switch device" hotkey
  let offload = $state<api.HardwareStatus | null>(null); // hardware EQ offload state
  let windowFocused = $state(true); // gates the OSD overlay: only show feedback when unfocused
  let isBypassed = $state(false);
  let bandCount = $state(defaultBandCount());
  let presetsDirPath = $state("");
  let refreshing = $state(false);
  let editorReloadToken = $state(0);

  // Global tone overlay (bass/mid/treble), layered over the active preset by the
  // backend. Writes are throttled while a knob is being dragged.
  let tone = $state<api.Tone>({ bass: 0, mid: 0, treble: 0, invert: false, swap: false });
  const toneFlat = $derived(
    tone.bass === 0 && tone.mid === 0 && tone.treble === 0 && !tone.invert && !tone.swap,
  );
  // Hardware Only: everything runs on the device and Equalizer APO stays flat —
  // tone is inert while it's engaged. Keyed on `active` (offload actually
  // engaged): on any other output the EQ (and tone) runs in software as usual.
  const hardwareOnly = $derived(!!offload?.active && offload.mode === "hardware-only");
  // The editor's Auto Preamp is forced on while offload manages the preamp
  // stage(s): Min. APO preamp (APO carries no headroom — the device's pregain
  // does) and Hardware Only (APO carries nothing at all).
  const forceAutoPreamp = $derived(
    !!offload?.active && (offload.mode === "minimize-preamp" || offload.mode === "hardware-only"),
  );
  // Startup offload reconcile: the active preset resolves immediately from its
  // provenance stamp (so the preset and its filters show right away), but the
  // ~1 s HID enumeration to (re)connect the device runs in the background. Show a
  // non-blocking "connecting" hint until the backend reports it finished
  // (`reconciled`); `fastpeq:changed` → reload() then clears it. Latches once at
  // startup — `reconciled` never flips back to false.
  const connectingHardware = $derived(!!offload?.enabled && !offload.reconciled);
  const toneThrottle = createTrailingThrottle(() => {
    api.setTone(tone).catch((e) => flash(String(e)));
  }, 80);
  function pushTone() {
    if (!status?.installed || hardwareOnly) return;
    toneThrottle.schedule();
  }
  function setKnob(which: "bass" | "mid" | "treble", v: number) {
    tone[which] = v;
    pushTone();
  }
  function resetTone() {
    toneThrottle.cancel();
    tone = { bass: 0, mid: 0, treble: 0, invert: false, swap: false };
    if (status?.installed) toneThrottle.flush();
  }
  const clampTone = (v: number) => Math.max(-12, Math.min(12, v)); // matches the tone Knob range

  // A global hotkey fired (emitted from the backend): run its bound action. Stale
  // preset references (deleted/renamed) just no-op.
  function dispatchHotkey(id: string) {
    const h = getHotkeys().find((x) => x.id === id);
    if (!h) return;
    // Tone is inert while Hardware Only offload is engaged (APO stays flat) —
    // a tone hotkey flashes why instead of silently changing a knob that
    // wouldn't be applied.
    const toneAction = h.action === "tone-up" || h.action === "tone-down" || h.action === "tone-reset";
    if (toneAction && hardwareOnly) {
      flash("Tone is off in Hardware Only mode");
      return;
    }
    if (h.action === "bypass") {
      toggleBypass();
    } else if (h.action === "preset") {
      if (h.preset && presets.includes(h.preset)) open(h.preset);
    } else if (h.action === "tone-up" || h.action === "tone-down") {
      const control = h.tone ?? "bass";
      const delta = getToneStep() * (h.action === "tone-up" ? 1 : -1);
      setKnob(control, clampTone(tone[control] + delta));
    } else if (h.action === "device") {
      // Stable endpoint id: works again automatically once an unplugged device
      // returns; a currently-absent device just surfaces the backend error. Show
      // the OSD only once the switch actually succeeds. Switching output may change
      // whether offload engages, so reconcile after.
      if (h.device)
        api
          .setDefaultAudioDevice(h.device)
          .then(() => {
            refreshOffload();
            maybeOsd(h);
          })
          .catch((e) => flash(String(e)));
      return;
    } else if (h.action === "tone-reset") {
      resetTone();
    }
    maybeOsd(h);
  }

  // When the main window is unfocused (the only time the user can't see the
  // in-window feedback), surface the hotkey's result in the OSD overlay window.
  function maybeOsd(h: ReturnType<typeof getHotkeys>[number]) {
    if (windowFocused) return;
    const payload = payloadForHotkey(h, {
      tone, // setKnob already updated this synchronously
      bypassed: !isBypassed, // toggleBypass flips it; this is the state it becomes
      presetName: h.preset && presets.includes(h.preset) ? h.preset : undefined,
    });
    if (payload) emit(OSD_EVENT, payload).catch(() => {});
  }

  // Reconcile hardware offload with the active output (off the UI thread) and pick
  // up the fresh status. Called on demand — window focus and output switches — so
  // offload follows the active output without polling.
  function refreshOffload() {
    api
      .refreshHardware()
      .then((s) => (offload = s))
      .catch(() => {});
  }

  // Audio outputs can change as hardware is plugged/unplugged, so refresh on
  // mount and window focus (the picker only matters on the Hotkeys page).
  function loadDevices() {
    api.listAudioDevices().then((d) => (devices = d)).catch(() => {});
  }

  // (Re)register the global hotkeys whenever the list changes, debounced so a
  // burst of edits (typing a key) doesn't thrash OS registration. Reading
  // `accelerators()` at fire time picks up the final state of the burst.
  const hotkeyRegistration = createDebounce(() => {
    api.setHotkeys(accelerators()).then((f) => (failedHotkeys = f)).catch(() => {});
  }, 300);
  $effect(() => {
    void accelerators(); // dependency: re-runs on every add/edit/remove/reorder
    hotkeyRegistration.schedule();
    return () => hotkeyRegistration.cancel(); // also drops a pending run on unmount
  });

  async function reload() {
    // The reads are independent, so batch them into two parallel IPC rounds
    // instead of eight sequential round-trips — reload runs on every window
    // focus, so the latency is felt.
    const [st, dir, hw] = await Promise.all([
      api.apoStatus(),
      api.presetsDir(),
      api.hardwareStatus().catch(() => null),
    ]);
    status = st;
    presetsDirPath = dir;
    offload = hw;
    // The backend works with or without Equalizer APO (without it, presets run
    // against a private config dir and only hardware offload is audible), so
    // the library always loads — the banner explains the difference.
    const [pres, cats, act, byp, tn] = await Promise.all([
      api.listPresets(),
      api.presetCategories(),
      api.activePreset(),
      api.bypassed(), // backend owns bypass state (tray/hotkey too)
      api.getTone(),
    ]);
    presets = pres;
    categories = cats;
    active = act;
    isBypassed = byp;
    tone = tn;
    if (selected && !presets.includes(selected)) selected = null;
    // Default the editor to the active preset when nothing is selected
    // (e.g. on startup), so it opens in the right panel automatically.
    if (!selected && active) selected = active;
  }

  async function refresh() {
    if (refreshing) return;
    refreshing = true;
    try {
      // Spin for a beat so the click registers even though reload is instant.
      await Promise.all([reload(), new Promise((r) => setTimeout(r, 450))]);
    } catch (e) {
      flash(String(e));
    } finally {
      refreshing = false;
    }
  }

  function flash(m: string) {
    message = m;
    setTimeout(() => {
      if (message === m) message = "";
    }, 2600);
  }

  async function guard(action: () => Promise<void>) {
    busy = true;
    try {
      await action();
    } catch (e) {
      flash(String(e));
    } finally {
      busy = false;
    }
  }

  // Clicking a preset loads it from its saved file into the live config AND the
  // editor — so re-clicking the one you've been editing reverts unsaved live
  // changes, and clicking another switches the live sound to it.
  const open = (name: string) =>
    guard(async () => {
      await api.applyPreset(name);
      active = name;
      selected = name;
      editorReloadToken++; // force the editor to reload even if already selected
      await tick();
      scrollCurrentIntoView();
    });


  function flashImport(r: api.ImportReport, empty: string) {
    const n = r.imported.length;
    const s = r.skipped.length;
    if (n) {
      flash(`Imported ${n} preset${n === 1 ? "" : "s"}${s ? `, skipped ${s} already present` : ""}`);
    } else if (s) {
      flash(`All ${s} preset${s === 1 ? "" : "s"} already imported`);
    } else {
      flash(empty);
    }
  }

  const importPeace = () =>
    guard(async () => {
      const r = await api.importPeacePresets();
      await reload();
      showSettings = false;
      flashImport(r, "No PEACE presets found in the config folder");
    });

  const importFiles = () =>
    guard(async () => {
      const selected = await openDialog({
        multiple: true,
        title: "Select PEACE preset(s)",
        filters: [{ name: "PEACE preset", extensions: ["peace"] }],
      });
      if (!selected) return; // cancelled — stay on settings
      const paths = Array.isArray(selected) ? selected : [selected];
      const r = await api.importPeaceFiles(paths);
      await reload();
      showSettings = false;
      flashImport(r, "Nothing imported");
    });

  // Add a target curve from a REW/CSV text file (freq + level rows), normalised
  // to a 0 dB midband like measurements, so the two compare directly.
  const addTargetCurve = () =>
    guard(async () => {
      const picked = await openDialog({
        multiple: false,
        title: "Import target curve",
        filters: [{ name: "Curve (text/CSV)", extensions: ["txt", "csv"] }],
      });
      if (!picked || Array.isArray(picked)) return;
      const points = downsample(normalize(parseRew(await api.readTextFile(picked))));
      if (!points.length) {
        flash("No curve data found in that file");
        return;
      }
      const name = (picked.split(/[\\/]/).pop() ?? "target").replace(/\.[^.]+$/, "");
      addTarget(name, points);
      flash(`Added target “${name}”`);
    });

  const openPresets = () => guard(async () => await api.openPresetsDir());

  const changePresetsDir = () =>
    guard(async () => {
      const picked = await openDialog({ directory: true, title: "Choose preset storage folder" });
      if (!picked || Array.isArray(picked)) return;
      await api.setPresetsDir(picked);
      await reload();
      flash("Preset folder changed");
    });

  const resetPresetsDir = () =>
    guard(async () => {
      await api.resetPresetsDir();
      await reload();
      flash("Using the default preset folder");
    });

  // Toggle: bypass drops the filters (keeping the preamp); toggling again
  // restores the preset that was active when you bypassed. The backend owns the
  // bypass state and the captured preset, so the hotkey and tray toggle the same
  // way; reload() then syncs `active`/`isBypassed` back from it.
  const toggleBypass = () =>
    guard(async () => {
      const wasBypassed = isBypassed;
      await api.toggleBypass();
      await reload();
      if (wasBypassed) {
        if (active) {
          selected = active;
          editorReloadToken++;
          flash(`Restored “${active}”`);
        } else {
          flash("Un-bypassed");
        }
      } else {
        flash("Bypassed — filters off, preamp kept");
      }
    });

  const remove = (name: string) =>
    guard(async () => {
      await api.deletePreset(name);
      clearPresetView(name); // drop its curve-editor view state too
      if (selected === name) selected = null;
      flash(`Deleted “${name}”`);
      await reload();
    });


  // Returning from Settings recreates the preset list (resetting its scroll to
  // the top), so jump back to the active preset instead of leaving it at top.
  // (scrollCurrentIntoView is shared from PresetsPanel's module script.)
  let wasOnSubPage = false;
  $effect(() => {
    const onSubPage = showSettings || showHotkeys;
    if (wasOnSubPage && !onSubPage) scrollCurrentIntoView();
    wasOnSubPage = onSubPage;
  });


    async function setCategoryFor(name: string, value: string | null) {
    const current = categories[name] ?? null;
    if (value === current) return;
    categories = { ...categories };
    if (value === null) delete categories[name];
    else categories[name] = value;
    try {
      await api.setCategory(name, value);
    } catch (e) {
      flash(String(e));
      if (current === null) delete categories[name];
      else categories[name] = current;
    }
  }

  const newPreset = (nameRaw: string) =>
    guard(async () => {
      const name = nameRaw.trim();
      if (!name) { flash("Type a name first"); return; }
      if (presets.some((p) => p.toLowerCase() === name.toLowerCase())) {
        flash(`“${name}” already exists`);
        return;
      }
      await api.savePreset(name, starterConfig(bandCount));
      await api.applyPreset(name);
      await reload();
      active = name;
      selected = name;
      editorReloadToken++;
      flash(`Created “${name}” with ${bandCount} bands`);
    });

  const capture = (nameRaw: string) =>
    guard(async () => {
      const name = nameRaw.trim();
      if (!name) { flash("Type a name first"); return; }
      if (presets.some((p) => p.toLowerCase() === name.toLowerCase())) {
        flash(`“${name}” already exists`);
        return;
      }
      await api.captureCurrent(name);
      await reload();
      active = name;
      selected = name;
      flash(`Saved current config as “${name}”`);
    });

  const commitRename = (from: string, toRaw: string) => {
    const to = toRaw.trim();
    if (!to || to === from) return;
    if (presets.some((p) => p !== from && p.toLowerCase() === to.toLowerCase())) {
      flash(`“${to}” already exists`);
      return;
    }
    guard(async () => {
      await api.renamePreset(from, to);
      renamePresetView(from, to);
      if (selected === from) selected = to;
      await reload();
      flash(`Renamed to “${to}”`);
    });
  };


  onMount(() => {
    // Load every backend-persisted store first (one local file read each), so
    // the panels don't render defaults and then flip once the files resolve.
    // initHotkeys feeds the accelerators() effect, which registers once loaded.
    Promise.all([initHotkeys(), initPrefs(), initTargets(), initPresetView(), initTheme()])
      .catch(() => {})
      .then(() => {
        bandCount = defaultBandCount(); // captured before initPrefs resolved
        reload().then(scrollCurrentIntoView); // on open, jump to the active preset
      });
    loadDevices();
    const unlisten = listen("fastpeq:changed", () => reload());
    const unlistenHotkey = listen<string>("hotkey-pressed", (e) => dispatchHotkey(e.payload));
    // Track window focus so the OSD overlay only fires when the user can't see
    // the in-window feedback (minimized to tray, or another app in front).
    const win = getCurrentWindow();
    win.isFocused().then((f) => (windowFocused = f)).catch(() => {});
    const unlistenFocus = win.onFocusChanged(({ payload }) => (windowFocused = payload));
    // Pick up external changes to the presets folder when the window is focused.
    const onFocus = () => {
      reload();
      loadDevices();
      // Belt-and-braces resync: output changes are normally caught live by the
      // backend's OS watcher (which emits fastpeq:changed), this covers anything
      // missed while we were away.
      refreshOffload();
    };
    window.addEventListener("focus", onFocus);
    return () => {
      unlisten.then((f) => f());
      unlistenHotkey.then((f) => f());
      unlistenFocus.then((f) => f());
      window.removeEventListener("focus", onFocus);
    };
  });
</script>

<main>
  <header>
    <div class="brand">
      <h1>fast<span>peq</span></h1>
      <p class="tag">Equalizer APO preset manager</p>
    </div>
    <div class="settings">
      <button
        class="kbd-btn"
        class:on={showHotkeys}
        onclick={() => {
          showHotkeys = !showHotkeys;
          showSettings = false;
        }}
        aria-label={showHotkeys ? "Back to presets" : "Hotkeys"}
        title={showHotkeys ? "Back to presets" : "Hotkeys"}
      >
        {#if showHotkeys}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <line x1="19" y1="12" x2="5" y2="12" />
            <polyline points="12 19 5 12 12 5" />
          </svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <rect x="2" y="6" width="20" height="12" rx="2" />
            <path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M6 14h.01M18 14h.01M9 14h6" />
          </svg>
        {/if}
      </button>
      <button
        class="gear"
        class:on={showSettings}
        onclick={() => {
          showSettings = !showSettings;
          showHotkeys = false;
        }}
        aria-label={showSettings ? "Back to presets" : "Settings"}
        title={showSettings ? "Back to presets" : "Settings"}
      >
        {#if showSettings}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <line x1="19" y1="12" x2="5" y2="12" />
            <polyline points="12 19 5 12 12 5" />
          </svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
            <circle cx="12" cy="12" r="3" />
          </svg>
        {/if}
      </button>
    </div>
  </header>

  {#if !showSettings && !showHotkeys}
    {#if status && !status.installed}
      <div class="banner error">
        <strong>Equalizer APO not detected — software EQ is off.</strong>
        Presets still work on a supported hardware device in Hardware Only mode.
        For software EQ, install Equalizer APO and restart fastpeq.
      </div>
    {/if}
    {#if connectingHardware}
      <div class="banner info connecting">
        <span class="spinner" aria-hidden="true"></span>
        <span>Connecting to your hardware EQ device — the preset is already applied.</span>
      </div>
    {/if}
  {/if}

  {#if showSettings}
    <Settings
      {status}
      {presetsDirPath}
      {busy}
      bind:bandCount
      onAddTarget={addTargetCurve}
      onImportFiles={importFiles}
      onImportPeace={importPeace}
      onOpenPresets={openPresets}
      onChangePresetsDir={changePresetsDir}
      onResetPresetsDir={resetPresetsDir}
      onHardwareChanged={reload}
    />
  {:else if showHotkeys}
    <HotkeysPage {presets} {categories} {devices} failedIds={failedHotkeys} />
  {:else}
  <div class="workspace">
  <TonePanel bind:tone {status} {toneFlat} disabled={hardwareOnly} onPushTone={pushTone} onResetTone={resetTone} />
  <div class="grid">
    <PresetsPanel
      {presets}
      {categories}
      {active}
      {selected}
      {isBypassed}
      {status}
      {refreshing}
      {busy}
      {bandCount}
      onRefresh={refresh}
      onToggleBypass={toggleBypass}
      onOpen={open}
      onRemove={remove}
      onSetCategory={setCategoryFor}
      onNewPreset={newPreset}
      onCapture={capture}
      onRename={commitRename}
    />

    {#if selected}
      <Editor
        name={selected}
        {tone}
        bypassed={isBypassed}
        {forceAutoPreamp}
        offloadActive={!!offload?.active}
        {hardwareOnly}
        hwUserPregain={offload?.device?.user_pregain ?? true}
        hwCommitToApply={offload?.device?.commit_to_apply ?? false}
        hwCommitDelay={offload?.device?.commit_delay_ms ?? 500}
        reloadToken={editorReloadToken}
        onApplied={(n) => {
          active = n;
          isBypassed = false;
        }}
      />
    {:else}
      <section class="panel">
        <div class="placeholder">Select a preset to edit its filters, or save your current config.</div>
      </section>
    {/if}
  </div>
  </div>
  {/if}

{#if message}
    <div class="toast">{message}</div>
  {/if}
</main>

<style>
  main {
    max-width: 1200px;
    height: 100%;
    margin: 0 auto;
    padding: 16px 20px 18px;
    display: flex;
    flex-direction: column;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 10px;
  }
  .brand {
    display: flex;
    align-items: baseline;
    gap: 12px;
  }
  .settings {
    position: relative;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .gear,
  .kbd-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 6px;
    border-radius: 8px;
    color: var(--text);
  }
  .gear svg,
  .kbd-btn svg {
    width: 18px;
    height: 18px;
    display: block;
  }
  .gear.on,
  .kbd-btn.on {
    background: var(--panel-2);
  }

  h1 {
    margin: 0;
    font-size: 26px;
    letter-spacing: -0.5px;
  }
  h1 span {
    color: var(--accent);
  }
  .tag {
    margin: 0;
    color: var(--muted);
  }

  .banner {
    border: 1px solid var(--border);
    border-radius: 9px;
    padding: 8px 12px;
    margin-bottom: 12px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 14px;
  }
  .banner.error {
    background: #2a1c1c;
    border-color: #5a2d2a;
  }

  /* Startup: a non-blocking hint that the hardware device is still connecting
     (the preset itself is already shown and applied). */
  .banner.info {
    background: var(--panel-2);
    justify-content: flex-start;
    color: var(--muted);
  }
  .spinner {
    flex: none;
    width: 15px;
    height: 15px;
    border-radius: 50%;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    animation: spin 0.7s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .workspace {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  .grid {
    flex: 1;
    min-height: 0;
    min-width: 0;
    display: grid;
    grid-template-columns: 0.9fr 1.1fr;
    gap: 18px;
  }
  @media (min-width: 1080px) {
    .workspace {
      flex-direction: row;
    }
  }

  @media (max-width: 820px) {
    main {
      height: auto;
    }
    .grid {
      grid-template-columns: 1fr;
    }
  }

</style>
