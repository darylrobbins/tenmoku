---
task_uid: "01m796pgrj01px17s906khnsx0"
task: "T595"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "a_check_stops_within_one_chunk_of_the_share_end and a_share_that_ends_mid_check_refuses_without_suspending_the_grant still pass on Linux and Mac"
    required: true
    manual: false
  - id: "C2"
    text: "no append(true) followed by set_len remains in kiln-plugin tests"
    required: true
    manual: false
---

# kiln-plugin tests: open big programs with write(true) so set_len works on Windows (Mac T1760)

Windows CI: `a_check_stops_within_one_chunk_of_the_share_end` (kiln/crates/kiln-plugin/src/tests/prelaunch.rs:485,
helper `big_program` ~line 478) and `a_share_that_ends_mid_check_refuses_without_suspending_the_grant`
(kiln/crates/kiln-plugin/src/tests/program_budget.rs:217) fail with "Access is denied". Both do
`OpenOptions::new().append(true).open(path)...set_len(..)`; on Windows `set_len` needs write access
(append-only handles lack FILE_WRITE_DATA).

Fix (test-only): `.write(true)` instead of `.append(true)` in both places. set_len keeps the file's start
and extends with zeros, same on every OS. Fix both call sites (they are the only two `append(true)` uses in kiln-plugin).

Runs unchanged on Linux/Mac (they pass today and must still); only Windows CI proves the fix.
