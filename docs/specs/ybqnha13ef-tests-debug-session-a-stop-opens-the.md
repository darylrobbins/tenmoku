---
task_uid: "01m7hd4y1pcchxxeybqnha13ef"
task: "T398"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "a_stop_opens_the_file_at_the_stopped_row_and_announces_it waits (debug.until or the fixture's equivalent) for the fake adapter to receive setBreakpoints before asserting on it, instead of asserting right after fake(0) returns"
    required: true
    manual: false
  - id: "C2"
    text: "The fe-workbench tests::debug_session tests pass 30 repeated runs under load (e.g. a parallel cargo build), with no change outside the test file"
    required: true
    manual: false
---

# tests::debug_session::a_stop_opens_the_file_at_the_stopped_row_and_announces_it flakes: it asserts setBreakpoints right after fake(0), which waits for the launch only, not the configuration requests (Mac T1886)
