---
task_uid: "01m4s3rjnxvdgpdqess4jcjr0m"
task: "T94"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test file_viewer::find_tests::next_and_previous: with a query set, find_next selects the next match after the selection and scrolls it into view, find_prev the previous one, each emitting Found with wrapped false, and past the last match Found with wrapped true; a query with no match emits NotFound and leaves the selection"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test file_viewer::find_tests::a_newer_search_stops_the_last: a second find_next, set_find(None) or a reload while a search runs stops it, and only the newest search lands; is_finding is true while one runs and false after"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test file_viewer::find_tests::only_indexed_rows: while the index is still building, a match past the rows found so far is not selected"
    required: true
    manual: false
  - id: "C4"
    text: "file_viewer/tests.rs and file_viewer/pane_item_tests.rs pass unchanged"
    required: true
    manual: false
---

# Find in a FileViewer: set_find, find_next and find_prev search off the main thread, select and scroll to the match, and tell the host found, wrapped or not found (Mac T569)

## Problem

`FileViewer` (`crates/fe-gpui/src/file_viewer.rs`), the read-only view of a file too large for a buffer, can
scroll, select and copy but not find. `EditorView` has a host-facing find (`EditorView::set_find`, stepping and
counting off the main thread, in `crates/fe-gpui/src/view/find.rs`) that the workbench's find bar drives. The
viewer needs the same seam over `fe_fileview::find` (the card this depends on), so that once the workbench shows
viewer tabs (Mac T398) its find bar can drive a viewer too. Searching a multi-GiB file takes seconds, so it must
run off the main thread and be stoppable.

## Change

In `crates/fe-gpui/src/file_viewer.rs`, plus a new submodule file `crates/fe-gpui/src/file_viewer/find.rs` for the
state and task (declared from `file_viewer.rs` as `pane_item` and `element` are):

- `pub fn set_find(&mut self, query: Option<fe_fileview::FindQuery>, cx)`: stores the query (`None` clears it) and
  stops any search running.
- `pub fn find_next(&mut self, cx)` and `pub fn find_prev(&mut self, cx)`: with a query set, stop any search
  running and start `fe_fileview::find` on `cx.background_spawn`, forward from the selection's end or backward from
  its start, `within` `0..` the current `FileText`'s length (so while the index is building only the rows found so
  far are searched). Each search has its own `Arc<AtomicBool>` stop flag; a newer call, `set_find`, a reload and
  dropping the viewer set it.
- When a search lands (and is still the current one): `Found` selects the match (anchor at its start, head at its
  end), scrolls it into view as a click-and-drag selection does (`autoscroll`), and emits
  `FileViewerEvent::Found { wrapped: bool }`; `NotFound` leaves the selection and emits `FileViewerEvent::NotFound`;
  `Stopped` does nothing; a read error is reported the way the viewer reports other read errors (the existing
  `Changed` path).
- `pub fn is_finding(&self) -> bool` while a search runs, so a host can show progress.

No keys or commands are added here: the find bar and its keys stay the workbench's, wired in a later card, so
nothing collides with vim or nvim conventions.

## Tests

New file `crates/fe-gpui/src/file_viewer/find_tests.rs`, declared `#[cfg(test)] mod find_tests;` from
`file_viewer.rs` beside `tests` and `pane_item_tests`, with temp files written by the test and a headless window as
`file_viewer/tests.rs` builds one (copy its helpers; don't edit it).

Lead note (rev 2): C2's cancellation must be shown against a scan that is provably still running. The synchronous `fe_fileview::find` runs a whole scan inside one test-executor tick, so a test that cancels after a tick checks nothing. Add a `#[cfg(test)]` pause point in `file_viewer/find.rs` (for example, a barrier or channel the scan waits on after it starts). In the test, wait until the scan has begun and is held, then replace, clear and reload in turn. After each, check that the captured stop flag is set, that the scan ends because of the cancellation, and that no stale result is applied. Keep the production path free of the hook.
