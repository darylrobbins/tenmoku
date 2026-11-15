# fe-workbench's test runs in the Runs tool window (T1575):
# `workbench-test-runs-…`.

## The commands (test_runner.rs)

command-runs-test_file = Runs: Test File
command-runs-test_project = Runs: Test Project

## A run's title, as the Runs window lists it

# Runs: Test Project's run. $folder is the name of the folder open.
workbench-test-runs-project-title = Test: { $folder }

## What the commands say when they can't run

# Runs: Test Project in a folder with no Cargo.toml at its top.
workbench-test-runs-no-cargo = No Cargo.toml in the folder to test
# Runs: Test File on a file in another language. $language is its name, as
# the status bar shows it; $command is the title of the command that runs
# its tests in a terminal tab, as the palette lists it.
workbench-test-runs-rust-only = Test runs are for Rust files: run { $language } tests with { $command }

## Rerunning a run's failed tests (T1576, test_runner.rs and test_run_view.rs)

command-runs-rerun_failed = Runs: Rerun Failed Tests
# The rerun's title, as the Runs window lists it. $title is the title of the
# run whose failed tests it runs again, such as a cargo command line.
workbench-test-runs-rerun-title = Rerun failed: { $title }
# Runs: Rerun Failed Tests with no test run selected, or none this window ran.
workbench-test-runs-rerun-none = Select a test run to rerun its failed tests
# Runs: Rerun Failed Tests on a run in which no test failed.
workbench-test-runs-rerun-no-failures = No test failed in this run

## A test run's detail in the Runs window (test_run_view.rs)

# The tree of its tests' accessible name.
workbench-test-runs-tree = Tests
# A module row's name, for tests whose binary names no module.
workbench-test-runs-module-top = Top level
# What a module row shows at its right: how many tests it has.
workbench-test-runs-module-count = { $count ->
    [one] { $count } test
   *[other] { $count } tests
}
# A module row's accessible name. $module is its name, as above.
workbench-test-runs-module-row-expanded = { $module }, { $passed } passed, { $failed } failed, expanded
workbench-test-runs-module-row-collapsed = { $module }, { $passed } passed, { $failed } failed, collapsed

## A test row: how the test ended, in its accessible name

workbench-test-runs-outcome-passed = passed
workbench-test-runs-outcome-failed = failed
workbench-test-runs-outcome-ignored = ignored
# A test that both passed and failed in its last runs.
workbench-test-runs-outcome-passed-flaky = passed, flaky
workbench-test-runs-outcome-failed-flaky = failed, flaky

## How long a test took, at its row's right

# $ms is a whole number of milliseconds, under a second.
workbench-test-runs-duration-ms = { $ms } ms
# $secs is a number of seconds, to a tenth.
workbench-test-runs-duration-secs = { $secs } s

# A test row's accessible name: the test's name, how it ended, as above,
# and the results of its last runs. $passed and $failed count them.
workbench-test-runs-test-row = { $name }, { $outcome }, last runs: { $passed } passed, { $failed } failed
# The same, with how long it took, as above.
workbench-test-runs-test-row-timed = { $name }, { $outcome }, { $duration }, last runs: { $passed } passed, { $failed } failed

# A rerun's title when a run's failed tests are rerun one test binary at a
# time, as cargo test reruns them. $title is as above; $binary is the
# binary's package and the cargo options that pick it, such as
# "p --test more".
workbench-test-runs-rerun-title-binary = Rerun failed: { $title } ({ $binary })
# Runs: Rerun Failed Tests on a run from a folder that's no longer open.
workbench-test-runs-rerun-other-folder = This run is from another folder: open it again to rerun its failed tests
# Runs: Rerun Failed Tests when cargo didn't say which target a failed
# test's binary was built from, so it can't be rerun alone. $binary is the
# binary's name, as cargo test names it.
workbench-test-runs-rerun-unknown-target = Can't tell which target { $binary } was built from: run its tests again, then rerun the failures
