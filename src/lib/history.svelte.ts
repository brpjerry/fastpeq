import type { Channel, FilterKind } from "./types";

export type EditorBand = {
  id: number;
  enabled: boolean;
  kind: FilterKind;
  freq: number;
  gain: number;
  q: number;
  channel: Channel;
};

export type Snapshot = {
  key: string;
  bands: EditorBand[];
  manualPreamp: number;
  balance: number;
};

export function createHistory(
  onRestore: (s: Snapshot) => void,
  isComparing: () => boolean
) {
  let history = $state<Snapshot[]>([]);
  let histIndex = $state(-1);
  const HIST_MAX = 100;

  return {
    get canUndo() {
      return histIndex > 0;
    },
    get canRedo() {
      return histIndex < history.length - 1;
    },
    reset(snap: Snapshot) {
      history = [snap];
      histIndex = 0;
    },
    flush(snap: Snapshot) {
      if (history[histIndex]?.key === snap.key) return;
      history = [...history.slice(0, histIndex + 1), snap];
      if (history.length > HIST_MAX) history = history.slice(history.length - HIST_MAX);
      histIndex = history.length - 1;
    },
    undo(currentSnap: Snapshot) {
      if (isComparing()) return;
      this.flush(currentSnap);
      if (histIndex <= 0) return;
      onRestore(history[--histIndex]);
    },
    redo(currentSnap: Snapshot) {
      if (isComparing()) return;
      this.flush(currentSnap);
      if (histIndex >= history.length - 1) return;
      onRestore(history[++histIndex]);
    }
  };
}
