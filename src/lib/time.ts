// Coarse relative-time labels for the history menu ("just now", "5 min ago").
// Deliberately coarse: a revision list doesn't need live-ticking precision.

/** `ms` (unix millis) as a long date — "July 3rd, 2026" — for the history
 *  menu's creation-date labels. English month names by design (matches the
 *  requested format), not the system locale. */
export function longDate(ms: number): string {
  const d = new Date(ms);
  const month = d.toLocaleDateString("en-US", { month: "long" });
  return `${month} ${ordinal(d.getDate())}, ${d.getFullYear()}`;
}

/** 1 → "1st", 2 → "2nd", 3 → "3rd", 4 → "4th"… (11th–13th stay "th"). */
function ordinal(n: number): string {
  if (n % 100 >= 11 && n % 100 <= 13) return `${n}th`;
  return `${n}${["th", "st", "nd", "rd"][n % 10] ?? "th"}`;
}

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
