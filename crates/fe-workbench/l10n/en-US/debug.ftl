# fe-workbench's debugger (docs/decisions/r1b9dw3g0g-debugger.md):
# `workbench-debug-…`.

## Launch configurations (launch_defs.rs): the names detection gives them.

# One per Cargo binary target: $bin is the binary's name.
workbench-debug-launch-detected-bin = Debug { $bin }
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
