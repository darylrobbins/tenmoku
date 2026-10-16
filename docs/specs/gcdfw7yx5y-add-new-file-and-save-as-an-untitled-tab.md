---
task_uid: "01m50ew2d1fz199ygcdfw7yx5y"
task: "T113"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test open::save_as_tests::new_file: cmd-n opens an empty untitled tab, focused, in the active pane; the File menu lists New File and Save As… with cmd-n and cmd-shift-s"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test open::save_as_tests::save_as_moves_the_tab: with the save panel answering dir/b.py for an open, edited a.rs, cmd-shift-s writes b.py, leaves a.rs as it was on disk, and the tab's path, title and language (Python) follow, clean"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test open::save_as_tests::untitled_cmd_s: cmd-s on an untitled tab shows the save panel and saves where it answers; cancelling leaves the tab untitled and dirty; a path open in another tab is refused with a prompt"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test open::save_as_tests::lsp_follows: with the fake server, saving a.rs as c.rs sends didClose for a.rs's URI and then didOpen for c.rs's"
    required: true
    manual: false
  - id: "C5"
    text: "fe-keymap one_keymap and menu tests pass, and fe-gpui command_tests::table_is_the_only_binding_source passes with its count updated"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench regression (F10): with a Save As from a.rs to an existing, closed b.rs held pending, a closed-file workspace edit to b.rs is refused and b.rs on disk is unchanged; after the save runs, b.rs holds a.rs's text"
    required: true
    manual: false
  - id: "C7"
    text: "fe-workbench regressions (F9): an ordinary save held, then Save As to a CRLF-configured b.txt and an LF-configured c.rs queued behind it, writes b.txt with CRLF and c.rs with LF; a failed Save As leaves the source buffer's line endings and dirty state as they were"
    required: true
    manual: false
---

# Add New File and Save As: an untitled tab from cmd-n, and a save panel from cmd-shift-s or from cmd-s on an untitled tab (Mac T434)

## Problem

There's no way to make a new file or save under another name. No command or key does either (the keymap's only save
is `secondary-s` → `editor.save`; the File menu, `crates/fe-keymap/menus/fastedit.toml`, has Open Folder, Open File
and Save), and cmd-s on an untitled tab fails with "buffer has no path" (`crates/fe-editor/src/lib.rs:206`), logged
only. The 2026-10-11 gap analysis (M6 "Save As and new untitled file") found no card. User 2026-10-11: "Draft any
work in earlier phases we haven't drafted yet."

## Change

- fe-editor (`crates/fe-editor/src/disk.rs`): `EditorModel::set_path(Option<PathBuf>)`, shared by split models
  (store the path beside the shared stamp, so every view of the buffer follows), and
  `prepare_save_to(path, overwrite)` building a `SaveJob` for another target.
- `EditorView::save_as(path, window, cx) -> Task<gpui::Result<()>>` (`crates/fe-gpui/src/pane_item.rs`): saves to
  `path` off the UI thread with `overwrite = true` (the save panel already asked about replacing), then `set_path`,
  re-detects the language from the new path (`fe_syntax::detect_language_name`, as `EditorView::new` does at
  `crates/fe-gpui/src/view.rs:201`, applied with `set_language`) and notifies, so the tab, title and breadcrumbs
  follow. The old file is left as it was. A path open in another tab (not a split of this buffer) is refused with
  an error prompt "“<name>” is open in another tab".
- Commands in `crates/fe-workbench/src/open.rs` (new `OpenOp::NewFile` and `OpenOp::SaveAs`, context
  `workbench`), handled where the other open ops are in `crates/fe-workbench/src/workbench.rs`:
  - `workbench.new_file` "New File": adds `EditorModel::new(Buffer::new(""), None)` to the active pane, focused.
  - `workbench.save_as` "Save As…": `cx.prompt_for_new_path` in the editor's folder (else the project root, else
    home) with its file name (else "untitled") suggested, then the active editor's `save_as`. Cancel changes nothing.
  - cmd-s on an untitled editor runs `workbench.save_as` instead of failing: the view's `editor.save` override
    (previous card) dispatches it when the model has no path.
- LSP (`crates/fe-workbench/src/lsp.rs`, the sync near :800 that keys documents by buffer): a buffer whose path
  differs from its document's is closed (`didClose` for the old URI) and opened under the new path, as a language
  change already is.
- Keys in `crates/fe-keymap/keymaps/fastedit.toml`, `default-macos.toml`, `default-linux.toml`: `secondary-n` →
  `workbench.new_file`, `secondary-shift-s` → `workbench.save_as`, context `workbench`. Menus in
  `crates/fe-keymap/menus/fastedit.toml`, `default-macos.toml`, `default-linux.toml`: File gets "New File" first and
  "Save As…" after "Save". Update `crates/fe-gpui/src/command_tests.rs`'s binding count.

## Tests

New file `crates/fe-workbench/src/save_as_tests.rs`, declared from open.rs
(`#[cfg(test)] #[path = "save_as_tests.rs"] mod save_as_tests;`). Answer the save panel through GPUI's test platform,
as `crates/fe-workbench/src/tests/open_folder.rs` answers the open panel, and use the fake LSP server of the
`tests::lsp*` suites.
## Rev 2 (neon lead, 2026-10-15): fix review round 3's F9 and F10

Review rounds ran out with F10 (high) and F9 (medium) open. Fix both rather than override:

