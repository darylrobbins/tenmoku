# fe-workbench's debugger (docs/decisions/r1b9dw3g0g-debugger.md):
# `workbench-debug-…`.

## Launch configurations (launch_defs.rs): the names detection gives them.

# One per Cargo binary target: $bin is the binary's name.
workbench-debug-launch-detected-bin = Debug { $bin }
# A main class jdtls found: $class is its name, simple (App) unless two
# share it.
workbench-debug-launch-detected-java = Debug { $class }
# The same, where two of jdtls's projects have the class: $project is
# jdtls's name for this one's.
workbench-debug-launch-detected-java-in = Debug { $class } ({ $project })
# For the Python file in the active editor.
workbench-debug-launch-current-file = Debug current file

## A launch file's problems (launch_defs.rs), as the Problems panel lists
## them. $file is .tenmoku/launch.toml or .vscode/launch.json; $key a key
## as the file writes it.

# $problem is one of the messages below.
workbench-debug-launch-file-problem = { $file }: { $problem }
# $entry is the configuration's name, or its place in the file: #2.
workbench-debug-launch-entry-problem = { $file }, configuration { $entry }: { $problem }
workbench-debug-launch-not-an-array = `launch` has to be an array of tables: [[launch]]
workbench-debug-launch-json-not-object = not a JSON object
workbench-debug-launch-not-array = `{ $key }` isn't an array
workbench-debug-launch-unknown-key = unknown key `{ $key }`, ignored
workbench-debug-launch-ignored-key = `{ $key }` isn't supported, ignored
workbench-debug-launch-platform-not-object = `{ $key }` isn't an object, ignored
workbench-debug-launch-repeated-name = repeats an earlier configuration's name, left out
# A launch.json configuration named like one in $file, the launch file.
workbench-debug-launch-shadowed = { $file } has a configuration of this name, which is used instead
# An entry that's left out: $problem is why, one of the entry problems
# below.
workbench-debug-launch-left-out = { $problem }, left out
workbench-debug-launch-not-a-table = not a table
workbench-debug-launch-not-an-object = not an object
workbench-debug-launch-missing = no `{ $key }`
workbench-debug-launch-empty-name = an empty name
workbench-debug-launch-not-string = `{ $key }` isn't a string
workbench-debug-launch-not-boolean = `{ $key }` isn't a boolean
workbench-debug-launch-not-strings = `{ $key }` isn't an array of strings
workbench-debug-launch-not-table = `{ $key }` isn't a table
workbench-debug-launch-not-object = `{ $key }` isn't an object
# An adapter_args value JSON can't carry: $key is its path, such as
# adapter_args.when or adapter_args.list[2].
workbench-debug-launch-not-json = `{ $key }` has no JSON form: a date, a time, or an infinite or NaN number
workbench-debug-launch-unknown-request = unknown request `{ $request }`, not launch or attach
workbench-debug-launch-unknown-console = unknown console `{ $console }`, not integrated or internal
workbench-debug-launch-cwd-outside = cwd `{ $cwd }` is outside the project root
workbench-debug-launch-env-bad-name = `{ $name }` can't name an environment variable
workbench-debug-launch-env-not-string = env `{ $name }` isn't a string

## Why a listed configuration can't start (launch_defs.rs).

# A configuration that's listed but can't start: $problem is why, one of
# the messages below.
workbench-debug-launch-cannot-start = { $problem }, so it can't be started
workbench-debug-launch-attach-unsupported = attach isn't supported yet
# $kind is a launch.json configuration's `type`, as written.
workbench-debug-launch-no-adapter-for-type = no adapter for type `{ $kind }`
# $adapter is a launch file configuration's `adapter`, as written.
workbench-debug-launch-no-adapter-named = no adapter named `{ $adapter }`
# $task is the configuration's pre-launch task's label.
workbench-debug-launch-task-not-found = pre-launch task `{ $task }` not found
# $variable is the whole reference, such as ${command:pickProcess}.
workbench-debug-launch-variable-unsupported = { $variable } isn't supported
workbench-debug-launch-variable-unknown = unknown variable { $variable }
# Expanding a configuration's variables as it starts.
workbench-debug-launch-variable-no-editor = { $variable } needs an active editor
workbench-debug-launch-variable-no-home = { $variable } needs a home folder

## The breakpoint store (debug_breakpoints.rs): its state file's problems,
## said in a toast.

# The state file couldn't be read: it's left as it is until the next
# change replaces it. $file is its path; $error why, as the system or
# the JSON parser gives it, or the message below.
workbench-debug-breakpoints-unreadable = Breakpoints in { $file } can't be read, and the next change replaces them: { $error }
workbench-debug-breakpoints-not-an-object = not a JSON object
# $error is why, as the system gives it.
workbench-debug-breakpoints-save-failed = Breakpoints can't be saved: { $error }

## Debug sessions (debug_session.rs, card D7).

