// WebdriverIO config for the Tauri end-to-end smokes. WDIO talks to
// `tauri-driver` (on :4444), which in turn drives the app's WebView2 through the
// native `msedgedriver`. The app is launched with FASTPEQ_TEST_DATA_DIR pointed
// at a throwaway dir, so a run never touches the real Equalizer APO config or
// the user's preset library.
//
// Prerequisites (see e2e/README.md):
//   - cargo install tauri-driver
//   - msedgedriver matching the installed WebView2 runtime, at
//     e2e/drivers/msedgedriver.exe (or set MSEDGEDRIVER)
//   - a debug build: npm run build && cargo build -p fastpeq
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { DATA_DIR, seed } from "./helpers/seed.js";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const application = path.resolve(__dirname, "..", "target", "debug", "fastpeq.exe");
const tauriDriverBin = path.resolve(os.homedir(), ".cargo", "bin", "tauri-driver.exe");
const nativeDriver =
  process.env.MSEDGEDRIVER || path.resolve(__dirname, "drivers", "msedgedriver.exe");

let tauriDriver;

export const config = {
  hostname: "127.0.0.1",
  port: 4444,
  path: "/",
  specs: [path.resolve(__dirname, "specs", "**", "*.e2e.js")],
  maxInstances: 1,
  capabilities: [
    {
      "tauri:options": {
        application,
        env: { FASTPEQ_TEST_DATA_DIR: DATA_DIR },
      },
    },
  ],
  reporters: ["spec"],
  framework: "mocha",
  mochaOpts: { ui: "bdd", timeout: 120000 },
  logLevel: "warn",

  // Fail fast with actionable messages, then seed a fresh library before launch.
  onPrepare() {
    if (!fs.existsSync(application)) {
      throw new Error(
        `Debug app missing at ${application}. Build it first: npm run build && cargo build -p fastpeq`,
      );
    }
    if (!fs.existsSync(nativeDriver)) {
      throw new Error(
        `msedgedriver not found at ${nativeDriver}. Set MSEDGEDRIVER or drop a build matching the WebView2 runtime there (see e2e/README.md).`,
      );
    }
    seed();
  },

  beforeSession() {
    tauriDriver = spawn(tauriDriverBin, ["--native-driver", nativeDriver], {
      stdio: [null, process.stdout, process.stderr],
    });
  },

  afterSession() {
    tauriDriver?.kill();
  },
};
