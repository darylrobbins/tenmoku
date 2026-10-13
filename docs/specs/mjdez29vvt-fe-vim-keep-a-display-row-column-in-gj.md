---
task_uid: "01m4sfxb2yrp548tmjdez29vvt"
task: "T101"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "After gj/gk from a caret inside a tab that is wider on its display row than in the buffer, a return gk/gj (e.g. (1,5) 2gk2gj) lands on the same display column, shown by unit tests in crates/fe-vim/src/tests/display_goal.rs."
    required: true
    manual: false
  - id: "C2"
    text: "Every other goal update (j, k, horizontal motions, edits) clears the display-row column; the j/k goal behaviour is unchanged."
    required: true
    manual: false
  - id: "C3"
    text: "T416's nvim corpus and all existing fe-vim tests pass unchanged; nothing outside the scope changes except this task's own docs/specs file."
    required: true
    manual: false
---

# fe-vim: keep a display-row column in gj/gk goals so a caret inside a wide wrapped tab returns to its display column (Mac T493)

Follow-up of T416 (review round 5, F14 remainder), accepted there as a known limit by the lead.

fe-display expands tabs from each display row's start, so on a wrapped row a tab can take more display columns than buffer columns (4 vs 1 in the reviewer's example). gj/gk keep their goal as a single buffer column (state.goals: Vec<(usize, Option<usize>)>, shared with j/k), so a caret at display column 5 of such a tab cannot be told from one at column 7: (1,5) 2gk2gj comes back at (1,7).

Change: keep an optional display-row column next to each caret's goal, set and read only by gj/gk and cleared by every other goal update (j/k, horizontal motions, edits). nvim has no per-display-row tab stops, so there is no oracle for this case: test it with unit tests on wrapped rows with wide tabs. Behaviour of every case T416's nvim corpus covers must not change.
