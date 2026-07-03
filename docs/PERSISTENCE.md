# Persistence catalog

Every piece of state fastpeq persists, where it lives, who writes it, and the
rules it follows. Current as of v0.5.0.

## Principles

- **Real files over WebView storage.** WebView2's localStorage sits in a
  browser profile shared by the installed app and any debug build, and an
  unclean shutdown can silently discard it — real hotkey bindings were lost
  that way once. Everything the user would miss now lives in backend-owned
  files; localStorage survives only as a one-time migration source and a
  secondary backup copy.
- **Every write is atomic.** All files go through
  `write_config_atomic` / `write_text_atomic`
  ([writer.rs](../crates/fastpeq-core/src/apo/writer.rs)): a uniquely-named
  sibling temp file, then a rename over the target. This matters doubly for
  the live `config.txt`, which Equalizer APO hot-reloads the instant it
  changes — a half-written file would be read mid-save.
- **Never seed-overwrite unreadable data.** When a store's file exists but
  can't be parsed, the store starts from defaults and the file is left alone
  (it may be recoverable by hand); it's only rewritten on the user's next
  edit. The old localStorage store was once wiped by exactly the opposite
  behavior.
- **Provenance over inference.** Which preset is active is *recorded* (a stamp
  in `config.txt`), not re-derived from content equivalence, so Auto Preamp
  rewrites and hardware offload can't confuse it.

## Storage locations

| Location | Path (default) | Contents |
| --- | --- | --- |
| App data dir | `%APPDATA%\com.fastpeq.desktop` | Backend settings, hotkeys, UI-state docs, one-time config backup |
| Preset store | `<app data>\presets` (user-relocatable) | Preset `.txt` files + sidecar metadata |
| APO config dir | From registry `HKLM\SOFTWARE\EqualizerAPO\ConfigPath` | The live `config.txt` APO watches |
| WebView localStorage | WebView2 profile (shared by main + OSD windows) | Backup copies + legacy migration sources only |
| Hardware device | e.g. Moondrop DHA15 over USB HID | The offloaded EQ bands + pregain (RAM and flash) |
| Registry | `HKLM\SOFTWARE\EqualizerAPO` | **Read-only** — APO detection, never written |

Two overrides collapse the first three into one directory:

- **E2E** (`FASTPEQ_TEST_DATA_DIR`): that directory becomes both the app data
  dir *and* the APO config dir, so a test run is fully self-contained.
- **APO not installed**: the manager falls back to the app data dir as a
  private config dir (same layout as E2E). Everything keeps working —
  presets, tone sidecar, provenance, bypass — but nothing reads that
  `config.txt`, so sound shaping only happens via Hardware Only offload.
  `ApoStatus.installed` stays `false` so the UI can say so.

## App data dir (backend-owned, all atomic writes)

Owner: `AppState` in [state.rs](../src-tauri/src/state.rs).

