# fe-workbench's Runs tool window (T1571): `workbench-runs-…`.

## The command (runs_panel.rs)

command-runs-show = Show Runs

## The tool window

# Its name: the bottom panel's tab, its rail button's tooltip and the list's
# accessible name.
workbench-runs-title = Runs

## The centred empty state: a heading and one hint

workbench-runs-empty = No runs yet
# $command is the title of the command that runs a task, as the palette lists
# it.
workbench-runs-empty-hint = Builds, tests and tasks you start with { $command } show here

## A run's status, in its row's accessible name

workbench-runs-status-running = Running
# $percent is how far along it is, from 0 to 100.
workbench-runs-status-running-progress = Running, { $percent }% done
workbench-runs-status-passed = Passed
workbench-runs-status-failed = Failed
workbench-runs-status-cancelled = Cancelled

## A run's one-line summary

# Nothing's been counted yet.
workbench-runs-summary-running = Running…
# It ended well, saying nothing more.
workbench-runs-summary-passed = Done
# It ended with no exit code: a signal stopped it, or it couldn't start.
workbench-runs-summary-no-code = Stopped without an exit code
workbench-runs-summary-cancelled = Cancelled
# $code is the exit code it ended with.
workbench-runs-summary-exit = Exited with code { $code }
# A test run's counts so far, before the total's known.
workbench-runs-summary-tests = { $passed } passed, { $failed } failed
# A test run's counts, of $total tests.
workbench-runs-summary-tests-total = { $passed } passed, { $failed } failed of { $total }
# A build's crates built so far, and what they said.
workbench-runs-summary-build = { $crates ->
    [one] { $crates } crate
   *[other] { $crates } crates
}, { $warnings ->
    [one] { $warnings } warning
   *[other] { $warnings } warnings
}, { $errors ->
    [one] { $errors } error
   *[other] { $errors } errors
}

## A run's age: how long since it ended, or, while it runs, since it began.
## Drawn short at the row's right; spelled out in its accessible name.

workbench-runs-age-now = now
# $count is a number of seconds, under a minute.
workbench-runs-age-seconds = { $count }s
# $count is a number of minutes, under an hour.
workbench-runs-age-minutes = { $count }m
# $count is a number of hours, under a day.
workbench-runs-age-hours = { $count }h
# $count is a number of days.
workbench-runs-age-days = { $count }d

workbench-runs-age-name-now = just now
workbench-runs-age-name-seconds = { $count ->
    [one] { $count } second ago
   *[other] { $count } seconds ago
}
workbench-runs-age-name-minutes = { $count ->
    [one] { $count } minute ago
   *[other] { $count } minutes ago
}
workbench-runs-age-name-hours = { $count ->
    [one] { $count } hour ago
   *[other] { $count } hours ago
}
workbench-runs-age-name-days = { $count ->
    [one] { $count } day ago
   *[other] { $count } days ago
}

# A row's accessible name: the run's title, its status and its age, each
# as above.
workbench-runs-row-name = { $title }, { $status }, { $age }
