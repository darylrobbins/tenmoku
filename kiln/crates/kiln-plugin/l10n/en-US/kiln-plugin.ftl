# kiln-plugin's messages: what a plugin's capabilities let it do, for the
# prompt that grants them, and why a plugin, its commands or its calls
# failed. `$plugin` is a plugin's ID, `$id` and `$command` command IDs;
# `$reason` is an error's own text.

## What a capability lets a plugin do (`warnings::describe`), in the prompt
## that grants it.

kiln-plugin-capability-buffer-read = read the text of your open files
kiln-plugin-capability-buffer-write = edit your open files
kiln-plugin-capability-commands = add commands, and run other commands
kiln-plugin-capability-decorations = mark up text and the gutter in your files
kiln-plugin-capability-status = show items in the status bar
# `$scope` is the folders' scope: `workspace`, the folders open in the
# workspace.
kiln-plugin-capability-fs-read = read any file in your { $scope ->
       *[workspace] workspace
    } folders
kiln-plugin-capability-fs-write = write any file in your { $scope ->
       *[workspace] workspace
    } folders
kiln-plugin-capability-spawn = run the program { $program }, outside the sandbox
kiln-plugin-capability-spawn-any = run any program, outside the sandbox
kiln-plugin-capability-net = connect to { $host }
kiln-plugin-capability-net-any = connect to any host on the internet

## The warnings a prompt shows for a plugin's capabilities together.
## `$hosts` and `$programs` are lists, joined by kiln-plugin-and-list.

kiln-plugin-warning-sends-code = It can read your code and send it to { $hosts }.
kiln-plugin-warning-sends-anywhere = Extreme: with net:* it can send your code anywhere, to any host on the internet.
kiln-plugin-warning-outside-sandbox = It runs { $programs } outside the sandbox, with your permissions.
kiln-plugin-warning-near-wildcard = { $program } is near-wildcard: it can run any code, so granting it is almost process.spawn:*.
kiln-plugin-warning-runs-any-program = Extreme: with process.spawn:* it can run any program as you, outside the sandbox.
# A list of two or more: `$items` is all but the last, joined by ", ".
kiln-plugin-and-list = { $items } and { $last }

## The grants file.

kiln-plugin-grants-file-header =
    # The capabilities you granted each plugin, and the programs its
    # process.spawn grants were bound to. Remove a plugin's table to be asked
    # again when it next loads.
# `$path` is the grants file.
kiln-plugin-grants-io = { $path }: { $reason }
kiln-plugin-grants-unreadable = { $path } isn't a grants file this app understands: { $reason }
# `$grants` are the capabilities asked for, as written.
kiln-plugin-grants-wildcard-not-accepted = granting { $grants } needs its own acceptance of the wildcard
kiln-plugin-grants-program-not-resolved = process.spawn:{ $program } has no program resolved for it
kiln-plugin-grants-program-suspended = process.spawn:{ $program } is suspended until it is granted again
kiln-plugin-grants-program-unbound = process.spawn:{ $program } can't be bound to a program: { $reason }

## The programs process.spawn grants are bound to. `$name` is a program's
## name, quoted where it isn't one.

kiln-plugin-program-not-a-name = { $name } is not a program name: it can't be empty, `.`, `..` or hold a `/`
kiln-plugin-program-not-found = { $name } isn't an executable file in any absolute PATH directory
kiln-plugin-program-unreadable = can't read { $path }: { $reason }
kiln-plugin-program-not-executable = not an executable file
kiln-plugin-program-check-timed-out = checking the program ran past its deadline
kiln-plugin-program-suspended = the program's grant is suspended: { $reason }
kiln-plugin-program-changed = the program changed
kiln-plugin-program-gone = the program is gone

## Running programs (process.spawn).

kiln-plugin-spawn-not-granted = process.spawn:{ $program } is not granted
kiln-plugin-spawn-suspended = process.spawn:{ $program }: { $reason }
# `$deadline` is a duration, as `300ms`.
kiln-plugin-spawn-timed-out = { $program } ran past its { $deadline } deadline and was killed
kiln-plugin-spawn-failed = can't run { $path }: { $reason }

## A plugin's manifest table, `[contributes.plugin]`.

kiln-plugin-manifest-workspace = a workspace extension cannot install a plugin ({ $id })
kiln-plugin-manifest-more-than-one = { $id }: [contributes.plugin] must be a single table
kiln-plugin-manifest-missing-wasm = { $id }: [contributes.plugin] needs a `wasm` string
kiln-plugin-manifest-capabilities-not-strings = { $id }: `capabilities` must be an array of strings
kiln-plugin-manifest-signature-not-string = { $id }: `signature` must be a string
kiln-plugin-manifest-capability = { $id }: { $reason }
# `$path` is the `wasm` path, quoted; `$reason` one of the messages below.
kiln-plugin-manifest-wasm-path = { $id }: wasm path { $path }: { $reason }
kiln-plugin-wasm-path-empty = the path is empty
kiln-plugin-wasm-path-backslash = use / in extension paths
kiln-plugin-wasm-path-absolute = must be relative to the extension's directory
kiln-plugin-wasm-path-outside = is outside the extension's directory
kiln-plugin-wasm-path-dir-unreadable = the extension's directory is unreadable
kiln-plugin-wasm-path-missing = no such file
kiln-plugin-wasm-path-not-a-file = is not a file

## The plugin engine and its API version. `$needed` is the version a
## plugin needs, `$host` this host's.