- **F10, pending Save As destinations vs workspace edits** (`crates/fe-workbench/src/workspace_edit.rs`): a Save As
  queued to an existing, closed file reserves its destination only when the save runs, so a closed-file workspace edit
  to that destination applied in between succeeds and the `overwrite = true` Save As then silently replaces it.
  Closed-file workspace edits must treat a pending Save As destination (by its resolved file identity, as
  `opened_files` does for open buffers) as open, and refuse, leaving disk unchanged.
- **F9, destination serialization settings per queued save** (`crates/fe-workbench/src/workbench.rs`): settings for a
  Save As destination (e.g. line endings) are applied when the save is requested, but a queued job captures its
  snapshot later, so a later queued Save As's settings can replace an earlier one's, and a failed Save As leaves the
  source buffer's settings changed and dirty. Carry the destination's settings with each queued save, apply them when
  that job captures its snapshot (ignoring stale configuration callbacks), and leave the buffer's committed settings
  as they were if the save fails.

Scope now also covers the files the work already needed: fe-editor lib.rs, fe-gpui view.rs and keymap_tests.rs,
fe-keymap tests/menu.rs, fe-workbench pane.rs, lsp_format.rs, lsp_format_tests.rs, workspace_edit.rs and
workspace_edit_tests.rs.

## Implementation notes

- The shared path is a chain of links (`fe_editor::disk::PathLink`): `set_path` links a new one after the latest, and
  every model reads the latest from the link it holds, through `&self`, so `EditorModel::path` and
  `PaneItem::path` keep returning `Option<&Path>` and a split sees the new path at once (even an inactive tab that
  never redraws). `sync_display` lets go of the links behind it. The stamp lives in `fe_buffer::Buffer`, which can't
  lend a `&Path` out of its `RefCell`, hence not literally beside it.
- `prepare_save_to` to a file other than the buffer's own expects it not to exist unless `overwrite`.
- The "open in another tab" refusal is in the workbench (`Workbench::save_editor_as`), which can see the other tabs;
  it also refuses a path a pane or the host is reading. After a save under another name the workbench has every
  pane re-read its tabs' paths (`Pane::sync_paths`, a small addition to `pane.rs`, outside the listed scope, since
  `Tab::path` is cached per tab) and the buffer's other editors re-detect their language
  (`EditorView::detect_language`, also what `EditorView::new` now calls).
- cmd-s on an untitled buffer dispatches `workbench.save_as` from `EditorView::save_from_command`, in the window's
  context stack; vim's `:w` on an untitled buffer still fails as before.
- `crates/fe-keymap/tests/menu.rs` expects the new File items.
- Carried T83 F4: `Workbench::launch_reading` returns the path only while its pane is in the pane group;
  regression `workspace_edit::tests::the_launch_read_after_its_pane_closed`.
- After review: the buffer takes the new path only once the file is written, in the turn the save commits (Saves
  queued for a buffer run in order, so paths change in the order asked, and a failure changes nothing). Each Save As
  destination is held from the panel's answer until the workbench has taken in the outcome (`open::reserve`, a
  thread-local registry beside `pane_item`'s save queue), whatever happens to the tab: another buffer's Save As there is
  refused, and `Pane::open_at` of it waits, then opens (the saved tab, if it went through). Conflicts compare
  `open::file_identity`: the target `fe_editor::save_target` (the save's own symlink resolution, dangling links too)
  resolved through `..`. `EditorView::save_as_with` calls a hook just before the commit, with which the workbench
  closes the LSP document under the old name (`Workbench::lsp_release`), so the commit's save reaches no document and
  saves queued before it reach the old one. The destination's `.editorconfig` line ending is applied before the text
  is taken (`pane::apply_save_config`), read on the UI thread so Save As requests keep their order. A save to the
  buffer's own path checks the disk stamp only while the stamp is of that file (`History::stamped` in fe-editor).
  Workspace edits for an open buffer and formatting results are refused or dropped if the buffer's path changed
  since the request. EditorConfig properties are cached per editor and path: a tab whose path changed goes back to
  the settings' tab size and its detected indentation, then takes the new file's properties.
- Rev 2: a pending Save As destination (path and identity) counts as opened for closed-file workspace edits
  (`Workbench::opened_files`), and project-wide replace waits for it as for a file being read (`being_read` in
  `search_replace.rs`, the same defect). The destination's `.editorconfig` line ending is no longer applied to the
  buffer when Save As is asked for: it goes with that save (`EditorView::save_as_with`), its job's snapshot is taken
  with it (`EditorModel::prepare_save_as`, the buffer's own line ending left as it was), and the buffer takes it only
  when the save commits (`EditorModel::finish_save_as`). A failed save leaves the line ending and dirty state alone.
  Regressions: `workspace_edit::tests::an_edit_to_a_pending_save_as` (C6),
  `open::save_as_tests::queued_save_as_line_endings` and `open::save_as_tests::a_failed_save_as_keeps_line_endings`
  (C7).
- Rebased onto T115 (format on save) and T119 (viewer tabs): `EditorView::save` keeps T115's save hook, run in turn
  before the job is taken; Save As doesn't run it (the formatter is the old file's document's, and the language may
  change with the name). The "open in another tab" refusal covers viewer tabs too, and `Pane::sync_paths` reads each
  tab's path through its item.
- Review after the rebase: the "open in another tab" refusal looks at every window's workbench (tabs, reads, the
  launch read; `Workbench::uses_file`), the reservations being app-wide already. A Save As whose file another writer
  replaced between the write and the commit isn't recorded (`Saved::is_current`, checked before the hook and again by
  `EditorModel::finish_save_as`, which then changes nothing): the buffer keeps its path, stamp and line ending, and
  the save fails with "<name> was replaced while it was being saved". The tabs and splits follow only a Save As that
  went through, so a failed one leaves a split's chosen language alone.
