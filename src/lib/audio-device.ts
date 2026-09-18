import type { AudioDevice } from "./api";

const normalizeName = (name: string) => name.trim().replace(/\s+/g, " ").toLowerCase();

// Windows can add an instance number after reinstalling an endpoint, e.g.
// "Speakers (2- USB DAC)". Keep model numbers and the rest of the name intact.
const withoutInstance = (name: string) =>
  name.replace(/(^|\()\d+\s*-\s*/g, "$1");

/** Resolve a saved output, preferring its id, then a unique full-name match. */
export function resolveAudioDevice(
  id: string,
  name: string | undefined,
  devices: AudioDevice[],
): AudioDevice {
  const existing = devices.find((d) => d.id === id);
  if (existing) return existing;

  const normalized = normalizeName(name ?? "");
  if (normalized) {
    // Prefer the full name before relaxing Windows instance numbering. Never
    // break ties by list order or the current default: either could be wrong.
    for (const normalize of [normalizeName, (s: string) => withoutInstance(normalizeName(s))]) {
      const matches = devices.filter((d) => normalize(d.name) === normalize(normalized));
      if (matches.length === 1) return matches[0];
      if (matches.length > 1) {
        throw new Error(`Multiple audio outputs match "${name}". Choose the device in Hotkeys.`);
      }
    }
  }
  throw new Error(`Audio output "${name || "Unknown device"}" is unavailable.`);
}
