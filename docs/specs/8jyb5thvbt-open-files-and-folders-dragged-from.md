---
task_uid: "01m4rqekd6f6akmw8jyb5thvbt"
task: "T57"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "external_drop_tests::files: dropping two files on the second of two panes opens both there in order, the last active and focused; a file already open there is activated, not reopened"
    required: true
    manual: false
  - id: "C2"
    text: "external_drop_tests::folder: dropping a folder (with a file) re-roots the window at the folder and then opens the file; with two folders only the first opens"
    required: true
    manual: false
  - id: "C3"
    text: "external_drop_tests::vim: with vim on, the dropped file's editor is focused in normal mode"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Open files and folders dragged from Finder onto the window: files as tabs in the pane under the drop, a folder as the project (Mac T516)

Filed on helium from Mac T516 (~/coordination/helium-cards/T516.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-x.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium notes (Mac note x, stream 2, Open Recent): helium is Linux. Where the card says Finder, read the file manager; test drops through GPUI's external-paths drop events in a gpui test, not a real drag. Paths, config locations and the recent list follow fe-config's existing per-platform rules (XDG on Linux).

## Problem

Dragging files or folders from Finder onto the window does nothing: nothing in fe-workbench handles GPUI's
`ExternalPaths` (day-one audit 2026-10-12; T119 is tab dragging only).

## Change

New `crates/fe-workbench/src/external_drop.rs`, declared from `crates/fe-workbench/src/workbench.rs`, whose root
element gets `on_drop::<ExternalPaths>` and a drag-over highlight (`drag_over::<ExternalPaths>`, a translucent
accent overlay):

- Files open as tabs in the pane under the drop point (else the active pane), in the order dropped, the last one
  active and focused; a file already open there is activated, not reopened.
- A folder re-roots this window, as Open Folder does (`Workbench::set_root`); with several folders only the first
  is opened. Files dropped with a folder open after it.
- Anything else (a missing path, a socket) is skipped and logged.
- Vim: a dropped file opens focused in vim normal mode.

## Tests

New file `crates/fe-workbench/src/external_drop_tests.rs`, declared from external_drop.rs, simulating the drop
with GPUI's test platform (`FileDropEvent`) on temp files.

## Implementation notes

- The drop target is an absolute, non-occluding overlay over the whole window (not the root div): an editor's mouse-up otherwise swallows the drop. It carries `on_drop::<ExternalPaths>` and the `drag_over` accent wash.
- Files open one at a time, each once read, so tabs follow the order dropped even though reads finish in any order; the last is active and focused.
- The tests move the pointer over the window before the drag, as a real drag does; the first mouse event after a split is otherwise not delivered to the overlay in the test window.
- The batch stops if its pane leaves the workbench, or a file doesn't land in it (a failed read, or something else opened in the pane and took over). With vim on, the last file's editor is sent escape if it isn't in normal mode (an editor already open).
- Each dropped file's editor is normalised as it lands, and a batch also stops if the pane's open serial moves (another open superseded it). A read already in flight in a pane that closes is `Pane`'s own (pane.rs is out of this card's scope); the batch just stops queuing.
