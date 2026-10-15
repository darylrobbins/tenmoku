---
task_uid: "01m50ew6b61mbzpxv4d0ex6z1x"
task: "T119"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test pane::large_file_edit_tests::edit_this_file_converts_in_place: EditRequested from a viewer tab reads the file on the background executor, then replaces the tab at the same index with an editor holding the file's text, its path set and the caret at the viewer's head"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test pane::large_file_edit_tests::changed_files_are_refused: a file changed on disk before the request, or while it is read, isn't converted; the viewer stays, its banner saying the file changed"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test pane::large_file_edit_tests::large_conversions_ask_first: above the (lowered) warning threshold a prompt asks first; Cancel keeps the viewer and confirming converts it"
    required: true
    manual: false
---

# Turn a read-only viewer tab into an editor with Edit This File, loaded off the main thread and refused if the file changed (Mac T399)

## Problem

A file of 512 MiB or more opens in a read-only viewer tab (the previous card). The decision
(docs/decisions/mmap-read-only.md:124-131) makes editing it an explicit conversion: "Edit this
file", with a size warning above ~1 GB, run off the UI thread, and refused if the file changed on
disk. The conversion costs what opening as a rope costs: 55 ms at 100 MB, 544 ms and 2.6 GB at
1 GB, 8.4 s and 8 GB at 4 GB. Nothing offers it.

## Change

- `crates/fe-gpui/src/file_viewer.rs`: the banner gets an **Edit This File** button that emits a
  new `FileViewerEvent::EditRequested`. While a conversion runs the host can set the banner text and
  disable the button (`FileViewer::set_converting(bool)`).
- `crates/fe-workbench/src/pane.rs`, on `EditRequested` from a viewer tab:
  1. Over 1 GiB, ask first with the window's prompt: "Edit <name>? It loads all <size> into memory,
     which takes several seconds and about 1.5 times its size." Cancel keeps the viewer.
  2. Refuse if `FileView::check()` isn't fresh: the banner says the file changed on disk and to
     reload it first.
  3. Read it with `loader::read` on the background executor, banner "Loading <name> for editing…".
     Check freshness again when the read finishes and refuse the same way if it changed meanwhile.
  4. Replace the viewer tab in place (same index, same pane, active if it was) with an editor on
     the buffer, its path set, the caret at the viewer's head offset (clipped), scrolled to it.
     A read error leaves the viewer and shows the error in its banner.
- A test-only setter lowers the 1 GiB warning threshold, as the previous card's does for 512 MiB.

Tests in the new file `crates/fe-workbench/src/large_file_edit_tests.rs`, declared from `pane.rs`
with `#[path]`.
