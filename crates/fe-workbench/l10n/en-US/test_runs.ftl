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
