// Seeds the throwaway data dir the app is pointed at via FASTPEQ_TEST_DATA_DIR,
// and reads back the live config.txt the backend writes. Plain ESM (no TS) to
// keep the WDIO harness dependency-light.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/** Self-contained data dir = app data dir AND the (fake) APO config dir. */
export const DATA_DIR = path.resolve(__dirname, "..", ".e2e-data");
export const PRESETS_DIR = path.join(DATA_DIR, "presets");
/** The live config the app writes — our window into what the backend did. */
export const CONFIG_PATH = path.join(DATA_DIR, "config.txt");

/** Seeded presets: name -> APO file body (valid for the core parser). */
export const SEED_PRESETS = {
  BassBoost: "Preamp: -6 dB\nFilter 1: ON PK Fc 60 Hz Gain 6 dB Q 0.7\nFilter 2: ON PK Fc 200 Hz Gain -2 dB Q 1\n",
  Vocal: "Preamp: -3 dB\nFilter 1: ON PK Fc 2500 Hz Gain 3 dB Q 1.5\n",
  Studio: "Preamp: -4 dB\nFilter 1: ON PK Fc 8000 Hz Gain 2 dB Q 0.7\n",
};

/** Device-type categories (free-form strings the UI maps to icons). */
export const SEED_CATEGORIES = { BassBoost: "headphone", Vocal: "iem", Studio: "speaker" };

/** Wipe and recreate the data dir with the seeded library. Run before launch. */
export function seed() {
  fs.rmSync(DATA_DIR, { recursive: true, force: true });
  fs.mkdirSync(PRESETS_DIR, { recursive: true });
  for (const [name, body] of Object.entries(SEED_PRESETS)) {
    fs.writeFileSync(path.join(PRESETS_DIR, `${name}.txt`), body);
  }
  fs.writeFileSync(
    path.join(PRESETS_DIR, ".categories.json"),
    JSON.stringify(SEED_CATEGORIES, null, 2),
  );
}

/** The live config.txt the app has written (empty string if none yet). */
export function readConfig() {
  try {
    return fs.readFileSync(CONFIG_PATH, "utf8");
  } catch {
    return "";
  }
}
