# Preset history — versioning & undo plan

**Status:** 🟨 Phases 1 (core history), 2 (undo-delete toast), and 3
(loudness-matched compare) implemented; 4–5 open. Deviations from the sketches:
Phase 2 needed no `restorePresetView` — the view-state clear is simply
*deferred* to toast expiry (the toast's `onExpire`), so an undo finds the entry
untouched. Phase 3's session semantics got sharpened during implementation:
the matching session **outlives individual A⇄B flips** (the Compare button
flips sides within it) and ends on Esc, Save, or a preset (re)load — otherwise
the louder-side offset would vanish exactly when flipping back to the edit,
defeating the match. The label shows the *audible side's* extra offset
(`Auto (−0.0 dB)` on the quieter side).

Every preset mutation today is destructive: Save overwrites the file, Delete
removes it (plus its category and per-preset view state) on a single
unconfirmed click, and neither can be taken back. Review #4 flagged the delete
as a P1-adjacent UX hazard (item 4); rather than bolt on a confirm dialog,
this plan gives presets a real version history — which makes *every*
destructive operation reversible and turns undo-delete into one small use of a
general mechanism.

## Goals

1. **Nothing a user tuned is ever one click from gone.** Overwritten saves and
   deleted presets are recoverable.
2. **Undo-delete without a confirm dialog.** A confirmation prompt punishes
   the common case to guard the rare one; an undo toast does the opposite.
3. **Browsable per-preset history.** "What did this preset sound like before
   yesterday's edits?" — list, preview (curve ghost), restore.
4. Stay inside the house persistence rules: real files, atomic writes, the
   preset `.txt` stays plain shareable APO text, and the store dir stays
   uncluttered (see `PERSISTENCE.md`).

## Non-goals

- **Not git** and not a diff store — presets are ~1 KB; whole-file snapshots
  are simpler and trivially restorable. No new dependencies.
- **No history for the sidecars** (`.categories.json`, `.tone.json`) or the
  UI-state docs — the undo-delete flow carries the category/view state through
  the toast instead (see Phase 2).
- **No cross-machine sync semantics.** History is local files; if the user
  syncs their preset dir, history syncs with it, which is fine but not a
  design target.
- **No automatic snapshots of live edits.** Live drags never touch preset
  files today, and that stays true — history granularity is the explicit
  Save/Delete/Restore, so it records intent, not noise.

## Design

### Where history lives

`<preset store>/.history/<preset name>/<revision>.txt`

- Inside the preset store so history **follows the library** when the user
  relocates the dir (Settings → Change folder), exactly like the existing
  dot-prefixed sidecars. Switching to a *different* dir is a different
  library, and correctly a different history.
- `PresetStore::list` only picks top-level `*.txt` files, so the subdirectory
  never pollutes the preset list.
- Preset names already pass `is_safe_name` (no separators/traversal/reserved
  device names), so they are safe as directory names; the history module
  still re-validates (defense in depth, same as `path_for`).

### Revision files

- **Content:** the preset's prior EQ, **normalized** — parse the prior file,
  drop the master (`Both`-channel) `Preamp:` line and every **no-op filter**
  (a gain-type filter sitting at 0 dB — exactly what the editor's
  "Remove 0 dB" button calls "no effect"), and serialize that. The master
  preamp is *derived* state (Auto Preamp rewrites it; compare mode recomputes
  it — see loudness matching below), so replaying an old value is noise, not
  history. Balance trims (one-sided preamps), disabled bands with real
  settings, and unmodeled `Line::Raw` lines (comments, `Include:`,
  `Device:` …) are all kept verbatim through the parse/serialize round-trip —
  normalization strips only what is audibly and semantically inert.
- **Name:** `<unix-millis>-<op>.txt`, e.g. `1783300512345-save.txt`.
  Numerically sortable, self-describing, no index file to corrupt. If a
  millisecond collides (two ops in one ms), bump until unique.
- **`op` records what displaced the content:**
  - `save` — overwritten by a Save (or by "Save current" capture over an
    existing name).
  - `delete` — the preset was deleted; this snapshot *is* the undo-delete.
  - `restore` — overwritten by restoring an older revision (so a restore is
    itself undoable).
- **Writes are atomic** via the existing temp-file + rename writer.

### Uniqueness — history never holds duplicates

Comparisons happen on the **normalized** form on both sides (the current
preset is normalized in memory before comparing, so its preamp and no-op
filters never make two contents read as "different"). Two invariants,
enforced at record time:

1. **No two revisions with the same normalized content.** Recording a
   revision that matches an existing one *removes the older copy* — the
   content keeps its most recent position in the timeline instead of
   appearing twice.
2. **No revision matching the preset's current normalized content.** The
   canonical case: restore an old snapshot, then hit Save — the preset file
   now *is* that snapshot, so the old revision file is removed rather than
   left as a duplicate of the live preset (the restore already recorded the
   pre-restore state, so nothing is lost).

Consequence for restore: a restored preset comes back with its master preamp
**recomputed** (the Auto-Preamp anti-clip value over the restored bands), not
replayed — snapshots don't carry one. This is deliberate; a hand-set manual
preamp is the one thing history does not preserve.

### What gets recorded, where in the code

All in `Manager` (crates/fastpeq-core), the same layer that keeps
`.categories.json` in step today:

| Operation | History action |
| --- | --- |
| `save_preset` over an existing file | Record the prior content (normalized) as `save` — **skipped when its normalized form matches the new content** (no snapshot spam from re-saves), and any older revision it duplicates is removed (uniqueness invariant above). |
| `save_preset`, file absent | Nothing (imports and new presets have no prior state). |
| `delete_preset` | Record the prior content (normalized) as `delete`, then delete. |
| `rename_preset` | Rename `.history/<from>/` → `.history/<to>/` (merge file-by-file if the target dir exists — e.g. renaming onto a previously-deleted name). No snapshot; content didn't change. |
| `restore_revision` (new) | Record the *current* content (if any) as `restore`, then write the revision back to `<name>.txt` with the master preamp recomputed (anti-clip over the restored bands). The restored revision file itself is removed once it matches the live preset (invariant 2). |

### Retention

Prune at record time: keep the **newest 30 revisions per preset**, delete the
rest. No age-based pruning — a delete snapshot's value *grows* with age, and
30 × ~1 KB per edited preset is beneath caring about. History dirs for
presets that no longer exist are deliberately kept (they hold the undo).
A Settings knob ("keep more/less", "clear history", disk usage line) is
Phase 4 — YAGNI until someone asks.

### Core API sketch

```rust
// crates/fastpeq-core/src/history.rs
pub struct PresetHistory { dir: PathBuf }            // <store>/.history

pub struct Revision {
    pub id: String,            // "<unix-ms>-<op>"
    pub saved_at_ms: u64,
    pub op: RevisionOp,        // Save | Delete | Restore
}

impl PresetHistory {
    /// Normalizes `prior`, drops duplicates per the uniqueness invariants
    /// (against existing revisions AND against `current`), then records.
    /// Returns the new revision id, or None when nothing needed recording.
    pub fn record(&self, name: &str, prior: &Config, current: &Config, op: RevisionOp)
        -> io::Result<Option<String>>;
    pub fn list(&self, name: &str) -> io::Result<Vec<Revision>>;   // newest first
    pub fn load(&self, name: &str, id: &str) -> io::Result<Config>;
    pub fn rename(&self, from: &str, to: &str) -> io::Result<()>;  // merge-move
    fn prune(&self, name: &str) -> io::Result<()>;                 // called by record
}

/// The normal form snapshots are stored and compared in: master preamp and
/// no-op (0 dB gain-type) filters removed; everything else — balance trims,
/// disabled bands, raw lines — untouched. Lives in fastpeq-core beside the
/// tone/provenance strip helpers.
pub fn normalize(config: &Config) -> Config;
```

`Manager` gains `restore_revision(name, id) -> io::Result<()>`,
`preset_history(name)`, and `load_revision(name, id) -> io::Result<Config>`
(parsed, for the preview ghost). History failures on the record path should
be **non-fatal to the user's operation** (log-and-continue): a save must
never fail because a snapshot couldn't be written — the snapshot is the
safety net, not the payload.

### IPC surface

| Command | Notes |
| --- | --- |
| `preset_history(name) -> Vec<{id, savedAtMs, op}>` | For the history browser. |
| `get_revision(name, id) -> Config` | Parsed, for the curve ghost preview. |
| `restore_revision(name, id)` | Restores; invalidates the active-preset cache; refreshes the tray. |
| `delete_preset` (existing) | Return type becomes `Option<String>` — the id of the `delete` revision — so the undo toast can restore precisely what it deleted without a follow-up query. |

## Phases

### Phase 1 — the safety net (core only, no UI) · effort M

`history.rs` + `Manager` wiring + tests. From the moment this ships, every
save/delete is recoverable by hand (the files are plain APO text in
`.history/`), even before any UI exists. Update `PERSISTENCE.md`'s preset
store table with the `.history/` row in the same PR.

### Phase 2 — undo-delete toast (closes review #4 item 4) · effort S–M

Replaces the "confirm delete" idea entirely:

1. `App.remove(name)` captures the category (already in `categories[name]`)
   and the preset-view entry **before** deleting, and holds them in the toast
   state instead of clearing immediately — `clearPresetView(name)` moves to
   toast expiry (a leaked entry on hard quit is harmless and idempotent to
   re-clear later).
2. `deletePreset` returns the revision id; the toast shows
   **Deleted "name" — Undo** for ~8 s (the current `flash()` becomes an
   action-capable toast: message + optional button + configurable timeout —
   one small component, reused later for other undoable actions).
3. Undo → `restore_revision(name, id)`, re-`setCategory`, re-set the view
   entry (new `restorePresetView(name, entry)` beside the existing
   rename/clear helpers), `reload()`.

### Phase 3 — loudness-matched compare · effort M

Louder reads as "better", so an A/B where one side carries more level is a
biased test. This phase fixes that for **today's saved-version A/B compare**
and everything built on it (the history preview in Phase 4):

- **Entering compare force-enables Auto Preamp** for both sides — an
  effective override exactly like offload's `forceAutoPreamp`, never
  overwriting the user's stored pref — so each side gets its anti-clip
  master preamp and neither replays a stale manual value.
- **On top of that, an extra offset volume-matches the sides by audible
  level:** estimate each side's perceived loudness as the **A-weighted
  power mean of its magnitude response** over the probe grid, and attenuate
  the louder side by the difference X. Attenuation-only, so matching can
  never introduce clipping.
- **UI:** while matching is active the PreampRow **"Auto" switch turns red**
  and its label reads **"Auto (−X dB)"**, X being the extra offset applied to
  the currently-audible side (one decimal; plain "Auto" when it rounds to
  0.0). Toggling the switch **off** during compare disables *both* the
  forced auto preamp and the offset — an explicit opt-out for when the level
  difference is the thing being judged; toggling back on re-engages both.
  Exiting compare restores normal Auto behavior (stored pref, no offset).
- Implementation notes: the IEC 61672 A-weighting curve as `aWeightDb(f)` in
  eq.ts beside the response math; loudness proxy
  `L = 10·log10( mean_i 10^((resp(fᵢ)+A(fᵢ))/10) )` over `FREQS`;
  `X = L_louder − L_quieter`, applied as extra master-preamp attenuation
  through the existing `applyLive` path. Unit-test the weighting at
  reference points (A(1 kHz) = 0, A(100 Hz) ≈ −19.1, A(10 kHz) ≈ −2.5 dB)
  and the matcher on two curves with a known level gap.

### Phase 4 — history browser in the editor · effort M

- A **History** action (clock icon) in the editor header, next to undo/redo.
- A panel/menu listing revisions: relative time + what happened
  ("2 h ago · overwritten by save", "yesterday · deleted"). Selecting one
  draws it as a faded ghost on the graph — the A/B-compare `reference` prop
  and `parseConfigEq` plumbing already do exactly this for the saved version,
  so the preview is nearly free.
- Auditioning a revision goes through the Phase 3 loudness-matched compare,
  so "the old version sounds better" can't just mean "the old version was
  louder".
- **Restore** writes it back (undoable, because restore snapshots first; the
  master preamp is recomputed, not replayed) and reloads the editor. While
  previewing, editing is locked — same `comparing`-style lock the editor
  already has.

### Phase 5 (optional, on demand)

Retention setting, "Clear history", disk-usage line in Settings; maybe
"restore as copy" (write to a new name instead of overwriting).

## Edge cases to cover in tests

- **Raw-line fidelity:** a preset with unmodeled lines (comments, `Include:`,
  non-ASCII) survives record → restore with those lines verbatim — the
  normalization strips *only* the master preamp and no-op filters.
- **Normalization:** a snapshot of a preset with a master preamp, a balance
  trim, a 0 dB peaking band, and a disabled −3 dB band contains no preamp and
  no 0 dB band but keeps the trim and the disabled band.
- **Dedupe:** save with content that normalizes equal (e.g. only the preamp
  or a 0 dB band differs) records nothing; save → save → save with two
  distinct contents records exactly two revisions.
- **Uniqueness (the restore → save case):** restore revision R, then Save
  unchanged — R's file is removed; history holds no revision equal to the
  live preset and no two equal revisions, ever (property worth asserting
  after every mutation in the integration flows).
- **Restore preamp:** the restored preset's master preamp is the recomputed
  anti-clip value for its bands, regardless of what the file had when
  snapshotted.
- **Prune:** 31st revision drops the oldest; newest 30 remain in order.
- **Rename:** history follows; merging into an existing history dir keeps
  both sets; case-only rename works (same dir on Windows).
- **Delete → undo:** file, category, and view entry all return; the preset
  reads as inactive (no stamp resurrection).
- **Restore over unsaved current:** current content is snapshotted as
  `restore` first, so nothing is lost.
- **ms collision:** two records in the same millisecond get distinct ids.
- **History write failure is non-fatal:** save/delete still succeed.
- Integration flows in `tests/storage.rs` style; frontend toast/undo tests
  beside `App.test.ts`; manual cases added to `TEST_PLAN.md`.

## Rejected alternatives

- **Confirm dialog on delete** — friction on every delete to guard the rare
  mistake; solved better by undo, and it does nothing for overwritten saves.
- **OS Recycle Bin** (`trash` crate) — covers delete only, adds a dependency,
  and restores land outside the app's control (no toast, no category/view
  restore).
- **Git-backed store** — a `.git` in a user-visible, possibly-synced folder,
  a heavyweight dependency, and far more machinery than "keep the last N
  copies" needs.
- **Single journal file (JSONL)** — one file to corrupt, append isn't
  atomic-rename, and per-revision plain `.txt` files are hand-recoverable by
  design (a user can open one in Notepad and see their preset).
