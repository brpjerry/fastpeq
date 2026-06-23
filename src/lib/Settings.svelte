<script lang="ts">
  // The Settings panel, extracted from App. Reads its prefs/targets/theme stores
  // directly; App passes the bits only it owns (status, presets dir, busy, the
  // shared new-preset band count) and the file/folder actions as callbacks.
  import type { ApoStatus } from "./types";
  import { ACCENTS, currentAccentId, applyAccent } from "./theme";
  import { BAND_COUNTS, setDefaultBandCount } from "./starter";
  import {
    getFilterSet,
    setFilterSet,
    getFilterShapes,
    setFilterShapes,
    getSpecialtyIcons,
    setSpecialtyIcons,
    getBluetoothIcons,
    setBluetoothIcons,
    getToneVolumeCap,
    setToneVolumeCap,
  } from "./prefs.svelte";
  import { getTargets, removeTarget, FLAT_TARGET } from "./targets.svelte";
  import Switch from "./Switch.svelte";

  let {
    status,
    presetsDirPath,
    busy,
    bandCount = $bindable(),
    onAddTarget,
    onImportFiles,
    onImportPeace,
    onOpenPresets,
    onChangePresetsDir,
    onResetPresetsDir,
  }: {
    status: ApoStatus | null;
    presetsDirPath: string;
    busy: boolean;
    bandCount: number;
    onAddTarget: () => void;
    onImportFiles: () => void;
    onImportPeace: () => void;
    onOpenPresets: () => void;
    onChangePresetsDir: () => void;
    onResetPresetsDir: () => void;
  } = $props();

  let accentId = $state(currentAccentId());
</script>

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
        <button class="seg-btn" class:sel={getFilterSet() === "basic"} onclick={() => setFilterSet("basic")}>
          Basic · PK, LSC, HSC
        </button>
        <button class="seg-btn" class:sel={getFilterSet() === "full"} onclick={() => setFilterSet("full")}>
          All filters
        </button>
      </div>
    </section>
    <section class="settings-section">
      <h3>Curve editor</h3>
      <p class="hint">How band handles are drawn on the expanded graph.</p>
      <div class="cat-switches">
        <Switch label="Show filter shape" checked={getFilterShapes()} onChange={(v) => setFilterShapes(v)} />
      </div>
    </section>
    <section class="settings-section">
      <h3>Target curves</h3>
      <p class="hint">
        Reference responses to aim for in the curve editor (REW/CSV text). Flat is always
        available; the editor's compensate toggle measures against the selected one.
      </p>
      <ul class="target-list">
        {#each getTargets() as t (t.id)}
          <li>
            <span class="target-name">{t.name}</span>
            {#if t.id === FLAT_TARGET.id}
              <span class="target-builtin">built-in</span>
            {:else}
              <button class="danger icon" onclick={() => removeTarget(t.id)} title="Remove target">
                &#10005;
              </button>
            {/if}
          </li>
        {/each}
      </ul>
      <div class="settings-actions">
        <button class="primary" onclick={onAddTarget} disabled={busy}>Add target curve…</button>
      </div>
    </section>
    <section class="settings-section">
      <h3>Preset categories</h3>
      <p class="hint">
        Extra device types you can assign by clicking a preset's icon (beyond speaker, headphone,
        IEM). Presets already using one always display its icon, even with its group off.
      </p>
      <div class="cat-switches">
        <Switch
          label="Specialty (electrostatic, earbud)"
          checked={getSpecialtyIcons()}
          onChange={(v) => setSpecialtyIcons(v)}
        />
        <Switch
          label="Bluetooth (headphone, IEM, earbud)"
          checked={getBluetoothIcons()}
          onChange={(v) => setBluetoothIcons(v)}
        />
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
        <button class="primary" onclick={onImportFiles} disabled={busy || !status?.installed}>
          Choose .peace file(s)…
        </button>
        <button onclick={onImportPeace} disabled={busy || !status?.installed}>
          Import all from config folder
        </button>
      </div>
    </section>
    <section class="settings-section">
      <h3>Preset storage</h3>
      <p class="hint">Folder where your preset files (and their metadata) are kept.</p>
      <p class="path-line"><code>{presetsDirPath}</code></p>
      <div class="settings-actions">
        <button onclick={onOpenPresets} disabled={busy}>Open folder</button>
        <button class="primary" onclick={onChangePresetsDir} disabled={busy}>Change folder…</button>
        <button onclick={onResetPresetsDir} disabled={busy}>Use default</button>
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

<style>
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
  .target-list {
    list-style: none;
    margin: 0 0 10px;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .target-list li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 4px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .target-name {
    font-size: 13px;
  }
  .target-builtin {
    font-size: 11px;
    color: var(--muted);
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
</style>