command-debug-start = Debug: Start Debugging
command-debug-stop = Debug: Stop
command-debug-rerun_last = Debug: Rerun Last Session
command-debug-show_log = Debug: Show Adapter Log

# Starting a configuration in a folder that isn't trusted, and the toast's
# button.
workbench-debug-untrusted = Trust this folder to debug its programs
workbench-debug-trust-folder = Trust Folder
# $name is the configuration's; $why the reasons it can't start, from the
# launch configuration messages above, joined by semicolons.
workbench-debug-cannot-start = { $name } can't start: { $why }
# Resolving or launching failed: the toast's detail says why.
workbench-debug-cant-start = Can't start { $name }
# The pre-launch task, by its label, exited with a code other than 0: the
# session doesn't start.
workbench-debug-pre-launch-failed = Pre-launch task { $label } failed
# A detected Cargo configuration's build named no program: $bin is the
# binary target's name.
workbench-debug-cargo-no-artifact = cargo built no program named { $bin }
# $language is the configuration's language, or the adapter it names.
workbench-debug-no-adapter-for = no debug adapter for { $language }
# $adapter is the adapter's name; $hint how to get it.
workbench-debug-adapter-not-found = { $adapter } isn't installed: { $hint }
# A session that failed: the toast's detail says why. $name is the
# configuration's.
workbench-debug-session-failed = The debug session { $name } failed
# The adapter, by its name, went away by itself mid-session.
workbench-debug-adapter-exited = The debug adapter { $adapter } exited
# Linux refused to let the debug adapter trace the program. $scope is
# Yama's kernel.yama.ptrace_scope, as /proc/sys/kernel/yama/ptrace_scope has it.
workbench-debug-ptrace-refused = Linux didn't let the debugger trace the program: kernel.yama.ptrace_scope is { $scope }. At 1 a debugger can launch programs; 2 needs CAP_SYS_PTRACE, and 3 forbids tracing.
workbench-debug-ptrace-refused-unknown = Linux didn't let the debugger trace the program, which kernel.yama.ptrace_scope may forbid.
# macOS refused to let the debug adapter attach to the program.
workbench-debug-signing-refused = macOS didn't let the debugger attach to the program: one built with the hardened runtime needs the com.apple.security.get-task-allow entitlement, as Xcode's Debug configurations add.
# What the adapter is told when its runInTerminal asks, on Windows, for
# variables to be removed from the program's environment: $names, joined
# by commas.
workbench-debug-env-unremovable = can't remove { $names } from the program's environment on Windows
# What the adapter is told when its runInTerminal can't be run.
workbench-debug-session-over = the debug session is over
workbench-debug-terminal-didnt-start = the terminal didn't start

## Session changes, said to a screen reader.

# $line is the stopped frame's line, from 1.
workbench-debug-stopped-at-line = Stopped at line { $line }
workbench-debug-running = Running
workbench-debug-session-ended = Session ended

## What the commands say.

workbench-debug-none-run = No debug session has run yet
workbench-debug-not-running = No debug session is running
# Rerunning a configuration the launch files no longer have.
workbench-debug-config-gone = There's no configuration { $name } any more
# The protocol log's tab.
workbench-debug-log-title = Debug Adapter Log
# A debuggee tab's failure toast: start its configuration afresh.
workbench-debug-debuggee-rerun = Rerun
# The button, with its accessible name, beside a debuggee tab.
workbench-debug-debuggee-rerun-session = Rerun Debug Session
# A request that failed while the session carries on: a Continue, a step,
# a breakpoint update. The toast's detail says why. $name is the
# configuration's.
workbench-debug-request-failed = The debug session { $name } couldn't do what was asked

## The configuration picker (debug_session/picker.rs).

workbench-debug-picker-placeholder = Start debugging…
workbench-debug-picker-reading = Reading launch configurations…
# Where a detected configuration comes from, in its row.
workbench-debug-picker-detected = detected
# A configuration that can't start: $source is where it's from; $why the
# reasons, joined by semicolons.
workbench-debug-picker-blocked = { $source } · { $why }
# $file is .tenmoku/launch.toml.
workbench-debug-picker-none = No launch configurations: add one to { $file }

## A session as a run (debug_runs.rs, runs_panel.rs) and as an operation.

# The status bar's operation while a session launches: $name is the
# configuration's.
workbench-debug-operation = Debugging { $name }
# A debug run's one line in the Runs panel.
workbench-debug-run-starting = Starting…
workbench-debug-run-running = Running
workbench-debug-run-stopped = Stopped
# A run's note when the adapter went away by itself.
workbench-debug-run-adapter-exited = The debug adapter exited
# A debug run's accessible name: its configuration, status and age.
workbench-debug-run-row-name = Debug session { $title }, { $status }, { $age }

## Java through jdtls (debug_java.rs). Each is a failed start's detail.

