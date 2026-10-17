---
task_uid: "01m50ew65vxvabs72scwg19nec"
task: "T117"
rev: 5
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test ex_write_path::parses: :w a.txt, :w! a\\ b.txt, :sav x and :saveas! x give HostCommand::Write and SaveAs with their force and unescaped paths; :sav alone is E471; :wq a stays unsupported; every existing fe-vim ex test passes"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test vim_host::write_path_tests::write_a_copy: :w other.txt on an edited named buffer writes other.txt while the tab keeps its path and stays dirty; again without ! shows E13 and leaves other.txt; :w! other.txt overwrites it"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test vim_host::write_path_tests::save_as: :sav b.txt on a named buffer, and :w b.txt on an untitled one, rename the tab to b.txt and leave it clean; vim_host_tests pass unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "fe-editor no_clobber_tests: a non-forced SaveJob persist onto a target created after the job was prepared fails with an exists/changed error and leaves that file's bytes intact (no check-to-rename window: the rename itself refuses an existing target); a forced one replaces it"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench vim_write_no_clobber_tests: :w other.txt and :sav other.txt onto a file created after E13's check give E13 and leave it intact, while :w! and :sav! overwrite; the save panel's Save As still replaces a file the user agreed to replace"
    required: true
    manual: false
  - id: "C6"
    text: "Rev 3 regressions: (F2) vim_write_no_clobber_tests creates the destination between the host's E13 check and the write via a test seam: :w {path} and :sav {path} give E13 and leave it intact, with ! they overwrite; (F3) no_clobber_tests: a dangling symlink created at a requested path that did not exist at the preflight is refused by a non-forced save and left in place; (F4) a project-panel rename into a pending copy destination is refused until the copy finishes"
    required: true
    manual: false
  - id: "C7"
    text: "Rev 4 regressions: (F6) with .editorconfig trim_trailing_whitespace and insert_final_newline on, Save As (:sav and the save panel, immediate and queued) writes the same normalised text an ordinary save does; (F4) a :w {path} copy paused during its background write keeps its destination reserved against a project-panel rename, even after its tab and window close, and releases it when the copy finishes"
    required: true
    manual: false
  - id: "C8"
    text: "Rev 5 tests: (F4) a :w {path} copy paused inside fe-gpui's background write (a doc(hidden) pub hook) keeps its destination reserved against a project-panel rename, also after its tab and window close, and after the write is released and completes the registry no longer holds the destination; (F8) the save panel's Save As, and a queued Save As whose source and destination .editorconfig differ, write the destination's normalised text"
    required: true
    manual: false
---

# Vim :w {path} writes a copy and :sav[eas] {path} saves as, with E13 for an existing file without ! (Mac T435)

## Problem

Vim's `:w {path}` is refused: `host_command` (`crates/fe-vim/src/ex.rs:331-345`) returns
`ExError::Unsupported(":w {path}")`, and `:sav[eas]` is E492. The module docs say so
(`crates/fe-vim/src/lib.rs:331-333`, "`:w {path}` isn't supported yet"). The 2026-10-11 gap analysis (M6 "Save As")
found no card. User 2026-10-11: "Draft any work in earlier phases we haven't drafted yet."

## Change

- `crates/fe-vim/src/ex.rs`: `HostCommand::Write { force, path: Option<String> }` and a new
  `HostCommand::SaveAs { force, path: String }`. In `host()`, `sav[eas]` (at least "sav"). `:w {path}` and
  `:w! {path}` parse, the path unescaped as `:e`'s is. `:sav` with no path is "E471: Argument required". `:wq {path}`
  and `:x {path}` stay unsupported. Update the lib.rs docs to match.
- `crates/fe-workbench/src/vim_host.rs`, the path resolved as `:e` resolves it (`resolve`):
  - `:w {path}` on an untitled buffer, and `:sav {path}`: save as, through `EditorView::save_as` (the Save As
    card), so the tab takes the new name.
  - `:w {path}` on a named buffer writes a copy: the buffer's text goes to `path` off the UI thread
    (`prepare_save_to`, not finished into the buffer), and the tab keeps its own path and dirty state, as in Vim.
    A path that is the buffer's own file is plain `:w`.
  - Either way, an existing target without `!` is "E13: File exists (add ! to override)" and nothing is written;
    with `!` it's overwritten.

## Tests

New files `crates/fe-vim/tests/ex_write_path.rs` and `crates/fe-workbench/src/vim_write_path_tests.rs`, the latter
declared from vim_host.rs (`#[cfg(test)] #[path = "vim_write_path_tests.rs"] mod write_path_tests;`).
## Rev 2 (Mac lead decision, 2026-10-15 note zu): widen the scope for an atomic no-clobber write

