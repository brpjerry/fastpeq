# fastpeq

A fast, clean, High-DPI-friendly preset manager for [Equalizer APO](https://equalizerapo.com/)
— a lightweight alternative to PEACE.

## Why

PEACE is the de-facto Equalizer APO GUI, but it's slow, **restarts its engine on
every preset switch**, clutters APO's config folder, and scales poorly on
High-DPI displays. The one modern alternative, Mega Switcher, has been
unmaintained since 2022.

fastpeq leans on a fact PEACE ignores: **Equalizer APO watches `config.txt` and
live-reloads it**. So switching a preset is just an atomic file write — instant,
no restart, no process churn.

## Design

A hard split between a UI-agnostic Rust core and a thin Tauri + Svelte shell.

| Component | Role | Status |
|-----------|------|--------|
| `crates/fastpeq-core` | Detect APO, parse/edit/serialize configs, preset store, atomic writes | **Phase 1–2 — done** |
| `src-tauri` | Tauri 2 commands, system tray, `Ctrl+Alt+B` bypass hotkey, single-instance | **Phase 2 — done** |
| `src/` | Svelte + TS UI: preset list, switch/capture/delete, parametric band editor + live response curve | **Phase 2–3 — done** |

### `fastpeq-core`

The core models an APO configuration as an ordered list of lines. It only
understands `Preamp:` and `Filter:` lines; everything else (`Include:`,
`Device:`, `GraphicEQ:`, `Convolution:`, comments) is preserved **verbatim** so
edits never mangle a user's existing config.

The correctness guarantee is a lossless model round-trip:

```
parse(serialize(config)) == config
```

Supported filter types: `PK`, `LS`, `HS`, `LSC`, `HSC`, `LP`, `HP`, `LPQ`,
`HPQ`, `BP`, `NO`, `AP`. Unrecognised types or variants we can't reproduce
faithfully (e.g. `LS 6dB`) are kept as raw lines rather than coerced.

## Building

Requires the Rust toolchain (MSVC) and Node.js.

```sh
# Core library only (fast; no frontend needed) — this is the default workspace member.
cargo test                      # parse/serialize + store/writer/manager suites
cargo test -- --ignored         # also the live APO-detection smoke test

# Full app
npm install
npm run tauri dev               # run fastpeq with hot-reload
npm run tauri build             # produce an installer
```

## Roadmap

1. **Core** ✅ — model, parser, serializer, round-trip tests, APO detection.
2. **Switch MVP** ✅ — atomic `config.txt` writer, preset store, manager; tray switching,
   `Ctrl+Alt+B` bypass hotkey, single-instance; apply / capture-current / delete / bypass;
   raw-text preset editor; first-run backup of the existing `config.txt`.
3. **Editor** ✅ — structured parametric band CRUD (add/remove/enable, type, Fc/Gain/Q), preamp
   control, and a **live magnitude-response curve** (client-side RBJ biquad math). Save / Save & Apply.
4. **Import/polish** *(next)* — PEACE import, autostart, per-preset hotkeys, standalone release packaging.
