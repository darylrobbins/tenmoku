---
task_uid: "01m4ry9wmh60xyrd2t3xe9332v"
task: "T60"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test disk_sync::tests::clean_buffer_reloads: an open clean file rewritten on disk, then a watcher batch naming it, makes the editor and a split of it show the new text, clean, with a caret on an unchanged row before the change kept and the same buffer id"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test disk_sync::tests::dirty_buffer_asks: with unsaved edits the Reload/Keep Mine prompt shows; Reload replaces the text and leaves it clean; Keep Mine keeps the edits dirty and the next cmd-s writes them with no conflict prompt"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test disk_sync::tests::own_writes_and_touches: a cmd-s whose watcher batch arrives while the save runs causes no reload or prompt; a rewrite with the same content only updates the stamp; deleting the file changes nothing and the next save recreates it"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test disk_sync::tests::activation_and_rescan: a file outside the root changed on disk reloads when the window becomes active; a Rescan batch checks each open buffer once; stats and reads run on the background executor"
    required: true
    manual: false
---

# Reload open files changed on disk, asking first when the buffer has unsaved changes (Mac T433)

Filed on helium from Mac T433 (~/coordination/helium-cards/T433.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-z.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium note: lent in batch Z to fill idle lanes; independent of helium's T47-T55 chain.

## Problem

Open buffers never follow their files: when a file changes outside fastedit (a `git checkout`, a formatter, another
editor), its tab keeps the old text, and the next save writes it over the new one. The project watcher already
reaches the workbench (`PanelEvent::FsChanged`, handled at `crates/fe-workbench/src/workbench.rs:409` for quick
open and nav only), but nothing checks open editors. The 2026-10-11 gap analysis (M6 "Files changed on disk")
found no card. User 2026-10-11: "Draft any work in earlier phases we haven't drafted yet."

The headless pieces exist after the earlier cards: `DiskStamp`, `Buffer::from_file`, `EditorModel::reload` (one
undoable edit that keeps the buffer, so splits and LSP follow), and `EditorView::is_saving`.

## Change

New module `crates/fe-workbench/src/disk_sync.rs` (`mod disk_sync;` in lib.rs), owned by the `Workbench`.

- Triggers:
  - each `PanelEvent::FsChanged` batch (workbench.rs:409): an `FsChange::Path` that names an open editor's file,
    compared by canonical path (cached per editor when first checked), checks that buffer; `FsChange::Rescan`
    checks every open buffer;
  - the window becoming active (`cx.observe_window_activation`): checks every open buffer, which covers files
    outside the project root and windows with no folder.
- A check runs once per buffer, however many panes or splits show it. It's skipped while that editor
  `is_saving()`, then runs again once the save finishes. `DiskStamp::of` runs on the background executor:
  - same as the buffer's stamp: nothing.
  - missing: the stamp is cleared (the next save recreates the file without a conflict prompt). Nothing else
    changes.
  - changed: the file is read and decoded on the background executor (`Buffer::from_file`'s decoding), then on the
    main thread:
    - text equal to the buffer's current text: only the stamp is set;
    - a clean buffer: `EditorModel::reload`;
    - a dirty buffer: `window.prompt(Warning, "“<name>” changed on disk.", Some("Reload it and lose your unsaved
      changes?"), ["Reload", "Keep Mine"])`. Reload: `reload`. Keep Mine: the stamp becomes the new file's, so the
      next save overwrites it without the conflict prompt. One prompt per buffer at a time; changes that arrive
      while it's up are checked again after the answer.

## Tests

New file `crates/fe-workbench/src/disk_sync_tests.rs`, declared from disk_sync.rs
(`#[cfg(test)] #[path = "disk_sync_tests.rs"] mod tests;`). Feed watcher batches through
`ProjectPanel::apply_fs_changes`, as the existing panel tests do, and run the background executor until parked.

## Implementation notes

- `DiskSync` lives on the `Workbench` (`disk_sync` field); the checks are `impl Workbench` methods in
  `disk_sync.rs`, so a check finds an open editor of its buffer again at each step and gives up if none is left.
- Matching a batch's paths to editors runs on the background executor: editor paths not yet cached are
  canonicalized there, and only batch paths whose file name is an open file's are canonicalized (a deleted file
  resolves through its directory).
- A check skipped for a running save observes an editor of the buffer; the save notifies it when it's over (the
  workbench passes notifications on to splits), and the check runs then. A save starting between a check's stat
  and its read landing sends that check to wait for the save too.
- Text equal to the file's only takes the stamp, as specified, even if the file's line ending or encoding
  differs.
- Tests count checks started, and stats and reads with whether they ran off the main thread
  (`DiskSync::test`, as `GitState`'s and the outline's counters do).
- A buffer that doesn't know its file's stamp (`Buffer::disk_stamp_known` false, e.g. one a host builds with
  `Buffer::new`) isn't checked, as `SaveJob` doesn't check it either; its first save stamps it.
- Review round 1:
  - Prompts are serialized per window (`DiskSync::asks`): GPUI's in-window prompt (Linux) replaces one already
    up, cancelling it. Only an explicit Keep Mine takes the file's stamp; a cancelled prompt changes nothing, so
    the next save still asks before it overwrites.
  - Each check records the buffer's stamp when it starts and validates it at every step, prompt answers
    included: a save or reload that landed meanwhile makes the result stale and the check starts over.
  - A check waiting for a save observes every editor of the buffer and is also polled (`SAVE_POLL`), so the
    editor that saved closing mid-save doesn't lose it.
  - Every changed path in a batch is canonicalized, so a symlink alias by another name matches; a batch of more
    than `MAX_BATCH_PATHS` paths checks every open buffer instead, as a rescan does.