Review round 2 found that E13 is enforced by a check before the write, with a window between them. The decision is to
widen the scope rather than accept that:

- **Copy (`:w {path}`):** write a new file with create-new semantics (`O_CREAT | O_EXCL`, `OpenOptions::create_new`),
  so an existing file, including one made after any check, is refused with E13 with no check-to-write window. `:w!`
  overwrites as now.
- **Persist** (`SaveJob::run_with` in `crates/fe-editor/src/disk.rs`, which checks and then calls
  `temp.persist(&target)`): when the job must not clobber, commit with a rename that refuses an existing target:
  `renameat2(RENAME_NOREPLACE)` on Linux, `renamex_np(RENAME_EXCL)` on macOS, or link + unlink as the portable fallback
  (`tempfile`'s `NamedTempFile::persist_noclobber` is that fallback, and may be enough on its own). An existing target
  fails as E13 / an "exists" error, never a silent replace. Keep the macOS path `cfg`'d and simple: it is compiled only
  in the Mac gate.
- **Force through Save As** (`EditorView::save_as_with` in `crates/fe-gpui/src/pane_item.rs`, and its caller in
  `crates/fe-workbench/src/workbench.rs`): carry `!` through, so only a forced `:sav!` / `:w! {path}` overwrites;
  the workbench's save panel Save As (which has already asked about replacing) keeps overwriting.

Regression tests in new files: `crates/fe-editor/src/no_clobber_tests.rs` (declared as a `#[cfg(test)]` module from
disk.rs: a non-forced persist onto a target created after the job was prepared fails and leaves that file intact; a
forced one replaces it) and `crates/fe-workbench/src/vim_write_no_clobber_tests.rs` (`:sav` and `:w {path}` onto a
file created after the E13 check give E13 and leave it intact; with `!` they overwrite).

Scope now also covers the files the work already touched: fe-vim src/tests/ex_host.rs, fe-workbench vim_host_tests.rs
and git_commands_tests.rs.

## Rev 3 (neon lead, 2026-10-16): close review round 3's F2, F3 and F4

Rev 2 is implemented (create-new copy, no-replace rename in `SaveJob::run_with`, force through `save_as_with`, E13 on a
refusal). Round 3's remaining findings are fixed rather than overridden:

- **F2, a deterministic race test through host dispatch** (`crates/fe-workbench/src/vim_host.rs`): add a test-only
  seam between the E13 check and the write (a `#[cfg(test)]` hook, as `SaveJob::run_with`'s `before_commit` is), and
  use it in vim_write_no_clobber_tests.rs to create the destination after the check, through `:w {path}` and `:sav
  {path}` dispatched by the host: E13, file intact; with `!`, overwritten.
- **F3, a dangling symlink created after the preflight** (`crates/fe-editor/src/disk.rs`): when a non-forced job's
  requested path did not exist at the preflight, commit with the no-replace rename at the **requested path itself**,
  not at a resolved target, so anything that appears there afterwards (a file or a symlink, dangling or not) is
  refused. A path that existed (and is a symlink) at the preflight keeps today's resolve-and-update behaviour.
- **F4, a copy's destination vs project-panel renames** (`crates/fe-workbench/src/open.rs`): hold the copy's
  destination in the same reservation Save As uses (`open::reserve` / its pending-destination registry) until the
  write finishes, so a project-panel rename or other reserved operation can't move a file into it meanwhile.

Each gets a regression in the existing new test files (no_clobber_tests.rs for F3, vim_write_no_clobber_tests.rs for
F2 and F4). Nothing else outside the scope.
## Rev 4 (neon lead, 2026-10-17): close review round 3's F4 and F6

Review rounds ran out on rev 3 with two findings open. Fix both rather than override:

- **F6, Save As skips the EditorConfig save fixes** (`crates/fe-gpui/src/pane_item.rs`): Mac T481 (synced in at
  8dae005) applies `.editorconfig` trimming and final-newline fixes before an ordinary save (`apply_save_fixes`, in
  both the immediate and the queued branch of `EditorView::save`). `save_as_with` doesn't, so `:w` normalises a
  buffer but `:sav` saves it unchanged and marks it clean. Apply the fixes for the **destination** path in both of
  Save As's branches (immediate and queued), and add a test comparing an ordinary save with Save As under enabled
  EditorConfig properties.
- **F4, the copy reservation's lifetime is untested** (`crates/fe-workbench/src/project_panel_ops_tests.rs`, with
  the test seam in `vim_host.rs` or `project_panel_ops.rs`): add a test-only seam that pauses an actual `:w {path}`
  copy during its background write; while paused, a project-panel rename into its destination is refused, still
  refused after the originating tab (and its window) closes; then the copy finishes, and the reservation is released
  (a rename then succeeds).

Scope now also covers project_panel_ops.rs and project_panel_ops_tests.rs, which the rev 3 work already needed.
## Rev 5 (neon lead, 2026-10-17): how to close F4 and F8

Rev 4's behaviour is in. These are the tests the review still wants, made concrete:

- **F4, pausing the copy's own background write:** add a test-only pause point inside fe-gpui's copy job (the
  `write_copy` background task in `crates/fe-gpui/src/pane_item.rs`) that fe-workbench tests can reach: a
  `#[doc(hidden)] pub` hook (e.g. a process-wide slot holding an optional gate the job waits on before writing). No
  Cargo feature or change is needed. Use it in project_panel_ops_tests.rs: pause the write; a project-panel rename into
  the destination is refused, still refused after the originating tab and window close; release the write; the copy
  completes. Check the reservation is **released** by its registry (`open::reserved_paths()` no longer lists the
  destination, or a fresh `open::reserve` of it succeeds), not by renaming into the now-existing destination.
- **F8, Save As and EditorConfig coverage:** add tests for the save panel's Save As (not only `:sav`), and for a
  queued Save As whose source and destination have different `.editorconfig` settings (the destination's apply).

## Implementation notes

- A copy (`:w {path}` on a named buffer) goes through `EditorView::write_copy`, queued with the buffer's other saves.
- `SaveJob::run_with` commits a non-overwriting job whose target was missing at its check with
  `persist_noclobber`, so a file created since is kept (`ChangedOnDisk`). Copy and Save As both use it.
- `prepare_save_as` / `save_as_with` / `Workbench::save_editor_as_with` take `overwrite`; the save panel and
  `save_editor_as` pass true, `:sav` and `:w {path}` pass `!`. A refusal there is E13, not a prompt.
- Rev 3: a non-forced job whose path held nothing (not even a symlink) at its check commits at the path itself with the
  no-replace rename, so a late file or dangling symlink is kept. A copy reserves its destination (`open::reserve`)
  until written. `vim_host` has a `#[cfg(test)]` hook (`AFTER_CHECK`) between the E13 check and the write.
- Round 4: a job prepared for a path that held no file (`expected == Some(None)`) commits at the path itself, so a
  dangling symlink made between the host's check and the job is refused too. The project panel's rename also asks
  `busy_under` about the new path (`project_panel_ops.rs`, and its test in `project_panel_ops_tests.rs`, both outside
  the listed scope), so a rename into a held copy or Save As destination waits.
- Round 5: `SaveJob` carries `new_entry` (set by `prepare_save_to` for a destination that isn't the buffer's own file)
  instead of reading `Some(None)` as "absent", so an ordinary save through an existing dangling symlink still creates its
  referent. A copy queued behind a running save holds its destination (project-panel test
  `rename_waits_for_a_pending_copy`); the reservation is owned by the copy's task, not the tab.
- Rev 4: `save_as_with` takes the destination's `SaveFixes` (read from its `.editorconfig` by `save_editor_as_with`) and
  applies them before preparing the snapshot, in the immediate and the queued branch
  (`save_fixes_tests::save_as_fixes_like_save`). `rename_waits_for_a_pending_copy` now queues the copy behind a held
  save, closes its tab and window with `:q!`, and attempts the rename from another window's panel: still refused until
  the copy is written, then the reservation is released. No separate pause seam in the background write was added: the
  copy waits in the buffer's save queue, which holds the same reservation.
- A copy refuses a destination another buffer's pending Save As or copy holds ("is being saved by another tab").
  `save_as_applies_the_destination_editorconfig` drives `:sav` against a real `.editorconfig`, at once and queued, and
  compares with `:w`.
- Rev 5: `fe_gpui::pane_item::pause_next_copy` (`#[doc(hidden)]`) holds the next `write_copy` just before it writes;
  `rename_waits_for_a_pending_copy` uses it, closes the tab and window, then releases the copy and checks the registry
  (`open::reserved_paths()`, a fresh `open::reserve`) rather than renaming into the now-existing file. The save
  panel's Save As (`save_as_tests::save_as_applies_the_destinations_editorconfig`) and `:sav`
  (`write_path_tests::save_as_applies_the_destination_editorconfig`) are tested with a source without fixes and a
  destination with them, at once and queued behind a save that writes the text as typed.
