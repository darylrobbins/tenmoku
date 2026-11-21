# fe-workbench's terminal panel (T349): `workbench-terminal-…`.

## The panel (terminal_panel.rs)

# The panel's name in the dock's strip and its rail button's tooltip, while
# the active session's shell has set no title.
workbench-terminal-title = Terminal
# A session's tab until its shell sets a title. $number counts the sessions
# started since none was left, from 1.
workbench-terminal-numbered = Terminal { $number }
# A task's tab while another tab with its label is open. $label is the task's
# label, $ordinal which of its tabs this is, from 2.
workbench-terminal-task-numbered = { $label } ({ $ordinal })
# A task's tab once it passed. $label is the tab's name.
workbench-terminal-task-passed = { $label } ✓

# An agent's terminal tab. $lane is the lane's number, $command what the
# terminal runs.
workbench-terminal-lane = lane { $lane } · { $command }

## Command blocks: the label at the right of a command's prompt row

# A command that exited 0 and ran over a second. $duration is how long it
# ran, as below.
workbench-terminal-block-passed = ✓ { $duration }
# A command that exited with another code. $code is the code, $duration how
# long it ran, as below.
workbench-terminal-block-failed = exit { $code } · { $duration }
# The same, when the shell didn't mark when the command started.
workbench-terminal-block-failed-untimed = exit { $code }
# How long a command ran, under a minute: $seconds whole seconds and $tenths
# tenths of one.
workbench-terminal-duration-seconds = { $seconds }.{ $tenths } s
# Under an hour: $minutes whole minutes and $seconds whole seconds.
workbench-terminal-duration-minutes = { $minutes } min { $seconds } s
# An hour or more: $hours whole hours and $minutes whole minutes.
workbench-terminal-duration-hours = { $hours } h { $minutes } min

## The strip's buttons: their accessible names and tooltips

workbench-terminal-new = New Terminal
workbench-terminal-kill = Kill Terminal

## Running a task: the toasts

# $label is the task's label.
workbench-terminal-task-cant-run = Can't run { $label }
# The toast's detail when the task's directory is missing. $path is it.
workbench-terminal-task-not-a-directory = { $path } is not a directory
# The toast's detail when the task names no program.
workbench-terminal-task-no-command = no command
# $title is the task tab's name.
workbench-terminal-task-couldnt-start = Couldn't start { $title }
# $title is the task tab's name, $code the code it exited with.
workbench-terminal-task-failed = { $title } failed (exit { $code })
# $title is the task tab's name: a signal ended it.
workbench-terminal-task-killed = { $title } was killed
# The toast's choice that shows the task's tab.
workbench-terminal-task-show = Show

## Renaming a session (terminal.rename's picker)

# The row that clears the session's name.
workbench-terminal-rename-clear = Use the shell's title
# The row that names the session. $name is the name typed.
workbench-terminal-rename-to = Rename to { $name }
workbench-terminal-rename-status = Enter to rename, Escape to cancel

## The find row (terminal_find.rs)

workbench-terminal-find-placeholder = Find
# Beside the query while none of the visible rows match it.
workbench-terminal-find-no-results = No results
