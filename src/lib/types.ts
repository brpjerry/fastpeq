// Mirrors the serde representation of fastpeq-core's config model.

export type FilterKind =
  | "Peak"
  | "LowShelf"
  | "HighShelf"
  | "LowShelfQ"
  | "HighShelfQ"
  | "LowPass"
  | "HighPass"
  | "LowPassQ"
  | "HighPassQ"
  | "BandPass"
  | "Notch"
  | "AllPass";

// Equalizer APO Channel: scope. `other` preserves specs we don't model.
export type Channel = { kind: "both" | "left" | "right" } | { kind: "other"; spec: string };

export interface Filter {
  enabled: boolean;
  kind: FilterKind;
  freq: number;
  gain: number | null;
  q: number | null;
  index: number | null;
  channel: Channel;
}

// Line is serialized adjacently-tagged: { kind, value }.
export type Line =
  | { kind: "Preamp"; value: { gain: number; channel: Channel } }
  | { kind: "Filter"; value: Filter }
  | { kind: "Raw"; value: string };

export interface Config {
  lines: Line[];
}

// Whether Equalizer APO reaches the *current* output. Separate from `installed`:
// APO's Configurator enables it per render endpoint, so an installed APO can still
// leave the active output untouched. "unknown" means the background check hasn't
// landed (or couldn't tell) and is rendered as the normal healthy state.
export type ApoOutputState =
  | "active"
  | "not-installed"
  | "not-on-output"
  | "enhancements-off"
  | "unknown";

export interface ApoStatus {
  installed: boolean;
  config_path: string | null;
  error: string | null;
  output_state: ApoOutputState;
  output_name: string | null;
}

// Global tone-control overlay (bass/mid/treble gains in dB) plus routing switches.
export interface Tone {
  bass: number;
  mid: number;
  treble: number;
  invert: boolean; // flip polarity on both channels
  swap: boolean; // swap left/right
}
