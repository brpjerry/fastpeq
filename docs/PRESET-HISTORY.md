# Preset history — versioning & undo plan

**Status:** 📋 planned — nothing implemented yet.

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
  are simpler, byte-faithful, and trivially restorable. No new dependencies.
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

- **Content:** the preset file's **prior bytes, copied verbatim** — not a
  parse/serialize round-trip. History must be byte-faithful for the same
  reason `Line::Raw` exists: we never mangle what we didn't model.
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

### What gets recorded, where in the code

All in `Manager` (crates/fastpeq-core), the same layer that keeps
`.categories.json` in step today:

| Operation | History action |
| --- | --- |
| `save_preset` over an existing file | Record prior bytes as `save` — **skipped when byte-identical** to the newest revision (no snapshot spam from re-saves). |
| `save_preset`, file absent | Nothing (imports and new presets have no prior state). |
| `delete_preset` | Record prior bytes as `delete`, then delete. |
| `rename_preset` | Rename `.history/<from>/` → `.history/<to>/` (merge file-by-file if the target dir exists — e.g. renaming onto a previously-deleted name). No snapshot; content didn't change. |
| `restore_revision` (new) | Record the *current* file (if any) as `restore`, then atomically write the revision's bytes to `<name>.txt`. |

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
    pub fn record(&self, name: &str, prior_bytes: &[u8], op: RevisionOp) -> io::Result<String>;
    pub fn list(&self, name: &str) -> io::Result<Vec<Revision>>;   // newest first
    pub fn load(&self, name: &str, id: &str) -> io::Result<Vec<u8>>;
    pub fn rename(&self, from: &str, to: &str) -> io::Result<()>;  // merge-move
    fn prune(&self, name: &str) -> io::Result<()>;                 // called by record
}
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

### Phase 3 — history browser in the editor · effort M

- A **History** action (clock icon) in the editor header, next to undo/redo.
- A panel/menu listing revisions: relative time + what happened
  ("2 h ago · overwritten by save", "yesterday · deleted"). Selecting one
  draws it as a faded ghost on the graph — the A/B-compare `reference` prop
  and `parseConfigEq` plumbing already do exactly this for the saved version,
  so the preview is nearly free.
- **Restore** writes it back (undoable, because restore snapshots first) and
  reloads the editor. While previewing, editing is locked — same
  `comparing`-style lock the editor already has.

### Phase 4 (optional, on demand)

Retention setting, "Clear history", disk-usage line in Settings; maybe
"restore as copy" (write to a new name instead of overwriting).

## Edge cases to cover in tests

- **Byte fidelity:** a preset with raw/unmodeled lines (comments, `Include:`,
  odd spacing, non-ASCII) survives record → restore byte-identical.
- **Dedupe:** save with identical bytes records nothing; save → save → save
  with two distinct contents records exactly two revisions.
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