kiln-plugin-no-engine = no plugin engine: { $reason }
kiln-plugin-no-deadline-thread = no thread to enforce plugins' deadlines: { $reason }
kiln-plugin-needs-newer-host = plugin needs API { $needed }, but this host only provides { $host }
kiln-plugin-unsupported-major = plugin needs API { $needed }, but this host ({ $host }) does not support that major

## Loading and running a plugin.

# What unloaded a plugin: `$message` says what happened.
kiln-plugin-unloaded = plugin { $plugin } unloaded, { $reason ->
        [trap] trapped
        [timeout] timed out
        [memory] out of memory
       *[protocol] protocol error
    }: { $message }
# Why a plugin was unloaded, alone.
kiln-plugin-unload-reason = { $reason ->
        [trap] trapped
        [timeout] timed out
        [memory] out of memory
       *[protocol] protocol error
    }
# `$bytes` and `$limit` are sizes in bytes.
kiln-plugin-too-large = plugin { $plugin }: the component is { $bytes } bytes, over the { $limit }-byte limit
kiln-plugin-invalid = plugin { $plugin }: not a component this host runs: { $reason }
kiln-plugin-no-thread = plugin { $plugin }: no thread to run it on: { $reason }
kiln-plugin-init-failed = plugin { $plugin }: init failed: { $reason }
kiln-plugin-call-unloaded = the plugin is unloaded
kiln-plugin-thread-stopped-loading = its thread stopped while loading it
kiln-plugin-thread-stopped = its thread stopped
# A component without an export this host calls: `$export` is its
# interface and name, as `kiln:extension/command-handler@1.0.0#run-command`.
kiln-plugin-no-export = no export { $export }
# `$deadline` is a duration, as `2s`.
kiln-plugin-call-deadline = a call ran past its { $deadline } deadline

## Plugins' commands.

kiln-plugin-already-loaded = plugin { $plugin } is already loaded
kiln-plugin-unloaded-with-requests = plugin { $plugin } was unloaded with an invocation still out
kiln-plugin-was-unloaded = plugin { $plugin } was unloaded
kiln-plugin-command-not-run = { $command } was not run
kiln-plugin-command-builtin = { $id } is a built-in command
kiln-plugin-command-taken = { $id } is already registered by plugin { $owner }
kiln-plugin-namespace-reserved = the { $namespace } namespace is reserved
kiln-plugin-command-plugin-unloaded = { $id }: plugin { $plugin } was unloaded
kiln-plugin-command-plugin-invoking = { $id }: plugin { $plugin } is waiting on a command it invoked
# `$error` and `$reply` are what the plugin returned, as the host reads it.
kiln-plugin-command-returned = { $id }: plugin { $plugin } returned { $error }
kiln-plugin-command-answered = { $id }: plugin { $plugin } answered { $reply }
kiln-plugin-command-failed = { $id }: { $reason }
# `$capability` is the capability's name, `commands`.
kiln-plugin-needs-capability = plugin { $plugin } needs the { $capability } capability to { $action ->
        [register] register commands
       *[invoke] invoke commands
    }
kiln-plugin-register-after-init = plugin { $plugin } registers commands only during init
kiln-plugin-command-outside-namespace = { $id } is outside plugin { $plugin }'s namespace: its commands are { $plugin }.<name>
kiln-plugin-command-args-schema = { $id }: args-schema: { $reason }
kiln-plugin-command-when = { $id }: when: { $reason }
kiln-plugin-command-args = { $id }: args: { $reason }
kiln-plugin-command-deadline = { $id } was not answered before the call's deadline

## Plugins' network requests. `$url` is the URL asked for or reached;
## quoted as the plugin wrote it where it's the URL refused.

kiln-plugin-net-none = the host has no network
kiln-plugin-net-timed-out = { $url } timed out
# `$peer` is the host it reached, `$host` the one asked for.
kiln-plugin-net-wrong-peer = { $url } connected to { $peer }, not { $host }
kiln-plugin-net-too-large = { $url }'s response is over { $limit } bytes
kiln-plugin-net-too-many-redirects = { $url } redirected more than { $limit } times
kiln-plugin-net-redirect-not-https = { $url } redirected to { $next }, which isn't https
kiln-plugin-net-not-a-url = { $url } isn't a URL: { $reason }
kiln-plugin-net-user-name = { $url } has a user name, which isn't sent
kiln-plugin-net-empty-label = { $url } has an empty label in its host
kiln-plugin-net-no-host = { $url } has no host
kiln-plugin-net-not-https = { $url } isn't https (http only to 127.0.0.1 or [::1])
kiln-plugin-net-bad-host = { $url } { $reason }
kiln-plugin-net-not-granted = { $url }: no net:{ $host } grant reaches its host
# `$status` is the response's HTTP status.
kiln-plugin-net-locations = a { $status } response has more than one Location
# `$location` is the redirect's Location, quoted; `$reason` why it's refused.
kiln-plugin-net-bad-redirect = { $url } redirected to { $location }: { $reason }

## Plugins' secrets.

kiln-plugin-secret-name = a secret's name is 1 to { $max } characters of a-z, 0-9, _ and -
# `$path` is the secrets index.
kiln-plugin-secrets-index = couldn't { $action ->
        [lock] lock
        [parse] parse
        [read] read
        [serialize] serialize
       *[write] write
    } the secrets index { $path }: { $reason }
