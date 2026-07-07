// Coarse relative-time labels for the history menu ("just now", "5 min ago").
// Deliberately coarse: a revision list doesn't need live-ticking precision.

/** A human label for how long ago `ms` (unix millis) was, relative to `now`. */
export function timeAgo(ms: number, now: number = Date.now()): string {
  const s = Math.max(0, Math.floor((now - ms) / 1000));
  if (s < 60) return "just now";
  const min = Math.floor(s / 60);
  if (min < 60) return `${min} min ago`;
  const h = Math.floor(min / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.floor(h / 24);
  if (d === 1) return "yesterday";
  if (d < 30) return `${d} days ago`;
  return new Date(ms).toLocaleDateString();
}
