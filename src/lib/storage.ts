// Tiny localStorage wrappers that swallow access failures (private mode, disabled
// storage, etc.) so callers don't each repeat the same try/catch. Persisted UI
// preferences flow through here.

/** Read a string, or `fallback` if absent/unavailable. */
export function loadString(key: string, fallback: string): string {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}

/** Read a boolean (stored as "true"/"false"), or `fallback` if absent/unavailable. */
export function loadBool(key: string, fallback = false): boolean {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : v === "true";
  } catch {
    return fallback;
  }
}

/** Read a finite number, or `fallback` if absent/unparseable/unavailable. */
export function loadNumber(key: string, fallback: number): number {
  try {
    const raw = localStorage.getItem(key);
    if (raw === null) return fallback;
    const v = Number(raw);
    return Number.isFinite(v) ? v : fallback;
  } catch {
    return fallback;
  }
}

/** Persist a value (stringified), ignoring storage failures. */
export function save(key: string, value: string | number | boolean): void {
  try {
    localStorage.setItem(key, String(value));
  } catch {
    // ignore storage failures
  }
}
