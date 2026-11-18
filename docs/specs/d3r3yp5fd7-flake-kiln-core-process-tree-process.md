---
task_uid: "01m7q9d7gk5s6ew9d3r3yp5fd7"
task: "T425"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "In kiln/crates/kiln-core/tests/process_tree.rs no leader_and_worker test can see the leader SIGKILLed only because the machine is loaded, and each test still asserts what it did before (the program's own exit status, the worker killed only after the grace, stdout closed)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p kiln-core --test process_tree passes 30 times in a row while the machine is under load (for example alongside a cargo build), recorded in the decision file"
    required: true
    manual: false
  - id: "C3"
    text: "cargo clippy -p kiln-core --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Flake: kiln-core process_tree::process_tree_unix_what_the_program_left_is_killed_when_the_grace_is_up fails under load (the leader script misses the 300 ms grace and is SIGKILLed) (Mac T1950)

`kiln-core::process_tree process_tree_unix_what_the_program_left_is_killed_when_the_grace_is_up` failed in main 9c7286825's post-merge check (run 01m7q7r40t2g9zxrrf9m5phrax, load ~34): `assertion left == right failed: ExitStatus(unix_wait_status(9)), left: None, right: Some(0)` at kiln/crates/kiln-core/tests/process_tree.rs:338. It passes alone (7/7 in process_tree).

The leader script (`leader_and_worker`) runs `trap 'exit 0' TERM` around a `sleep 0.05` loop. Under load the shell doesn't run its TERM trap and exit within the 300 ms grace, so cancel_within SIGKILLs it and the status is signal 9, not 0. The same race can hit `process_tree_unix_grace_outlives_the_program` and the other leader_and_worker tests.

Fix the test, not ProcessTree: make the leader's own exit independent of the grace. For example, have cancel_within's grace long enough that only the worker can outlast it (keep `took >= grace` meaningful), or have the leader exit on TERM without a foreground sleep, so it isn't waiting for a child before running the trap. Keep what each test asserts: that the program's status is its own exit code, that the worker is killed only after the grace, and that stdout closes.

## Implementation notes

- The leader script now runs `trap 'exit 0' TERM`, starts the worker, and `wait`s on it: no foreground sleep, and no extra process of its own that could outlive it (an earlier `sleep 1000 &` variant left a straggler holding stdout when TERM landed between the fork and recording its pid).
- `..._killed_when_the_grace_is_up` uses a 2 s grace, so only the worker (ignoring TERM) outlasts it; `took >= grace` still holds.
- C2: with 8 `yes` busy loops running, `cargo nextest run -p kiln-core --test process_tree` passed 30 times in a row (7/7 each).
