---
task_uid: "01m4srcfvykp3x3f428y36cddd"
task: "T78"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "FileViewer::refresh installs a snapshot only when it has no read error and at least the rows shown; otherwise the rows, selection and model stay as they were, polling stops, and FileViewerEvent::Changed is sent once (file_viewer::tests::a_refresh_that_finds_fewer_rows_keeps_the_last_snapshot)"
    required: true
    manual: false
  - id: "C2"
    text: "An index that stops with IndexState::Changed while the viewer polls leaves the rows already shown and sends Changed once (file_viewer::tests::an_index_that_changes_while_polling_keeps_its_rows)"
    required: true
    manual: false
  - id: "C3"
    text: "Every existing fe-gpui file_viewer test passes unchanged"
    required: true
    manual: false
---

# Keep a FileViewer's last snapshot when a refresh reads fewer rows or fails, instead of panicking in DisplayMap::replace_text (T274 review F1) (Mac T618)

Filed on argon from Mac T618 (card ~/coordination/argon-cards/T618.json rev 1). Mac task uid 01m4srbrxvnzvqc98jh46f42eb. No dependencies (a review follow-up of Mac T274, F1). Title the host commit "... (Mac T618)".

## Problem

Finding F1 from the codex review of T274 (approved, follow-up, never filed). It is still on main at c65979ea.

`crates/fe-gpui/src/file_viewer.rs` `FileViewer::refresh` installs a new snapshot before it checks it:

```rust
self.text = FileText::new(self.view.clone(), self.index.clone());
self.model.refresh(Arc::new(self.text.clone()));
```

`FileText::new` (`crates/fe-fileview/src/source.rs`) reads `index.progress()` and then calls
`index.row_start(progress.rows)`. Suppose the scan moves past `progress.rows` between those two calls. Then
`row_start` walks from a checkpoint through an uncached read. If the file was truncated meanwhile, that read
fails, the error is recorded, and the snapshot keeps `last_row = 0`: one row. `ViewerModel::refresh`
(`crates/fe-editor/src/viewer.rs`) passes that snapshot to `DisplayMap::replace_text`
(`crates/fe-display/src/display_map.rs`), which asserts `text.row_count() >= ours.row_count()`. Debug builds
panic. Release builds drop every row shown so far, and the selection with them. `check_index` sees the error
only after the snapshot is installed.

The constructor (`FileViewer::new`) and the reload path build a new `ViewerModel` rather than refreshing one,
so only `refresh` has this problem.

## Change

- In `FileViewer::refresh`, build the `FileText` in a local first. Install it (`self.text`,
  `self.model.refresh`, `self.generation += 1`, `put_back`) only when it has no `error()` and its
  `row_count()` is at least the current snapshot's. Otherwise keep the current snapshot and model unchanged,
  stop polling (`self.poll = None`), and call `mark_changed`, which already sends
  `FileViewerEvent::Changed` only once and offers a reload in the banner.
- The rest of `refresh` stays as it is: the `building` read before the snapshot, the `check_index` before
  it, and the poll returning whether to go on.
- Tests go in `crates/fe-gpui/src/file_viewer/tests.rs`, which is already declared at base. This is
  `FileViewer`'s child module, so it can reach `refresh` and the `index` field. The race happens inside
  `FileText::new` and can't be timed from a test, so the regression test feeds `refresh` the snapshot the
  race would produce. It steps a `LineIndex::build_stepped` index far enough that the viewer shows more than
  one row, then replaces `viewer.index` with a fresh stepped index of the same view that hasn't scanned
  anything (a one-row snapshot), and calls `refresh`. At base this panics in `replace_text`'s debug assert.
  The second test truncates the file under a stepped index, steps it to `IndexState::Changed`, and checks
  that the poll that sees it keeps the rows shown and sends `Changed` once.
