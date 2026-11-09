# fe-lsp's messages.

lsp-error-server-down = the language server isn't running
lsp-error-cancelled = the request was cancelled
lsp-error-timeout = the request timed out
lsp-error-retry = the server asked for the request to be retried
lsp-error-server = { $message } ({ $code })
lsp-error-protocol = bad reply: { $why }
lsp-error-refused-edit = the edit can't be applied: { $why }

lsp-action-disabled = disabled: { $why }
lsp-action-stale = the action is from an earlier session or opening

## Completion kinds: `lsp-kind-<name>`, the menu's label for an item kind.

lsp-kind-text = text
lsp-kind-method = method
lsp-kind-function = function
lsp-kind-constructor = constructor
lsp-kind-field = field
lsp-kind-variable = variable
lsp-kind-class = class
lsp-kind-interface = interface
lsp-kind-module = module
lsp-kind-property = property
lsp-kind-unit = unit
lsp-kind-value = value
lsp-kind-enum = enum
lsp-kind-keyword = keyword
lsp-kind-snippet = snippet
lsp-kind-color = color
lsp-kind-file = file
lsp-kind-reference = reference
lsp-kind-folder = folder
lsp-kind-enum-member = enum member
lsp-kind-constant = constant
lsp-kind-struct = struct
lsp-kind-event = event
lsp-kind-operator = operator
lsp-kind-type-parameter = type parameter

## Unavailable: why a request on a document can't be answered.

lsp-unavailable-buffer-changed = buffer changed
lsp-unavailable-too-large = too large for the language server
lsp-unavailable-not-open = not open on a language server
lsp-not-absolute = { $path } isn't an absolute path
lsp-not-a-server-command = { $command }: not a command of the server

## Server lifecycle: the detail a server's status shows.

lsp-server-thread-panicked = fe-lsp's server thread panicked
lsp-server-not-found = { $name } not found
lsp-server-stopped-before-initialize = { $name } stopped before answering initialize: { $why }
lsp-server-exited-before-initialize =
    { $name } { $kind ->
        [code] exited with code { $code }
        [signal] was killed by signal { $signal }
       *[plain] exited
    } before answering initialize
lsp-server-initialize-timeout =
    { $seconds ->
        [one] { $name } didn't answer initialize within { $seconds } s
       *[other] { $name } didn't answer initialize within { $seconds } s
    }
lsp-server-initialize-failed = { $name }: initialize failed: { $why }
lsp-server-stopped-after-initialize = { $name } stopped after answering initialize: { $why }
lsp-server-orphans =
    { $count ->
        [one] { $name } left more than { $count } cancelled requests unanswered
       *[other] { $name } left more than { $count } cancelled requests unanswered
    }
lsp-server-stopped = { $name } stopped: { $why }
lsp-server-crashed =
    { $name } { $kind ->
        [code] exited with code { $code }
        [signal] was killed by signal { $signal }
       *[plain] exited
    }
lsp-loop-closed = the connection closed
lsp-loop-eof = the server closed its output (EOF)
lsp-loop-stopped = the connection stopped
lsp-backlog-over-mib =
    { $count ->
        [one] server output backlog over { $count } MiB
       *[other] server output backlog over { $count } MiB
    }
lsp-backlog-over-bytes =
    { $count ->
        [one] server output backlog over { $count } bytes
       *[other] server output backlog over { $count } bytes
    }

## Navigation: why a navigation request has no answer.

lsp-nav-not-final = empty answer from a server without a completion signal
lsp-nav-none-ready = no language server is ready
lsp-nav-no-buffer = the request names no buffer
lsp-nav-gone = the language servers have gone
lsp-nav-dropped = { $uri }: { $why }
lsp-nav-not-a-file = not a file
lsp-nav-unreadable = couldn't be read

## Workspace edits: why the edit can't be applied.

lsp-edit-ends-before-start = an edit ends ({ $end_line }:{ $end_column }) before it starts ({ $start_line }:{ $start_column })
lsp-edit-overlap = its edits overlap
lsp-edit-after-renamed = it edits { $path }, which it renamed earlier
lsp-edit-after-deleted = it edits { $path }, which it deleted earlier
lsp-edit-made-by-edit = { $path } is made by the edit, which can't know its version { $version }
lsp-edit-in-file = { $path }: { $why }
lsp-edit-open-twice = { $path } is open twice
lsp-edit-not-open-at-version = { $path } isn't open at version { $version }
lsp-edit-changed = { $path } has changed: the edit is for version { $version }, the server has { $server }
lsp-not-a-file = { $uri } isn't a file
lsp-not-a-local-file = { $uri } isn't a local file

## Launching: why a server's process didn't start.

lsp-launch-cancelled = { $name }'s launch was cancelled
lsp-launch-stdio = the child's stdio wasn't piped
