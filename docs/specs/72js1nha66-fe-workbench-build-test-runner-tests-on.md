---
task_uid: "01m7reb58qpxaptc72js1nha66"
task: "T1970"
rev: 1
type: "chore"
complexity: "l"
criteria:
  - id: "C1"
    text: "GH CI windows Test passes the 4 tests::build_runner tests failing at 8ba6f008c: an_untrusted_root_refuses, compiler_messages_become_problems_and_replace_the_last_builds, files_are_saved_first, the_summary_follows_artifacts; and tests::build_timeline_view::a_running_build_grows_its_bars"
    required: true
    manual: false
  - id: "C2"
    text: "GH CI windows Test passes the 10 tests::test_runner tests failing at 8ba6f008c: a_failure_longer_than_a_line_is_read_whole, a_file_run_reports_live_counts_and_finishes, an_aborted_binary_still_counts_its_failure, an_untrusted_root_refuses, files_are_saved_first, history_keeps_the_order_runs_ended_in, nextest_is_found_as_programs_are_run, nextest_is_used_when_the_root_configures_it, results_are_recorded_in_the_history, results_keep_their_binaries_across_streams"
    required: true
    manual: false
  - id: "C3"
    text: "GH CI windows Test passes the 6 tests::test_run_view tests failing at 8ba6f008c: a_filtered_nextest_run_reruns_only_its_failed_test, a_live_run_reads_history_as_tests_come_and_keeps_open_modules, a_member_run_beside_a_root_package_reruns_in_the_member, a_rerun_that_cant_be_exact_is_refused, bars_show_the_last_eight_oldest_left, rerun_failed_runs_exactly_the_failed_names"
    required: true
    manual: false
  - id: "C4"
    text: "One fake script per tool still drives every platform (no Windows-only copy of its logic); macOS and Linux runs of these modules are unchanged"
    required: true
    manual: false
---

# fe-workbench build/test runner tests on Windows: the fake cargo and nextest are #!/bin/sh scripts on a unix PATH, so the real cargo runs instead; make the fakes runnable on Windows

# Fake cargo/nextest on Windows

GH CI 44405849017 on 8ba6f008c, windows Test: 21 tests fail across build_runner, build_timeline_view, test_runner and test_run_view.

Cause: each fixture copies tests/fixtures/fake_cargo_{build,test}.sh (or an inline `#!/bin/sh` GATED/FILTERING script, or `#!/bin/sh\n` for nextest) to `<bin>/cargo` and sets `RunnerPath` to `bin;/usr/bin;/bin`. On Windows an extensionless shell script isn't runnable and the unix dirs don't exist. The output shows the real cargo ran instead (test_runner's a_failure_longer_than_a_line_is_read_whole recorded `panicked at src\lib.rs:16:9` with a backslash), so the fake recorded no args: `left: []`, "gave up waiting: the run to start". test_run_view.rs:729 also fails with os error 123: the fail markers `fail/<executable> <name>` contain `::`, which is invalid in Windows file names.

Fix (tests only):
- Make the fake runnable as `cargo` (and `cargo-nextest`/`nextest`) on Windows. For example, a `cargo.cmd` shim next to it that runs the same script with the POSIX sh helper from the run_process card (`"<sh>" "%~dp0cargo.sh" %*`), or another way the runner's program lookup accepts. Find out why the lookup fell back to the real cargo and make sure the fake is the one found.
- Build `RunnerPath` with `std::env::join_paths` from the bin dir plus the sh's folder on Windows, not `/usr/bin` and `/bin`.
- Encode fail-marker names (e.g. `::` → `__`) in both the test and the script.
- If the lookup itself is wrong on Windows (e.g. it ignores PATHEXT), stop and report it for a bugfix card.

## Implementation notes

- Why the real cargo ran: `RunProcess` runs `Command::new("cargo")` with the
  run's `PATH`. On Windows std looks a bare name up as `<name>.exe` only
  (no `PATHEXT`): the child's `PATH`, then the app's folder, the system
  folders and the parent's `PATH`, where the real `cargo.exe` was. A
  `cargo.cmd` shim would be skipped the same way. This isn't treated as a
  lookup bug: cargo and cargo-nextest only ship as `.exe`s, and
  `nextest_on_path` already looks for `cargo-nextest.exe`.
- So the shim is an `.exe`: `tests/fixtures/fake_cargo_shim.rs`, built with
  `rustc` once per source version beside the test binary
  (`target/*/deps/fake_cargo_shim-<hash>.exe`, moved in whole so parallel
  nextest processes don't race), copied to `<bin>/cargo.exe`. It runs the
  same `<bin>/cargo` script the other platforms run with the `sh` on
  `PATH`. `install_fake_cargo` (tests/test_runner.rs) does this for all
  four modules; on macOS/Linux it writes the script and the old
  `bin:/usr/bin:/bin` `PATH`, as before.
- The fakes print their folder (manifest paths, package ids, `Compiling`
  lines), which MSYS's `pwd` gives as `/c/…`, unreadable to the workbench.
  The shim passes the Windows form (with `/`s) as `FAKE_CARGO_PWD`; the
  scripts use it when set, else `pwd` as before.
- The fake `cargo-nextest` is only checked for, never run (the fake cargo
  answers `cargo nextest`): on Windows it's named `cargo-nextest.exe`
  (`nextest_in`).
