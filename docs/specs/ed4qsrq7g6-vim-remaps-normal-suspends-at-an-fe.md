---
task_uid: "01m59sr9e1ty1f3aed4qsrq7g6"
task: "T226"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim integration test ex_normal_resume::an_fe_piece_in_the_middle_of_normal_suspends_and_resumes passes: with a host that completes an fe: command later, :normal ifoo\u003cEsc\u003efe:x\u003cCR\u003eibar\u003cEsc\u003e runs the keys after the fe: piece only after it completed, and over a range each row resumes in order; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-vim -p fe-gpui passes with no existing test edited; cargo clippy -p fe-vim -p fe-gpui --all-targets -- -D warnings is clean; no Cargo change"
    required: true
    manual: false
---

# Vim remaps: :normal suspends at an fe: piece and resumes with its remaining keys and range iterations (T220 review F3)

## Problem

T220 (Mac T876, `<Cmd>...<CR>` in a vim remap) ships with a known limit (its review finding F3, `crates/fe-vim/src/ex/normal.rs:~185`): the `:normal` runner recognises `fe:` command tokens but runs them immediately instead of suspending: a `:normal` whose keys contain an `fe:<command id>` piece must suspend at that piece and resume with the remaining keys, and with the remaining range iterations, after the command has run.

## Change

Make the `:normal` runner resumable (a state machine over its keys and its range iterations) so an `fe:` piece can suspend and resume it. New tests in a new file declared from a test file: `:normal` with an `fe:` piece in the middle of its keys, and over a range.
