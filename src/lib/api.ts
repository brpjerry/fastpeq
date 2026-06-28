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
export const applyLive = (config: Config) => invoke<void>("apply_live", { config });

export const getTone = () => invoke<Tone>("get_tone");
export const setTone = (tone: Tone) => invoke<void>("set_tone", { tone });

/** (Re)register global hotkeys; resolves to the ids that failed to register. */
export const setHotkeys = (bindings: { id: string; accelerator: string }[]) =>
  invoke<string[]>("set_hotkeys", { bindings });

/** An audio output device, for the "switch output device" hotkey principal. */
export interface AudioDevice {
  id: string;
  name: string;
  is_default: boolean;
}
export const listAudioDevices = () => invoke<AudioDevice[]>("list_audio_devices");
export const setDefaultAudioDevice = (id: string) =>
  invoke<void>("set_default_audio_device", { id });

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
