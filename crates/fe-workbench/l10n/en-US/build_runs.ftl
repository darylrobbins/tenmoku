# fe-workbench's build runs in the Runs tool window (T1579):
# `workbench-build-runs-…`.

## The command (build_runner.rs)

command-runs-build = Runs: Build Project

## A run's title, as the Runs window lists it

# Runs: Build Project's run. $folder is the name of the folder open.
workbench-build-runs-title = Build: { $folder }

## What the command says when it can't run

# Runs: Build Project in a folder with no Cargo.toml in it or above it.
workbench-build-runs-no-cargo = No Cargo.toml in or above the folder to build
