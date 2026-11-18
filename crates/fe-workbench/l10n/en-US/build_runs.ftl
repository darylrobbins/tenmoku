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

## A build run's detail in the Runs window (build_timeline_view.rs)

# The switch between the crate waterfall and the build's output, as a
# screen reader names it, and its two segments.
workbench-build-runs-view = Build detail
workbench-build-runs-timeline = Timeline
workbench-build-runs-output = Output
# The waterfall's rows, one per crate, as a screen reader names the list.
workbench-build-runs-crates = Crates
# A tick on the waterfall's time axis, $minutes and $seconds from the
# build's start.
workbench-build-runs-tick =
    { $minutes ->
        [0] { $seconds }s
       *[other]
            { $seconds ->
                [0] { $minutes }m
               *[other] { $minutes }m { $seconds }s
            }
    }
# How long a crate took to build, at its row's right, and the build's
# length, over them: whole milliseconds under a second, else seconds to a
# tenth.
workbench-build-runs-duration-ms = { $ms }ms
workbench-build-runs-duration-secs = { $secs }s
# A crate's warning or error count, beside its glyph (▲ or ✕).
workbench-build-runs-count = { $count }
# What a crate's row says it is, for a screen reader: fresh (up to date,
# nothing compiled), being compiled, or built in $duration.
workbench-build-runs-crate-fresh = fresh
workbench-build-runs-crate-building = building
workbench-build-runs-crate-built = built in { $duration }
# A crate's row, for a screen reader: its name, what it is (one of the
# three above), whether it's a local crate ($local is yes or no), and its
# warnings and errors.
workbench-build-runs-crate-row =
    { $name }, { $state }{ $local ->
        [yes] , local
       *[no] {""}
    }{ $warnings ->
        [0] {""}
        [one] , 1 warning
       *[other] , { $warnings } warnings
    }{ $errors ->
        [0] {""}
        [one] , 1 error
       *[other] , { $errors } errors
    }
# The waterfall of a build with no crates: one still starting, or one that
# built none.
workbench-build-runs-no-crates-yet = Waiting for cargo to start a crate
workbench-build-runs-no-crates = No crates were built
# The Output view of a build that printed nothing to stderr.
workbench-build-runs-no-output = No output
