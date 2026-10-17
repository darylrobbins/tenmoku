---
task_uid: "01m53maam8pfdv2yt2vj28fvqx"
task: "T729"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "A rootless window with a dirty untitled buffer quits without a prompt, and the next no-argument launch shows it dirty with its text and caret (new test in crates/fe-workbench/tests/hot_exit_rootless.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "With backups written but no quit (the workbench dropped), the next no-argument launch restores the rootless window's editors; a folder launch doesn't take them"
    required: true
    manual: false
  - id: "C3"
    text: "Folder windows behave as after T509; cargo test -p fe-workbench -p fastedit and clippy -D warnings pass"
    required: true
    manual: false
---

# Hot exit for rootless windows: a no-folder launch backs up and restores its untitled and dirty buffers like a folder window

## Problem

T509 (hot exit) backs up dirty and untitled buffers and restores them with the window's workspace layout.
A rootless window (fastedit launched with no folder argument) has no WindowLayout: `crates/fastedit/src/main.rs`
gives it none, and hot exit is initialised only through `Workbench::set_layout`
(`crates/fe-workbench/src/workbench.rs`). So a rootless window's untitled buffers still ask at quit and are lost
in a crash. T509's review finding F5 found this; T509 carved it out to this card.

## Change

Give rootless windows a layout and backup owner of their own, kept under the app's state directory, not a
workspace's. Initialise hot exit for them at launch, and restore their editors on the next no-argument launch.
Follow T509's store and pruning rules (`crates/fe-workbench/src/hot_exit.rs`), including keeping backups whose
recovery fails.

## Implementation notes

- `fe_workbench::window_layouts(dir, root)` is the layout store for a window on `root`, or with no root the
  rootless windows' store, under the key of the empty path (which no canonical root has). Hot exit's backups
  sit beside it as for any store, at `<state>/backups/<key>`, so a folder launch never sees them.
- `main.rs` uses it for every window: a no-argument launch restores the rootless store's saved windows
  (`kiln::shell::layout::launch` with restore on), and `window.new` with no root takes its newest free entry.
  Hot exit then starts through `Workbench::set_layout` as for a folder window, with T509's orphan, pruning
  and keep-on-failure rules unchanged.
- A rootless window whose restored layout leaves it with no tabs shows the welcome text, as a launch with
  nothing saved does; otherwise the first no-argument quit would lose it for good.
- The integration tests can't open a folder window (the project watcher's thread trips gpui's test
  scheduler), so "a folder launch doesn't take them" is checked at the store level there and by
  `hot_exit::tests::folder_leaves_rootless_backups`, which opens a folder window beside rootless backups.
