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

export interface ApoStatus {
  installed: boolean;
  config_path: string | null;
  error: string | null;
}

// Global tone-control overlay (bass/mid/treble gains in dB) plus routing switches.
export interface Tone {
  bass: number;
  mid: number;
  treble: number;
  invert: boolean; // flip polarity on both channels
  swap: boolean; // swap left/right
}
