// Typed wrappers around the Rust IPC commands. The whole app surface lives here,
// so adding a feature is: add a core method -> a command -> a wrapper here.
import { invoke } from "@tauri-apps/api/core";
import type { ApoStatus, Config, Tone } from "./types";

export type { ApoStatus, Config, Tone };

export const apoStatus = () => invoke<ApoStatus>("apo_status");
export const listPresets = () => invoke<string[]>("list_presets");
export const activePreset = () => invoke<string | null>("active_preset");
export const applyPreset = (name: string) => invoke<void>("apply_preset", { name });
export const toggleBypass = () => invoke<void>("toggle_bypass");
export const bypassed = () => invoke<boolean>("bypassed");
export const captureCurrent = (name: string) => invoke<void>("capture_current", { name });
export const deletePreset = (name: string) => invoke<void>("delete_preset", { name });
export const renamePreset = (from: string, to: string) =>
  invoke<void>("rename_preset", { from, to });
export type Category = string; // free-form: speaker / headphone / iem / estat / earbud / …
export const presetCategories = () => invoke<Record<string, Category>>("preset_categories");
export const setCategory = (name: string, category: Category | null) =>
  invoke<void>("set_category", { name, category });

export const getPreset = (name: string) => invoke<Config>("get_preset", { name });
export const savePreset = (name: string, config: Config) =>
  invoke<void>("save_preset", { name, config });
/** Live preview. `pregain` (dB, ≤ 0) sets the hardware device's pregain when
 *  offload is active; `null` keeps the automatic pregain. */
export const applyLive = (config: Config, pregain: number | null = null) =>
  invoke<void>("apply_live", { config, pregain });

export const getTone = () => invoke<Tone>("get_tone");
export const setTone = (tone: Tone) => invoke<void>("set_tone", { tone });

/** (Re)register global hotkeys; resolves to the ids that failed to register. */
export const setHotkeys = (bindings: { id: string; accelerator: string }[]) =>
  invoke<string[]>("set_hotkeys", { bindings });

/** Persisted hotkey bindings — a raw JSON document owned by hotkeys.svelte.ts,
 *  stored by the backend as hotkeys.json in the app data dir (atomic writes),
 *  so bindings survive webview profile loss. `null` = never saved. */
export const loadHotkeyBindings = () => invoke<string | null>("load_hotkey_bindings");
export const saveHotkeyBindings = (json: string) =>
  invoke<void>("save_hotkey_bindings", { json });

/** A frontend store's persisted UI state document (preset view state, targets,
 *  prefs, theme) — a raw JSON document stored by the backend as `<key>.json` in
 *  the app data dir (atomic writes), so it survives webview profile loss like
 *  the hotkey bindings. `null` = never saved. Keys are allowlisted backend-side. */
export type UiStateKey = "preset-view" | "targets" | "prefs" | "theme";
export const loadUiState = (key: UiStateKey) => invoke<string | null>("load_ui_state", { key });
export const saveUiState = (key: UiStateKey, json: string) =>
  invoke<void>("save_ui_state", { key, json });

/** An audio output device, for the "switch output device" hotkey principal. */
export interface AudioDevice {
  id: string;
  name: string;
  is_default: boolean;
}
export const listAudioDevices = () => invoke<AudioDevice[]>("list_audio_devices");
export const setDefaultAudioDevice = (id: string) =>
  invoke<void>("set_default_audio_device", { id });

/** A hardware-EQ device fastpeq can offload a preset's first bands to. */
export interface HardwareDevice {
  id: string;
  name: string;
  manufacturer: string;
  model: string;
  max_filters: number;
}
/** EQ routing: offload off (`apo-only`), or which bands go to hardware. */
export type OffloadMode =
  | "apo-only"
  | "first-x"
  | "largest-change"
  | "minimize-preamp"
  | "hardware-only";

/** Current hardware-offload state. `enabled` is the global toggle; `active` means
 *  offload is actually engaged (the active output is a supported device). */
export interface HardwareStatus {
  enabled: boolean;
  active: boolean;
  device: HardwareDevice | null;
  version: string | null;
  error: string | null;
  max_filters: number | null;
  mode: OffloadMode;
}
export const listHardwareDevices = () => invoke<HardwareDevice[]>("list_hardware_devices");
/** Cheap status read (no reconcile). */
export const hardwareStatus = () => invoke<HardwareStatus>("hardware_status");
/** Reconcile offload with the active output (off the UI thread), then return status.
 *  Call on demand — focus, opening the panel, a mode change, an output switch. */
export const refreshHardware = () => invoke<HardwareStatus>("refresh_hardware");
export const setOffloadMode = (mode: OffloadMode) =>
  invoke<void>("set_offload_mode", { mode });
/** Filter positions (document order) in `config` currently sent to hardware. */
export const offloadSelection = (config: Config) =>
  invoke<number[]>("offload_selection", { config });

export const readTextFile = (path: string) => invoke<string>("read_text_file", { path });

export const presetsDir = () => invoke<string>("presets_dir");
export const setPresetsDir = (path: string) => invoke<void>("set_presets_dir", { path });
export const resetPresetsDir = () => invoke<void>("reset_presets_dir");
export const openPresetsDir = () => invoke<void>("open_presets_dir");

export interface ImportReport {
  imported: string[];
  skipped: string[];
  ignored: number;
}
export const importPeacePresets = () => invoke<ImportReport>("import_peace_presets");
export const importPeaceFiles = (paths: string[]) =>
  invoke<ImportReport>("import_peace_files", { paths });
