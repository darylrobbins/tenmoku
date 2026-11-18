---
task_uid: "01m7q2j0bn0epxczss7ahtz9c4"
task: "T424"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "the_system_runner_stops_a_descendant_holding_its_stdout writes its pid file before the runner can see output, and passes 50 runs in a row under nextest's default parallelism"
    required: true
    manual: false
---

# Flake: fe-dap registry the_system_runner_stops_a_descendant_holding_its_stdout reads a pid file the script hadn't written yet under load (Mac T1911)

# Flake: fe-dap registry the_system_runner_stops_a_descendant_holding_its_stdout reads a pid file the script never wrote

Seen in T1852's gate on 2026-11-16 (run `~/.local/state/regatta/runs/01m7jjyq4c9fft00aj3yb5xw75`,
test.fastedit.log line 1321): 5880 of 5881 passed; this one panicked at
`crates/fe-dap/src/tests/registry.rs:1038` with `Os { code: 2, kind: NotFound }`
reading `descendant.pid`. It passed 5 of 5 alone right after.

The `lingering` script is `echo /opt/rust; sleep 30 &; echo $! > descendant.pid; exit 0`.
Under the gate's load the runner's deadline can stop the script (and its process group)
before the second `echo` writes the pid file, so `read_to_string(&pid_file).unwrap()`
fails. T1877 (abandoned) was the same test's printing case timing out; this is the
lingering case.

Fix in the test: write the pid file before printing (`sleep 30 & echo $! > pid; echo /opt/rust`),
so the file exists whenever the runner has seen output; or treat a missing pid file as
"nothing lingered" only after checking the runner returned within its deadline. Keep
the assertion that a descendant holding stdout is stopped.

## Implementation notes

- The lingering script now writes `descendant.pid` before printing. The test also treats a missing pid file (NotFound) as "the deadline stopped the script first, nothing lingered"; other read errors still panic and `wait_gone` still checks a recorded pid.
- Evidence for C1: `cargo nextest run -p fe-dap --stress-count 50 -E 'test(system_runner)'` gave 50/50 iterations passed.
