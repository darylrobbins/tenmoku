---
task_uid: "01m4rff3dbgfg00yra4p63se5q"
task: "T495"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "counted_search_is_bounded no longer depends on wall-clock time: under load ~90 (T416's post-merge check, run 01m4rdtq9ecqd2tsfyv8g56n62) it took 16.7s and failed at search.rs:269, consistent with a search exceeding fe-vim's DEFAULT_TIME_LIMIT (2s) and returning no match. Fix in the test (e.g. a generous limit via set_time_limit for its searches, restored afterwards), keeping its ROWS_SEARCHED bound check intact. If the cause is not the time limit, block with what you found."
    required: true
    manual: false
  - id: "C2"
    text: "Proven by 20 repeated runs of the test passing while the machine is under load (e.g. alongside a full workspace nextest run); record the command and result in the evidence."
    required: true
    manual: false
  - id: "C3"
    text: "Nothing outside crates/fe-vim/src/tests/search.rs changes, except this lane's own docs/specs file."
    required: true
    manual: false
---

# Flake: fe-vim counted_search_is_bounded fails under heavy load (search hits its 2s time limit)
