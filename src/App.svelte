<script lang="ts">
  import { onMount, tick } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import * as api from "./lib/api";
  import Editor from "./lib/Editor.svelte";
  import CategoryIcon from "./lib/CategoryIcon.svelte";
  import Knob from "./lib/Knob.svelte";
  import { dismissable } from "./lib/dismiss";
  import { ACCENTS, currentAccentId, applyAccent } from "./lib/theme";
  import { starterConfig, BAND_COUNTS, defaultBandCount, setDefaultBandCount } from "./lib/starter";
  import {
    getFilterSet,
    setFilterSet,
    getToneVolumeCap,
    setToneVolumeCap,
    getSpecialtyIcons,
    setSpecialtyIcons,
    getBluetoothIcons,
    setBluetoothIcons,
    getFilterShapes,
    setFilterShapes,
  } from "./lib/prefs.svelte";

  let status = $state<api.ApoStatus | null>(null);
  let presets = $state<string[]>([]);
  let categories = $state<Record<string, string>>({});
  let active = $state<string | null>(null);
  let selected = $state<string | null>(null);
  let newName = $state("");
  let message = $state("");
  let busy = $state(false);
  let showSettings = $state(false);
  let accentId = $state(currentAccentId());
  let isBypassed = $state(false);
  let bandCount = $state(defaultBandCount());
  let presetsDirPath = $state("");
  let refreshing = $state(false);
  let renaming = $state<string | null>(null);
  let renameValue = $state("");
  let editorReloadToken = $state(0);
  let query = $state("");
  let typeFilter = $state(""); // "" = all, a category value, or "__none" = uncategorized
  const filteredPresets = $derived(
    presets.filter((p) => {
      if (!p.toLowerCase().includes(query.trim().toLowerCase())) return false;
      if (typeFilter === "") return true;
      const cat = categories[p] ?? null;
      return typeFilter === "__none" ? cat === null : cat === typeFilter;
    }),
  );

  // Global tone overlay (bass/mid/treble), layered over the active preset by the
  // backend. Writes are throttled while a knob is being dragged.
  let tone = $state<api.Tone>({ bass: 0, mid: 0, treble: 0, invert: false, swap: false });
  const toneFlat = $derived(
    tone.bass === 0 && tone.mid === 0 && tone.treble === 0 && !tone.invert && !tone.swap,
  );
  let toneTimer: ReturnType<typeof setTimeout> | null = null;
  let toneLast = 0;
  function pushTone() {
    if (!status?.installed) return;
    const send = () => {
      toneLast = Date.now();
      api.setTone(tone).catch((e) => flash(String(e)));
    };
    const elapsed = Date.now() - toneLast;
    if (toneTimer) clearTimeout(toneTimer);
    if (elapsed >= 80) send();
    else toneTimer = setTimeout(send, 80 - elapsed);
  }
  function setKnob(which: "bass" | "mid" | "treble", v: number) {
    tone[which] = v;
    pushTone();
  }
  function resetTone() {
    if (toneTimer) clearTimeout(toneTimer);
    tone = { bass: 0, mid: 0, treble: 0, invert: false, swap: false };
    if (status?.installed) {
      toneLast = Date.now();
      api.setTone(tone).catch((e) => flash(String(e)));
    }
  }

  async function reload() {
    status = await api.apoStatus();
    presetsDirPath = await api.presetsDir();
    if (status.installed) {
      presets = await api.listPresets();
      categories = await api.presetCategories();
      active = await api.activePreset();
      isBypassed = await api.bypassed(); // backend owns bypass state (tray/hotkey too)
      tone = await api.getTone();
      if (selected && !presets.includes(selected)) selected = null;
      // Default the editor to the active preset when nothing is selected
      // (e.g. on startup), so it opens in the right panel automatically.
      if (!selected && active) selected = active;
    }
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
      isBypassed = false;
      editorReloadToken++; // force the editor to reload even if already selected
    });

  // Order here is the cycle order: headphone → iem → specialty → bluetooth →
  // speaker. `group` decides which are selectable ("base" always; the others gate
  // on a settings switch), but every category always *displays* its icon
  // (src/lib/icons/<value>.svg).
  const CATEGORIES: { value: string; label: string; group: "base" | "specialty" | "bluetooth" }[] = [
    { value: "headphone", label: "Headphone", group: "base" },
    { value: "iem", label: "IEM", group: "base" },
    { value: "estat", label: "Electrostatic", group: "specialty" },
    { value: "earbud", label: "Earbud", group: "specialty" },
    { value: "bluetooth_headphone", label: "BT Headphone", group: "bluetooth" },
    { value: "bluetooth_iem", label: "BT IEM", group: "bluetooth" },
    { value: "bluetooth_earbud", label: "BT Earbud", group: "bluetooth" },
    { value: "speaker", label: "Speaker", group: "base" },
  ];
  const CATEGORY_LABELS: Record<string, string> = Object.fromEntries(
    CATEGORIES.map((c) => [c.value, c.label]),
  );
  const categoryLabel = (c: string | undefined) =>
    c ? (CATEGORY_LABELS[c] ?? c) : "Uncategorized";

  // Categories you can actually assign: base always, plus enabled groups. Drives
  // both the left-click cycle and the right-click picker.
  const selectableCategories = $derived(
    CATEGORIES.filter(
      (c) =>
        c.group === "base" ||
        (c.group === "specialty" && getSpecialtyIcons()) ||
        (c.group === "bluetooth" && getBluetoothIcons()),
    ),
  );

  // The filter dropdown only offers types that some preset actually uses.
  const usedCategories = $derived(
    CATEGORIES.filter((c) => presets.some((p) => categories[p] === c.value)),
  );
  const hasUncategorized = $derived(presets.some((p) => !categories[p]));
  // Fall back to "All types" if the active filter's type no longer has presets.
  $effect(() => {
    const stillValid =
      typeFilter === "" ||
      (typeFilter === "__none" ? hasUncategorized : usedCategories.some((c) => c.value === typeFilter));
    if (!stillValid) typeFilter = "";
  });

  // Set or clear a preset's category. An absent key means "uncategorized"
  // (matching the backend), so clearing deletes the key rather than storing
  // `undefined` — which keeps the map a clean Record<string, string>.
  function withCategory(
    map: Record<string, string>,
    name: string,
    value: string | null,
  ): Record<string, string> {
    const next = { ...map };
    if (value === null) delete next[name];
    else next[name] = value;
    return next;
  }

  // Updated optimistically and saved without the global `busy` lock, which would
  // briefly dim (flash) the whole list; the change is reverted if the save fails.
  async function setCategoryFor(name: string, value: string | null) {
    const current = categories[name] ?? null;
    if (value === current) return;
    categories = withCategory(categories, name, value);
    try {
      await api.setCategory(name, value);
    } catch (e) {
      flash(String(e));
      categories = withCategory(categories, name, current); // revert on failure
    }
  }

  const cycleCategory = (name: string) => {
    // Cycle through the selectable set (base + enabled groups). A value not in
    // the active cycle (e.g. a disabled group's icon) lands on index 0 → none.
    const cycle: (string | null)[] = [null, ...selectableCategories.map((c) => c.value)];
    const current = categories[name] ?? null;
    setCategoryFor(name, cycle[(cycle.indexOf(current) + 1) % cycle.length]);
  };

  // Right-click a preset's icon to choose a type directly (left-click cycles).
  let catMenu = $state<{ name: string; x: number; y: number } | null>(null);
  function openCatMenu(e: MouseEvent, name: string) {
    e.preventDefault();
    const w = 200;
    const h = (selectableCategories.length + 1) * 30 + 10;
    catMenu = {
      name,
      x: Math.max(8, Math.min(e.clientX, window.innerWidth - w - 8)),
      y: Math.max(8, Math.min(e.clientY, window.innerHeight - h - 8)),
    };
  }
  function pickCategory(name: string, value: string | null) {
    catMenu = null;
    setCategoryFor(name, value);
  }

  // Custom device-type filter dropdown. A native <select> can't render the
  // category icons in its options, so this mirrors the right-click picker:
  // a trigger button plus a fixed-positioned themed menu anchored under it.
  let typeMenu = $state<{ left: number; top: number; minW: number } | null>(null);
  let typeTriggerEl = $state<HTMLButtonElement | null>(null);
  const typeFilterLabel = $derived(
    typeFilter === ""
      ? "All types"
      : typeFilter === "__none"
        ? "Uncategorized"
        : (CATEGORY_LABELS[typeFilter] ?? typeFilter),
  );
  function toggleTypeMenu() {
    if (typeMenu) {
      typeMenu = null;
      return;
    }
    const el = typeTriggerEl;
    if (!el) return;
    const r = el.getBoundingClientRect();
    typeMenu = {
      left: Math.max(8, Math.min(r.left, window.innerWidth - 200)),
      top: r.bottom + 4,
      minW: r.width,
    };
  }
  function pickType(v: string) {
    typeFilter = v;
    typeMenu = null;
  }

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
      if (selected === name) selected = null;
      flash(`Deleted “${name}”`);
      await reload();
    });

  // The new-preset form is collapsed to a single button until clicked, then
  // reveals a name field plus the two ways to create: from scratch or capture.
  let creating = $state(false);
  let presetListEl = $state<HTMLUListElement | null>(null);
  function startCreate() {
    newName = "";
    creating = true;
  }
  function cancelCreate() {
    creating = false;
    newName = "";
  }

  // Scroll the current preset — the one open in the editor, or failing that the
  // active one — into view, once the list has settled. Centers it so the list
  // lands on the active preset by default rather than at the top. A no-op if it
  // isn't in the visible (filtered) list.
  async function scrollCurrentIntoView() {
    await tick();
    const el =
      presetListEl?.querySelector("li.selected") ?? presetListEl?.querySelector("li.active");
    el?.scrollIntoView({ block: "center" });
  }

  // After creating a preset, make sure it's actually visible: a new preset is
  // uncategorized, so clear any active filter that would hide it, then scroll
  // the now-selected row into view.
  async function revealSelected() {
    query = "";
    typeFilter = "";
    await scrollCurrentIntoView();
  }

  // Keep the current preset in view when the visible list changes — after the
  // search is cleared or the device-type filter changes. (App-open is handled in
  // onMount.) Scrolls only if that preset is actually in the filtered list.
  let prevQuery = "";
  let prevFilter = "";
  $effect(() => {
    const q = query.trim();
    const tf = typeFilter;
    const searchCleared = !!prevQuery && !q;
    const filterChanged = tf !== prevFilter;
    prevQuery = q;
    prevFilter = tf;
    if (searchCleared || filterChanged) scrollCurrentIntoView();
  });

  function takeName(): string | null {
    const name = newName.trim();
    if (!name) {
      flash("Type a name first");
      return null;
    }
    if (presets.includes(name)) {
      flash(`“${name}” already exists`);
      return null;
    }
    return name;
  }

  const newPreset = () =>
    guard(async () => {
      const name = takeName();
      if (!name) return;
      await api.savePreset(name, starterConfig(bandCount));
      await api.applyPreset(name); // apply it live, so it's the active preset
      cancelCreate();
      await reload();
      active = name;
      selected = name;
      editorReloadToken++;
      flash(`Created “${name}” with ${bandCount} bands`);
      await revealSelected();
    });

  const capture = () =>
    guard(async () => {
      const name = takeName();
      if (!name) return;
      await api.captureCurrent(name);
      cancelCreate();
      await reload();
      active = name; // the capture matches the live config, so it's active
      selected = name;
      flash(`Captured “${name}”`);
      await revealSelected();
    });

  // Svelte action: focus + select the rename field as soon as it appears.
  function focusInput(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  function startRename(name: string) {
    renaming = name;
    renameValue = name;
  }

  function cancelRename() {
    renaming = null;
  }

  function commitRename() {
    const from = renaming;
    if (from === null) return;
    renaming = null; // leave edit mode regardless of outcome
    const to = renameValue.trim();
    if (!to || to === from) return;
    if (presets.includes(to)) {
      flash(`“${to}” already exists`);
      return;
    }
    guard(async () => {
      await api.renamePreset(from, to);
      if (selected === from) selected = to;
      await reload();
      flash(`Renamed to “${to}”`);
    });
  }

  onMount(() => {
    reload().then(scrollCurrentIntoView); // on open, jump to the active preset
    const unlisten = listen("fastpeq:changed", () => reload());
    // Pick up external changes to the presets folder when the window is focused.
    const onFocus = () => reload();
    window.addEventListener("focus", onFocus);
    return () => {
      unlisten.then((f) => f());
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
        class="gear"
        class:on={showSettings}
        onclick={() => (showSettings = !showSettings)}
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

  {#if !showSettings}
    {#if status && !status.installed}
      <div class="banner error">
        <strong>Equalizer APO not detected.</strong>
        {status.error ?? "Install Equalizer APO, then restart fastpeq."}
      </div>
    {/if}
  {/if}

  {#if showSettings}
    <section class="panel settings-page">
      <div class="panel-head">
        <h2>Settings</h2>
      </div>
      <div class="settings-body">
        <section class="settings-section">
          <h3>Accent color</h3>
          <p class="hint">Recolor the highlights throughout the app.</p>
          <div class="swatches">
            {#each ACCENTS as a}
              <button
                class="swatch"
                class:sel={accentId === a.id}
                style="--sw: {a.accent}"
                onclick={() => {
                  accentId = a.id;
                  applyAccent(a.id);
                }}
                title={a.name}
                aria-label={a.name}
              ></button>
            {/each}
          </div>
        </section>
        <section class="settings-section">
          <h3>New presets</h3>
          <p class="hint">Bands a new preset starts with — 0 gain, log-spaced frequencies.</p>
          <div class="seg">
            {#each BAND_COUNTS as n}
              <button
                class="seg-btn"
                class:sel={bandCount === n}
                onclick={() => {
                  bandCount = n;
                  setDefaultBandCount(n);
                }}>{n}</button
              >
            {/each}
          </div>
        </section>
        <section class="settings-section">
          <h3>Filter types</h3>
          <p class="hint">Which filter types the editor's type dropdown offers.</p>
          <div class="seg">
            <button
              class="seg-btn"
              class:sel={getFilterSet() === "basic"}
              onclick={() => setFilterSet("basic")}
            >
              Basic · PK, LSC, HSC
            </button>
            <button
              class="seg-btn"
              class:sel={getFilterSet() === "full"}
              onclick={() => setFilterSet("full")}
            >
              All filters
            </button>
          </div>
        </section>
        <section class="settings-section">
          <h3>Curve editor</h3>
          <p class="hint">How band handles are drawn on the expanded graph.</p>
          <div class="cat-switches">
            <label class="switch">
              <input
                type="checkbox"
                checked={getFilterShapes()}
                onchange={(e) => setFilterShapes(e.currentTarget.checked)}
              />
              <span class="track"><span class="thumb"></span></span>
              <span class="sw-label">Show each filter's shape (instead of a stem to the preamp)</span>
            </label>
          </div>
        </section>
        <section class="settings-section">
          <h3>Preset categories</h3>
          <p class="hint">
            Extra device types you can assign by clicking a preset's icon (beyond speaker,
            headphone, IEM). Presets already using one always display its icon, even with its
            group off.
          </p>
          <div class="cat-switches">
            <label class="switch">
              <input
                type="checkbox"
                checked={getSpecialtyIcons()}
                onchange={(e) => setSpecialtyIcons(e.currentTarget.checked)}
              />
              <span class="track"><span class="thumb"></span></span>
              <span class="sw-label">Specialty (electrostatic, earbud)</span>
            </label>
            <label class="switch">
              <input
                type="checkbox"
                checked={getBluetoothIcons()}
                onchange={(e) => setBluetoothIcons(e.currentTarget.checked)}
              />
              <span class="track"><span class="thumb"></span></span>
              <span class="sw-label">Bluetooth (headphone, IEM, earbud)</span>
            </label>
          </div>
        </section>
        <section class="settings-section">
          <h3>Tone generator</h3>
          <p class="hint">Maximum volume the curve editor's sine generator can reach.</p>
          <div class="cap-row">
            <input
              type="range"
              min="0.05"
              max="1"
              step="0.05"
              value={getToneVolumeCap()}
              oninput={(e) => setToneVolumeCap(Number(e.currentTarget.value))}
            />
            <span class="cap-val">{Math.round(getToneVolumeCap() * 100)}%</span>
          </div>
        </section>
        <section class="settings-section">
          <h3>Import PEACE presets</h3>
          <p class="hint">
            PEACE saves presets as <code>.peace</code> files in the Equalizer APO config folder.
          </p>
          <div class="settings-actions">
            <button class="primary" onclick={importFiles} disabled={busy || !status?.installed}>
              Choose .peace file(s)…
            </button>
            <button onclick={importPeace} disabled={busy || !status?.installed}>
              Import all from config folder
            </button>
          </div>
        </section>
        <section class="settings-section">
          <h3>Preset storage</h3>
          <p class="hint">Folder where your preset files (and their metadata) are kept.</p>
          <p class="path-line"><code>{presetsDirPath}</code></p>
          <div class="settings-actions">
            <button onclick={openPresets} disabled={busy}>Open folder</button>
            <button class="primary" onclick={changePresetsDir} disabled={busy}>Change folder…</button>
            <button onclick={resetPresetsDir} disabled={busy}>Use default</button>
          </div>
        </section>
        <section class="settings-section">
          <h3>Equalizer APO</h3>
          {#if status?.installed}
            <p class="hint">Config file: <code>{status.config_path}</code></p>
          {:else}
            <p class="hint">Not detected — install Equalizer APO and restart fastpeq.</p>
          {/if}
        </section>
      </div>
    </section>
  {:else}
  <div class="workspace">
  <section class="panel tone-panel">
    <div class="tone-head">
      <h2>Tone</h2>
      <span class="tone-sub">Global · layered over the active preset</span>
      <button class="tone-reset" onclick={resetTone} disabled={toneFlat || !status?.installed}>
        Reset
      </button>
    </div>
    <div class="tone-body">
      <div class="knobs">
        <Knob label="Bass" value={tone.bass} onInput={(v) => setKnob("bass", v)} />
        <Knob label="Mids" value={tone.mid} onInput={(v) => setKnob("mid", v)} />
        <Knob label="Treble" value={tone.treble} onInput={(v) => setKnob("treble", v)} />
      </div>
      <div class="switches">
        <label class="switch">
          <input type="checkbox" bind:checked={tone.invert} onchange={pushTone} disabled={!status?.installed} />
          <span class="track"><span class="thumb"></span></span>
          <span class="sw-label">Invert polarity</span>
        </label>
        <label class="switch">
          <input type="checkbox" bind:checked={tone.swap} onchange={pushTone} disabled={!status?.installed} />
          <span class="track"><span class="thumb"></span></span>
          <span class="sw-label">Switch L / R</span>
        </label>
      </div>
    </div>
  </section>
  <div class="grid">
    <section class="panel">
      <div class="panel-head">
        <h2>Presets</h2>
        <div class="head-actions">
          <button
            class="refresh"
            onclick={refresh}
            disabled={refreshing || !status?.installed}
            title="Refresh preset list"
            aria-label="Refresh preset list"
          >
            <svg class:spin={refreshing} viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M21 12a9 9 0 1 1-2.64-6.36" />
              <path d="M21 3v5h-5" />
            </svg>
          </button>
          <button
            class="ghost"
            class:on={isBypassed}
            onclick={toggleBypass}
            disabled={busy || !status?.installed}
            title="Drop the EQ filters (keeps the preamp) — click again to restore"
          >
            {isBypassed ? "Bypassed" : "Bypass"}
          </button>
        </div>
      </div>

      <div class="filters">
        <input
          class="search"
          type="search"
          placeholder="Search presets…"
          bind:value={query}
          onkeydown={(e) => {
            if (e.key === "Enter" && filteredPresets.length) {
              open(filteredPresets[0]);
              query = "";
            }
          }}
          disabled={!status?.installed}
        />
        <div class="type-dd">
          <button
            bind:this={typeTriggerEl}
            class="type-trigger"
            class:open={!!typeMenu}
            onclick={toggleTypeMenu}
            disabled={!status?.installed}
            aria-haspopup="listbox"
            aria-expanded={!!typeMenu}
            aria-label="Filter by device type"
          >
            <span class="type-trigger-icon">
              {#if typeFilter === ""}
                <svg class="type-all-svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 5h18M6 12h12M10 19h4" /></svg>
              {:else if typeFilter === "__none"}
                <CategoryIcon category={undefined} />
              {:else}
                <CategoryIcon category={typeFilter} />
              {/if}
            </span>
            <span class="type-trigger-label">{typeFilterLabel}</span>
            <svg class="type-caret" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M6 9l6 6 6-6" />
            </svg>
          </button>
        </div>
      </div>

      <ul class="presets" bind:this={presetListEl}>
        {#each filteredPresets as name (name)}
          <li class:active={name === active} class:selected={name === selected}>
            {#if renaming === name}
              <input
                class="rename-input"
                bind:value={renameValue}
                use:focusInput
                onblur={commitRename}
                onkeydown={(e) => {
                  if (e.key === "Enter") commitRename();
                  else if (e.key === "Escape") cancelRename();
                }}
              />
            {:else}
              <button
                class="cat"
                class:empty={!categories[name]}
                onclick={() => cycleCategory(name)}
                oncontextmenu={(e) => openCatMenu(e, name)}
                disabled={busy}
                title={`${categoryLabel(categories[name])} — click to cycle, right-click to choose`}
              >
                <CategoryIcon category={categories[name]} />
              </button>
              <button
                class="name"
                onclick={() => open(name)}
                ondblclick={() => startRename(name)}
                title="Click to load (reverts unsaved live changes) · double-click to rename"
              >
                {name}
              </button>
              <div class="row-actions">
                <button class="icon" onclick={() => startRename(name)} disabled={busy} title="Rename">
                  &#9998;
                </button>
                <button class="danger icon" onclick={() => remove(name)} disabled={busy} title="Delete">
                  &#10005;
                </button>
              </div>
            {/if}
          </li>
        {:else}
          <li class="empty">
            {query.trim() || typeFilter
              ? "No presets match your filters."
              : "No presets yet — create or capture one below."}
          </li>
        {/each}
      </ul>

      {#if creating}
        <div class="create">
          <input
            placeholder="New preset name"
            bind:value={newName}
            use:focusInput
            onkeydown={(e) => {
              if (e.key === "Enter") newPreset();
              else if (e.key === "Escape") cancelCreate();
            }}
            disabled={busy || !status?.installed}
          />
          <div class="create-actions">
            <button
              class="primary"
              onclick={newPreset}
              disabled={busy || !status?.installed}
              title="Start from {bandCount} empty bands (set the count in Settings)"
            >
              From scratch
            </button>
            <button
              class="capture-btn"
              onclick={capture}
              disabled={busy || !status?.installed}
              title="Build the preset from the current live Equalizer APO config"
            >
              Capture current
            </button>
            <button class="ghost create-cancel" onclick={cancelCreate} title="Cancel">Cancel</button>
          </div>
        </div>
      {:else}
        <button
          class="primary new-btn"
          onclick={startCreate}
          disabled={busy || !status?.installed}
        >
          + New preset
        </button>
      {/if}
    </section>

    {#if selected}
      <Editor
        name={selected}
        {tone}
        bypassed={isBypassed}
        reloadToken={editorReloadToken}
        onApplied={(n) => {
          active = n;
          isBypassed = false;
        }}
      />
    {:else}
      <section class="panel">
        <div class="placeholder">Select a preset to edit its filters, or capture your current config.</div>
      </section>
    {/if}
  </div>
  </div>
  {/if}

  {#if catMenu}
    {@const menu = catMenu}
    <div
      class="cat-menu"
      style="left:{menu.x}px; top:{menu.y}px"
      use:dismissable={{ onDismiss: () => (catMenu = null) }}
    >
      <button class="cat-menu-item" class:sel={!categories[menu.name]} onclick={() => pickCategory(menu.name, null)}>
        <span class="cat-menu-icon"><CategoryIcon category={undefined} /></span>
        Uncategorized
      </button>
      {#each selectableCategories as c}
        <button
          class="cat-menu-item"
          class:sel={categories[menu.name] === c.value}
          onclick={() => pickCategory(menu.name, c.value)}
        >
          <span class="cat-menu-icon"><CategoryIcon category={c.value} /></span>
          {c.label}
        </button>
      {/each}
    </div>
  {/if}

  {#if typeMenu}
    {@const m = typeMenu}
    <div
      class="cat-menu type-menu"
      role="listbox"
      style="left:{m.left}px; top:{m.top}px; min-width:{m.minW}px"
      use:dismissable={{ onDismiss: () => (typeMenu = null), ignore: typeTriggerEl }}
    >
      <button class="cat-menu-item" class:sel={typeFilter === ""} onclick={() => pickType("")}>
        <span class="cat-menu-icon"><svg class="type-all-svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 5h18M6 12h12M10 19h4" /></svg></span>
        All types
      </button>
      {#each usedCategories as c}
        <button class="cat-menu-item" class:sel={typeFilter === c.value} onclick={() => pickType(c.value)}>
          <span class="cat-menu-icon"><CategoryIcon category={c.value} /></span>
          {c.label}
        </button>
      {/each}
      {#if hasUncategorized}
        <button class="cat-menu-item" class:sel={typeFilter === "__none"} onclick={() => pickType("__none")}>
          <span class="cat-menu-icon"><CategoryIcon category={undefined} /></span>
          Uncategorized
        </button>
      {/if}
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
  }
  .gear {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 6px;
    border-radius: 8px;
    color: var(--text);
  }
  .gear svg {
    width: 18px;
    height: 18px;
    display: block;
  }
  .gear.on {
    background: var(--panel-2);
  }
  .ghost.on {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .ghost.on:hover:not(:disabled) {
    background: var(--accent-2);
    border-color: var(--accent-2);
  }
  .head-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .refresh {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 6px;
    border-radius: 7px;
    color: var(--muted);
  }
  .refresh:hover:not(:disabled) {
    color: var(--text);
  }
  .refresh svg {
    display: block;
  }
  .refresh svg.spin {
    animation: spin 0.6s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .settings-page {
    flex: 1;
  }
  .settings-body {
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 22px;
    padding-top: 4px;
  }
  .settings-section h3 {
    margin: 0 0 6px;
    font-size: 14px;
    font-weight: 600;
  }
  .settings-section .hint {
    margin: 0 0 12px;
    color: var(--muted);
    font-size: 13px;
  }
  .settings-section code {
    background: var(--panel-2);
    padding: 1px 6px;
    border-radius: 5px;
    font-size: 12px;
  }
  .path-line {
    margin: 0 0 12px;
  }
  .path-line code {
    word-break: break-all;
  }
  .settings-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .swatches {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
    /* Room for the selected swatch's 4px ring so the first one isn't clipped. */
    padding: 4px;
  }
  .swatch {
    width: 28px;
    height: 28px;
    padding: 0;
    border-radius: 50%;
    background: var(--sw);
    border: 2px solid transparent;
  }
  .swatch:hover {
    background: var(--sw);
  }
  .swatch.sel {
    box-shadow:
      0 0 0 2px var(--panel),
      0 0 0 4px var(--sw);
  }
  .seg {
    display: flex;
    gap: 6px;
  }
  .seg-btn {
    min-width: 46px;
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
  .cap-row {
    display: flex;
    align-items: center;
    gap: 12px;
    max-width: 320px;
  }
  .cap-row input[type="range"] {
    flex: 1;
  }
  .cap-val {
    width: 44px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .cat-switches {
    display: flex;
    flex-direction: column;
    gap: 12px;
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

  .workspace {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
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
  .switch {
    display: inline-flex;
    align-items: center;
    gap: 9px;
    cursor: pointer;
    user-select: none;
    font-size: 13px;
  }
  .switch input {
    position: absolute;
    opacity: 0;
    width: 0;
    height: 0;
  }
  .switch .track {
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
  .switch .thumb {
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

  .grid {
    flex: 1;
    min-height: 0;
    min-width: 0;
    display: grid;
    grid-template-columns: 0.9fr 1.1fr;
    gap: 18px;
  }

  /* Wide windows: the tone controls become a vertical pane to the right of the
     filters, instead of a horizontal bar above them. */
  @media (min-width: 1080px) {
    .workspace {
      flex-direction: row;
    }
    .tone-panel {
      order: 1; /* after the grid in row mode → on the right */
      width: 150px;
      padding: 12px 8px;
      overflow-y: auto;
    }
    .tone-head {
      justify-content: space-between;
    }
    .tone-sub {
      display: none; /* no room for the caption in a narrow pane */
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
    .switches .switch {
      flex-direction: column;
      gap: 6px;
      text-align: center;
    }
  }

  @media (max-width: 820px) {
    main {
      height: auto;
    }
    .grid {
      grid-template-columns: 1fr;
    }
    .presets {
      flex: none;
      overflow: visible;
    }
  }

  .filters {
    display: flex;
    gap: 8px;
    margin-bottom: 8px;
  }
  .search {
    flex: 1;
    min-width: 0;
  }
  .type-dd {
    flex: none;
    position: relative;
  }
  .type-trigger {
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 150px;
    padding: 6px 9px;
  }
  .type-trigger-icon {
    flex: none;
    display: inline-flex;
  }
  .type-trigger-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }
  .type-caret {
    flex: none;
    width: 14px;
    height: 14px;
    opacity: 0.65;
  }
  .type-trigger.open .type-caret {
    transform: rotate(180deg);
  }
  .type-all-svg {
    width: 16px;
    height: 16px;
    display: block;
  }

  .presets {
    list-style: none;
    margin: 0;
    padding: 0;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .presets li {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px;
    border-radius: 8px;
    border: 1px solid transparent;
  }
  .presets li.selected {
    border-color: var(--border);
    background: var(--panel-2);
  }
  /* The open preset (selected) and the applied one (active) — normally the same
     row — get the accent name; the selected row also has the background above. */
  .presets li.active .name,
  .presets li.selected .name {
    color: var(--accent);
    font-weight: 600;
  }
  .cat {
    flex: none;
    width: 26px;
    height: 26px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--border);
    background: var(--panel-2);
    padding: 0;
    line-height: 0;
    border-radius: 6px;
    color: var(--text);
    overflow: hidden;
  }
  .cat:hover:not(:disabled) {
    background: #2b3038;
    border-color: #3a4150;
  }
  .cat.empty {
    color: var(--muted);
  }

  /* Right-click device-type picker, positioned at the cursor. */
  .cat-menu {
    position: fixed;
    z-index: 81;
    min-width: 184px;
    max-height: 70vh;
    overflow-y: auto;
    padding: 4px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.45);
  }
  .cat-menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    white-space: nowrap;
    border: none;
    background: transparent;
    padding: 6px 8px;
    border-radius: 5px;
    font-size: 13px;
    color: var(--text);
  }
  .cat-menu-item:hover {
    background: var(--panel-2);
  }
  .cat-menu-item.sel {
    color: var(--accent);
  }
  .cat-menu-icon {
    flex: none;
    display: inline-flex;
  }

  .name {
    flex: 1;
    text-align: left;
    background: transparent;
    border: none;
    padding: 7px 8px;
  }
  .name:hover {
    background: var(--panel-2);
  }
  .row-actions {
    display: flex;
    gap: 4px;
  }
  /* Match the category indicator: 26×26 bordered icon buttons. */
  .row-actions .icon {
    width: 26px;
    height: 26px;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: 1px solid var(--border);
    background: var(--panel-2);
    border-radius: 6px;
    line-height: 0;
    font-size: 13px;
    color: var(--muted);
  }
  .row-actions .icon:hover:not(:disabled) {
    background: #2b3038;
    border-color: #3a4150;
    color: var(--text);
  }
  .row-actions .danger.icon:hover:not(:disabled) {
    border-color: var(--danger);
    color: var(--danger);
  }
  .rename-input {
    flex: 1;
    margin: 1px 0;
  }
  .empty {
    color: var(--muted);
    padding: 10px 8px;
  }

  .new-btn {
    width: 100%;
    margin-top: 12px;
  }
  .create {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 12px;
  }
  .create input {
    width: 100%;
  }
  .create-actions {
    display: flex;
    gap: 8px;
  }
  .create-actions .primary,
  .create-actions .capture-btn {
    flex: 1;
  }
  .create-cancel {
    flex: none;
  }
</style>
