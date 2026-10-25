---
task_uid: "01m5rpbqexvrxhjb4sg8dk4pnx"
task: "T337"
rev: 5
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "vim_buffers_ownership_tests::a_pane_closed_before_a_read_replays_into_its_own_window: with two workbench windows, a pane added and closed in window B inside one update, before any registry read, never takes a buffer number in window A's list, and B's alternate buffer is right after the replay; fails at base"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Tag pending vim buffer transitions with their owning workbench so a pane added and closed between registry reads replays into its own window (T333 F2)
