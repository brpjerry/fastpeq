# End-to-end smokes (WebdriverIO + tauri-driver)

Layer 2 of the [test plan](../docs/TEST_PLAN.md): drive the **real built app** —
real Svelte UI ↔ real Rust backend ↔ real `config.txt` writes — over WebDriver.
Heavier and flakier than the unit suite, so it's a small set of critical-path
smokes, run manually / in CI rather than on every push.

## How it stays hermetic

The app honors `FASTPEQ_TEST_DATA_DIR`: when set, that directory becomes both the
app-data dir **and** the (fake) Equalizer APO config dir, so a run reads/writes
only its own throwaway `config.txt` and preset library — never the machine's real
APO install. The harness seeds that dir (`helpers/seed.js`) with a few presets +
categories before launch, and asserts against the `config.txt` the backend writes.

## Layout

- `wdio.conf.js` — boots `tauri-driver`, points it at the debug build, injects the
  test data dir, seeds the library.
- `helpers/seed.js` — seeds the data dir; reads back `config.txt`.
- `specs/smoke.e2e.js` — launch, apply, bypass round-trip, create, device filter.

## Running locally (Windows)

1. **tauri-driver**: `cargo install tauri-driver`
2. **msedgedriver** matching the installed WebView2 runtime. Check the version
   (the runtime is separate from the Edge browser — read the runtime, not Edge):
   ```powershell
   (Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}').pv
   ```
   Download `edgedriver_win64.zip` for that version from
   `https://msedgedriver.microsoft.com/<version>/edgedriver_win64.zip`, and place
   `msedgedriver.exe` at `e2e/drivers/` (or point `MSEDGEDRIVER` at it).
3. **Build the app**: `npx tauri build --debug --no-bundle`. This must be a
   `tauri build` (it runs `npm run build` then embeds `dist/` into the debug
   binary). A plain `cargo build` produces a debug binary that loads the frontend
   from the dev server (`devUrl`, localhost:1420) instead, so the app shows
   `ERR_CONNECTION_REFUSED` and every spec fails to find the UI.
4. **Run**: `npm run e2e`

## CI

`.github/workflows/e2e.yml` runs the whole thing on `windows-latest` via manual
dispatch (Actions tab → E2E → Run workflow). It installs `tauri-driver`, fetches
the msedgedriver matching the runner's Edge, builds, and runs the smokes. Kept off
push/PR until it's proven stable; add a `schedule:` trigger there when it is.