| File | Format | What / when |
| --- | --- | --- |
| `settings.json` | JSON object | `presets_dir` (custom store location, `null` = default) and `offload_mode` (the 5-way EQ routing). Read at startup **before the WebView exists**, which is why it can't live in a frontend store. Written when either setting changes. |
| `hotkeys.json` | JSON array (opaque) | The global hotkey bindings. The backend never interprets the document — the schema belongs to [hotkeys.svelte.ts](../src/lib/hotkeys.svelte.ts) — but a write must at least be a JSON array. |
| `preset-view.json` | JSON object (opaque) | Per-preset curve-editor view state keyed by preset name: selected target, compensate/reference toggles, target offset/align frequency, and the **imported FR measurement** (the highest-value data here — a user can't trivially re-create an import). Owner: [preset-view.svelte.ts](../src/lib/preset-view.svelte.ts). |
| `targets.json` | JSON array (opaque) | User-imported target curves (id, name, points), normalised to a 0 dB midband. Owner: [targets.svelte.ts](../src/lib/targets.svelte.ts). |
| `prefs.json` | JSON object (opaque) | One document for all UI prefs: `filterSet`, `toneVolumeCap`, `toneStep`, `toneHeadroom`, `specialtyIcons`, `bluetoothIcons`, `filterShapes`, `autoPreamp`, `bandCount`. Every read validates its field (the file is hand-editable), so a bad value falls back to its default. Owner: [prefs.svelte.ts](../src/lib/prefs.svelte.ts). |
| `theme.json` | JSON object (opaque) | `{ "accent": "<id>" }` — the accent color. Owner: [theme.ts](../src/lib/theme.ts). |
| `config.backup.txt` | APO text | The user's pre-fastpeq `config.txt`, copied **exactly once** (`backup_once`) before fastpeq's first live write, and never touched again. |

The four opaque UI-state documents go through one generic command pair
(`load_ui_state` / `save_ui_state`). The backend allowlists the keys (a key
becomes a file name, so unknown keys are rejected outright) and checks each
document's top-level JSON type (object vs array) so a confused caller can't
replace a file with garbage. `hotkeys.json` predates the generic mechanism and
keeps its dedicated `load_hotkey_bindings` / `save_hotkey_bindings` commands
with the same guarantees.

### Frontend store lifecycle (all five stores)

1. On App mount (and OSD startup for the theme), `init<Store>()` loads the
   backend file — **before** the first data `reload()`, so panels don't render
   defaults and then flip.
2. File exists and parses → it is authoritative; nothing is written back.
3. File exists but is unreadable / wrong shape → defaults, **no write**.
4. No file → one-time migration from localStorage (the whole-doc backup key
   first, then the pre-file per-pref legacy keys), then persisted.
5. Every subsequent edit writes the file (source of truth) *and* the
   localStorage backup. A failed file write isn't surfaced — the backup still
   holds the data and the next edit retries.

The accent is special-cased for startup: `applyAccent` (CSS variables only) is
split from `setAccent` (persists), so the synchronous pre-mount apply of the
*cached* accent can never clobber `theme.json` with a stale value.

## Preset store (default `<app data>\presets`, relocatable via Settings)

Owner: `PresetStore` / `Manager` in
[store.rs](../crates/fastpeq-core/src/store.rs) and
[manager.rs](../crates/fastpeq-core/src/manager.rs). The location itself is
persisted in `settings.json`; moving it re-points the manager without copying
files.

| File | Format | What |
| --- | --- | --- |
| `<Name>.txt` | Native APO config text | One preset per file — deliberately plain APO text so presets are shareable/importable as-is. The parse/serialize round-trip is the core invariant (`parse(serialize(c)) == c`); unmodeled lines survive as `Line::Raw`. Presets **never** contain a provenance stamp (stripped on save) or the tone overlay (stripped on capture). |
| `.categories.json` | JSON object | Preset name → device-type category (`headphone`, `iem`, `speaker`, …). Tracks renames and deletes. |
| `.tone.json` | JSON object | The global tone overlay's knob values: `bass`/`mid`/`treble` (dB) + `invert`/`swap`. The sidecar is the persistence layer; `AppState` holds a cache so live knob drags don't re-read it. Written even while Hardware Only offload keeps the overlay out of the live config (`Manager::save_tone`), so the knobs come back when the mode changes. |

## The live `config.txt` (APO's config dir)

The one file Equalizer APO actually reads, and the reason atomic writes exist.
fastpeq composes it from three layers on every apply:

```text
# fastpeq:preset=<name>        ← provenance stamp (leading line)
Preamp: -6 dB                  ← the base EQ (preset or live edit)
Filter 1: ON PK Fc 100 Hz ...
# fastpeq tone overlay (managed — edit with the app's tone knobs)
Filter: ON LSC Fc 105 Hz ...   ← tone overlay block (fenced, recomposable)
Copy: L=R R=L                  ← polarity/swap routing, when enabled
# end fastpeq tone overlay
```

- **Provenance stamp** ([provenance.rs](../crates/fastpeq-core/src/provenance.rs)):
  `# fastpeq:preset=<name>`, an APO-ignored comment recording which preset
  produced the config. Advisory — trusted only while the base EQ still matches
  the named preset (or stamp-only while hardware offload is active, since the
  offloaded bands are absent from the file).
- **Tone overlay** ([tone.rs](../crates/fastpeq-core/src/tone.rs)): fenced by
  sentinel comments so it can be stripped and recomposed without disturbing
  the base EQ; `strip(compose(base, tone)) == base` holds even with L/R swap.
  A flat tone composes to nothing.
- **Hardware offload** rewrites what "base EQ" means here: in the split modes
  the file holds only the software remainder; in **Hardware Only** it holds no
  EQ at all (stamp + non-EQ raw lines only — APO is a pass-through, and the
  tone overlay is withheld too).
- Bypass keeps the preamp but drops filters, tone, and the stamp.

## WebView localStorage (backup / migration only)

Shared by the main and OSD windows (same origin). Access is wrapped by
[storage.ts](../src/lib/storage.ts), which swallows failures.

| Key | Role |
| --- | --- |
| `fastpeq.hotkeys` | Backup of `hotkeys.json` + pre-file migration source |
| `fastpeq.presetView` | Backup of `preset-view.json` + migration source |
| `fastpeq.targets` | Backup of `targets.json` + migration source |
| `fastpeq.prefs` | Whole-document backup of `prefs.json` |
| `fastpeq.accent` | Backup of `theme.json` **and** the synchronous pre-mount cache (no flash of default blue) |
| `fastpeq.filterSet`, `fastpeq.toneVolumeCap`, `fastpeq.toneStep`, `fastpeq.toneHeadroom`, `fastpeq.specialtyIcons`, `fastpeq.bluetoothIcons`, `fastpeq.filterShapes`, `fastpeq:autoPreamp`, `fastpeq.bandCount` | Legacy pre-`prefs.json` keys — read once during migration (only keys actually set are carried over), then left untouched |

## Hardware device (Moondrop DHA15 and future drivers)

Owner: [src-tauri/src/hardware/](../src-tauri/src/hardware/). The device holds
the offloaded bands + pregain in two tiers:

- **RAM** (`push_live`): live edits while dragging, and the automatic push
  when offload engages because the active output changed — following the
  output must not wear the flash.
- **Flash** (`push_commit`): deliberate actions — applying a preset, changing
  the routing mode, clearing on bypass — so the EQ survives the device being
  unplugged or used with another source (the point of Hardware Only mode).
  The worker coalesces rapid pushes but a requested commit sticks until the
  next successful flush, so a flash save is never dropped.

fastpeq never *reads* state back from the device except the firmware version;
the app's own files are always the source of truth for what to push.

## What is deliberately not persisted

- Bypass state, the un-bypass restore config, and `last_full` (the un-split EQ
  while offloading) — runtime-only in `AppState`; a restart re-derives from
  `config.txt`.
- The editor's manual two-stage preamp values while Auto is off — runtime-only
  so presets stay pure EQ.
- Window geometry (Tauri defaults), and anything derivable from the files
  above.