# java-debug wasn't found: the hint at the end of what a failed start says.
workbench-debug-java-no-jar = Java debugging needs java-debug, which Tenmoku doesn't include, and it wasn't found. Install the Debugger for Java extension in VS Code, or java-debug-adapter with Mason, or set debug.adapters.java.path to its com.microsoft.java.debug.plugin jar.
# No jdtls runs for the folder: $root is the folder.
workbench-debug-java-no-jdtls = Java debugging goes through jdtls, and none is running for { $root }. Open one of its Java files, let jdtls start, and try again.
# jdtls runs without java-debug, as it started before java-debug was
# found: $root is the folder.
workbench-debug-java-restart = jdtls for { $root } started without java-debug, which it loads only as it starts. Restart it (lsp.restart in one of its Java files), then try again.
# The user's [lsp.servers.jdtls] initialization_options can't take
# java-debug.
workbench-debug-java-options-not-table = java-debug can't be added to jdtls: [lsp.servers.jdtls] initialization_options isn't a table. Make it one, then restart jdtls.
workbench-debug-java-bundles-not-list = java-debug can't be added to jdtls: the bundles in [lsp.servers.jdtls] initialization_options aren't a list. Make them one, then restart jdtls.
# jdtls was given java-debug and still lacks its command: $jar is the jar,
# $command the command.
workbench-debug-java-not-loaded = jdtls has no { $command }: it didn't load java-debug from { $jar }.
# A step's failure: $error is what jdtls said.
workbench-debug-java-main-class-failed = jdtls couldn't list the main classes: { $error }
workbench-debug-java-classpath-failed = jdtls couldn't resolve the classpath of { $class }: { $error }
workbench-debug-java-start-failed = jdtls couldn't start java-debug: { $error }
# What jdtls answered wasn't what was asked for: $command is the command.
workbench-debug-java-bad-answer = jdtls's answer to { $command } wasn't one Tenmoku can read.
workbench-debug-java-no-main-class = jdtls found no main class to debug. Name one with mainClass.
# $classes are the classes, joined by commas.
workbench-debug-java-main-classes = jdtls found several main classes: { $classes }. Name one with mainClass.

## A breakpoint's menu, a right click on its row's line number
## (debug_gutter.rs).

workbench-debug-menu-add-breakpoint = Add Breakpoint
workbench-debug-menu-disable-breakpoint = Disable Breakpoint
workbench-debug-menu-enable-breakpoint = Enable Breakpoint
workbench-debug-menu-remove-breakpoint = Remove Breakpoint

## Stepping and breakpoint commands (debug_session/commands.rs, card D8).

command-debug-continue = Debug: Continue
command-debug-pause = Debug: Pause
command-debug-step_over = Debug: Step Over
command-debug-step_into = Debug: Step Into
command-debug-step_out = Debug: Step Out
command-debug-restart = Debug: Restart
command-debug-run_to_cursor = Debug: Run to Cursor
command-debug-start_with_args = Debug: Start with Arguments…
command-debug-show_session = Debug: Show Session…
command-debug-toggle_breakpoint = Debug: Toggle Breakpoint
command-debug-toggle_breakpoint_enabled = Debug: Enable or Disable Breakpoint
command-debug-remove_all_breakpoints = Debug: Remove All Breakpoints

# A breakpoint command, or run to cursor, with no file's editor active.
workbench-debug-no-file = Put the caret in a file first
# Enable or Disable Breakpoint on a line that has none.
workbench-debug-no-breakpoint-here = There's no breakpoint on this line

# Start with Arguments' prompt: its input, then its one row.
workbench-debug-args-placeholder = Arguments for the program…
# $args is what was typed, as typed.
workbench-debug-args-start = Start debugging with { $args }
workbench-debug-args-none = Start debugging with no extra arguments
# The typed arguments open a quote they don't close.
workbench-debug-args-unclosed = Close the quote to start debugging
# The typed arguments end in a backslash outside quotes, which escapes
# nothing.
workbench-debug-args-trailing-escape = Finish the escape at the end, or remove the backslash, to start debugging
# Show Session's picker, over the window's sessions.
workbench-debug-session-placeholder = Debug sessions

## The floating debug toolbar (debug_toolbar.rs): its buttons' names, each
## its tooltip too, then its session label's accessible name.

workbench-debug-toolbar-continue = Continue
workbench-debug-toolbar-pause = Pause
workbench-debug-toolbar-step-over = Step Over
workbench-debug-toolbar-step-into = Step Into
workbench-debug-toolbar-step-out = Step Out
workbench-debug-toolbar-restart = Restart
workbench-debug-toolbar-stop = Stop Debugging
# $name is the session's configuration; $line the stopped frame's line,
# from 1.
workbench-debug-toolbar-session-starting = Debug session: { $name }, starting
workbench-debug-toolbar-session-running = Debug session: { $name }, running
workbench-debug-toolbar-session-stopped = Debug session: { $name }, stopped
workbench-debug-toolbar-session-stopped-at = Debug session: { $name }, stopped at line { $line }
