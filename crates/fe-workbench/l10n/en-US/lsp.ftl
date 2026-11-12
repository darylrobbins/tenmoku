# fe-workbench's language server, tasks, tests, quickfix and reference panel
# strings (i18n design, W6): `workbench-lsp-…`.

workbench-lsp-ok = OK

## A language server's edit previewed in the AI Changes panel
## (refactor_preview.rs, refactor_preview_view.rs). $file is a path.

# A row's operation.
workbench-lsp-refactor-create = Create
workbench-lsp-refactor-rename-to = Rename to { $path }
workbench-lsp-refactor-delete = Delete
# Why the edit can't apply as reviewed.
workbench-lsp-refactor-no-longer-open = { $file } is no longer open
workbench-lsp-refactor-changed-since = { $file } has changed since
workbench-lsp-refactor-doesnt-exist = { $file } doesn't exist
workbench-lsp-refactor-cant-read = can't read { $file }: { $error }
workbench-lsp-refactor-saved-under-another-name = { $file } has been saved under another name since
workbench-lsp-refactor-opened-since = { $file } has been opened since
# $message is why applying stopped.
workbench-lsp-refactor-partly-made =
    { $message }

    Part of the edit was made.
# A review tab's title: the file's name.
workbench-lsp-refactor-tab-title = { $name } (preview)
workbench-lsp-refactor-whole-only = this edit applies whole or not at all
workbench-lsp-refactor-overlap = its edits overlap
# $range is the edit's byte range, such as 3..7.
workbench-lsp-refactor-edit-doesnt-fit = an edit at { $range } doesn't fit its text
# $problem is what's wrong, such as workbench-lsp-refactor-overlap.
workbench-lsp-refactor-file-problem = { $file }: { $problem }

## The quickfix and location lists (quickfix.rs, quickfix_panel.rs), as
## nvim words them: a message's E number is nvim's.

workbench-lsp-quickfix-e42 = E42: No Errors
workbench-lsp-quickfix-e553 = E553: No more items
workbench-lsp-quickfix-e380 = E380: At bottom of quickfix stack
workbench-lsp-quickfix-e381 = E381: At top of quickfix stack
workbench-lsp-quickfix-e776 = E776: No location list
# On landing at an item: (2 of 3) error: text. $kind is the item's type,
# error, warning, info or note.
workbench-lsp-quickfix-landed = ({ $current } of { $total }) { $kind ->
        [error] error
        [warning] warning
        [info] info
       *[note] note
    }: { $text }
# The same, for an item with no type, such as a :grep match.
workbench-lsp-quickfix-landed-untyped = ({ $current } of { $total }): { $text }
# An item in :clist's line: 1 src/a.rs:3 col 5 error: text.
workbench-lsp-quickfix-listed = { $nth } { $path }:{ $row } col { $col } { $kind ->
        [error] error
        [warning] warning
        [info] info
       *[note] note
    }: { $text }
workbench-lsp-quickfix-listed-untyped = { $nth } { $path }:{ $row } col { $col }: { $text }
# After :clist's first items: how many it leaves out.
workbench-lsp-quickfix-more = … { $count } more
# The list in use, as :colder and :cnewer say it: nvim says errors
# whatever the count.
workbench-lsp-quickfix-summary = error list { $current } of { $total }; { $count } errors
# The panel's title row.
workbench-lsp-quickfix-quickfix-list = Quickfix List
workbench-lsp-quickfix-location-list = Location List
workbench-lsp-quickfix-items = { $count ->
        [one] { $count } item
       *[other] { $count } items
    }
workbench-lsp-quickfix-no-location-list = No location list
workbench-lsp-quickfix-no-quickfix-list = No quickfix list

## Filling a list: :grep, :vimgrep and :make (quickfix_fill.rs).

workbench-lsp-quickfix-no-folder = Open a folder to search it
workbench-lsp-quickfix-invalid-glob = Invalid glob { $glob }: { $error }
# $reason is the regex parser's.
workbench-lsp-quickfix-e383 = E383: Invalid search string: { $pattern } ({ $reason })
workbench-lsp-quickfix-e480 = E480: No match: { $pattern }
# $text is what landing says, when the search stopped at its cap.
workbench-lsp-quickfix-capped = { $text } (the first { $count } matches)
# $args is what was typed after :make.
workbench-lsp-quickfix-make-ignoring = Ignoring { $args }: :make runs the default build task
workbench-lsp-quickfix-e499 = E499: Empty file name for '%' or '#'
# $path is a :vimgrep file argument.
workbench-lsp-quickfix-outside = { $path } is outside the folder

## Shared by the language server features.

workbench-lsp-no-server = No language server for this file
# $error is why the file can't be read.
workbench-lsp-cant-open-error = Can't open { $path }: { $error }

## The call and type hierarchy panel (hierarchy_panel.rs).

workbench-lsp-hierarchy-show-outgoing = Show Outgoing Calls
workbench-lsp-hierarchy-show-incoming = Show Incoming Calls
workbench-lsp-hierarchy-show-subtypes = Show Subtypes
workbench-lsp-hierarchy-show-supertypes = Show Supertypes
# What an expanded row with nothing under it says.
workbench-lsp-hierarchy-no-calls = No calls
workbench-lsp-hierarchy-no-supertypes = No supertypes
workbench-lsp-hierarchy-no-subtypes = No subtypes
workbench-lsp-hierarchy-loading = Loading…
# A row's badge: the call is the function calling itself, or one of its
# callers.
workbench-lsp-hierarchy-recursive = recursive
workbench-lsp-hierarchy-calls = Call Hierarchy
workbench-lsp-hierarchy-types = Type Hierarchy
# The title row: $name is the symbol at the root.
workbench-lsp-hierarchy-calls-of = Call Hierarchy: { $name }
workbench-lsp-hierarchy-types-of = Type Hierarchy: { $name }
# $error is the language server's.
workbench-lsp-hierarchy-calls-failed = Call Hierarchy: { $error }
workbench-lsp-hierarchy-types-failed = Type Hierarchy: { $error }
workbench-lsp-hierarchy-no-calls-here = No call hierarchy available here
workbench-lsp-hierarchy-no-types-here = No type hierarchy available here

## An ignore file's problems (ignore_diagnostics.rs). $line is a line
## number, from 1.

workbench-lsp-ignore-unused = This pattern matches nothing in the project
workbench-lsp-ignore-duplicate = Duplicate of line { $line }
workbench-lsp-ignore-shadowed = Everything this pattern matches is decided the other way by line { $line }

## The language server's status item (lsp.rs). $name is the server's,
## such as rust-analyzer.

workbench-lsp-status-starting = { $name }: starting…
# $title is the server's progress title, such as Indexing or Loading.
workbench-lsp-status-indexing = { $name }: indexing { $title }
# $attempt is this restart's number, $attempts how many it's allowed.
workbench-lsp-status-restarting = { $name }: restarting ({ $attempt }/{ $attempts })
workbench-lsp-status-stopped-error = { $name }: stopped: { $error }
workbench-lsp-status-stopped = { $name }: stopped
workbench-lsp-status-tooltip = Show Language Server Log
workbench-lsp-status-untrusted = { $name }: folder not trusted
# $root is the folder the server would start in.
workbench-lsp-status-untrusted-detail = { $root } isn't inside a trusted folder, so its language servers don't start
workbench-lsp-cant-restart = Can't restart { $name }

## A document link, followed (lsp_links.rs).

workbench-lsp-link-no-target = This link has no target
# $target is the link's address, or a file's name.
workbench-lsp-link-cant-open = Can't open { $target }
workbench-lsp-link-unsupported-scheme = Can't open link: unsupported scheme

## Code actions (lsp_code_actions.rs).

workbench-lsp-code-actions-stale = The file changed; ask again
# $kind is the kind asked for: organize-imports (source.organizeImports),
# fix-all (source.fixAll), refactor or source. Each variant is a whole message;
# any other kind uses workbench-lsp-code-actions-none.
workbench-lsp-code-actions-none-of-kind =
    { $kind ->
        [organize-imports] No organize imports actions available
        [fix-all] No fix all actions available
        [refactor] No refactor actions available
       *[source] No source actions available
    }
workbench-lsp-code-actions-none = No code actions available
workbench-lsp-code-actions-placeholder = Filter code actions…
workbench-lsp-code-actions-window-closed = the window has closed
# $why is the language server's reason the action is disabled.
workbench-lsp-code-actions-disabled = Disabled: { $why }
workbench-lsp-code-actions-status = Enter to apply, Escape to cancel

## Formatting (lsp_format.rs). $language is the file's language's name.

workbench-lsp-no-formatter = No formatter for { $language }

## Rename Symbol (lsp_rename.rs).

workbench-lsp-rename-stale = The file changed since the rename began: nothing renamed
workbench-lsp-rename-running = A rename is already running
workbench-lsp-rename-nothing = Nothing to rename here
workbench-lsp-rename-placeholder = New name…
# The preview's title: $from is the symbol's name, $to the new one.
workbench-lsp-rename-title = Rename { $from } to { $to }
workbench-lsp-rename-unsupported = The language server can't rename
# The picker's row: $name is the new name typed.
workbench-lsp-rename-to = Rename to { $name }
workbench-lsp-rename-status = Enter to rename, Escape to cancel

## Peek Definition (peek.rs). $name is a file's name.

# $error is why it can't be shown: workbench-lsp-peek-too-large,
# workbench-lsp-peek-not-found or the system's.
workbench-lsp-peek-cant-show = Couldn't show { $name }: { $error }
workbench-lsp-peek-too-large = too large to show here
workbench-lsp-peek-not-found = not found

## Find references' panel and its peek (references_panel.rs,
## peek_references.rs). $word is the symbol's name. $source is search when
## a search provider gave the answer, which the status then names as
## $provider, and exact otherwise.

workbench-lsp-references-header = References to `{ $word }`
workbench-lsp-references-no-folder = No folder open
workbench-lsp-references-no-symbol = No symbol at the caret
workbench-lsp-references-finding = Finding references to `{ $word }`…
workbench-lsp-references-failed = Find references failed: { $error }
workbench-lsp-references-no-symbol-by = No symbol at the caret{ $source ->
        [search] { " " }(by { $provider })
       *[exact] { "" }
    }
# When the search stopped at its cap.
workbench-lsp-references-first = Showing the first { $count ->
        [one] { $count } reference
       *[other] { $count } references
    }{ $source ->
        [search] { " " }(by { $provider })
       *[exact] { "" }
    }
workbench-lsp-references-none = No references to `{ $word }`{ $source ->
        [search] { " " }(by { $provider })
       *[exact] { "" }
    }
# $count references in $files files.
workbench-lsp-references-found = { $count ->
        [one] { $count } reference
       *[other] { $count } references
    } in { $files ->
        [one] { $files } file
       *[other] { $files } files
    }{ $source ->
        [search] { " " }(by { $provider })
       *[exact] { "" }
    }
# Beside a reference that is the symbol's definition.
workbench-lsp-references-definition = definition

## Insert Snippet and Configure User Snippets (snippet_picker.rs).

# An extension's snippet's detail: its description, and $source, where it
# comes from.
workbench-lsp-snippet-detail = { $description } ({ $source })
workbench-lsp-snippet-none = No snippets for this language: Configure User Snippets adds some
workbench-lsp-snippet-new-global = New Global Snippets file
# $file is the snippets file's name.
workbench-lsp-snippet-exists = { $file } exists
# A row as it's matched and read: its label, then its note,
# workbench-lsp-snippet-exists.
workbench-lsp-snippet-label-note = { $label }, { $note }
workbench-lsp-snippet-cant-create = Can't create { $path }: { $error }
workbench-lsp-snippet-read-only = Can't insert a snippet: the editor is read-only
workbench-lsp-snippet-placeholder = Select a snippet
workbench-lsp-snippet-no-config-dir = No config directory to keep snippets in
workbench-lsp-snippet-file-placeholder = Select a snippets file

## The diagnostics picker (diagnostics_picker.rs).

workbench-lsp-diagnostics-no-file = No file
workbench-lsp-diagnostics-none-in-file = No problems in this file
workbench-lsp-diagnostics-none = No problems
workbench-lsp-diagnostics-no-match = No matching problems
workbench-lsp-diagnostics-file-placeholder = Search diagnostics in file…
workbench-lsp-diagnostics-placeholder = Search diagnostics…

## Running tests (test_commands.rs, test_runnables.rs).

workbench-lsp-tests-none-here = No test at the cursor
workbench-lsp-tests-none-run = No test has run yet
workbench-lsp-tests-no-file = Open a file to run its tests
workbench-lsp-tests-no-folder = Open a folder to run its tests
workbench-lsp-tests-not-saved = Save your changes to run the tests
workbench-lsp-tests-not-ready = Still reading the file: try again
# $language is the language's name, such as Rust.
workbench-lsp-tests-no-runner = No test runner for { $language }
workbench-lsp-tests-not-in-folder = { $path } isn't in the folder
# A test run's task label: $name is the test's, or the file's.
workbench-lsp-tests-task-label = Test: { $name }
# $path is the test's file, from the folder.
workbench-lsp-tests-no-cargo-package = No Cargo package holds { $path }
workbench-lsp-tests-which-crate = Can't tell which crate { $path } is in
workbench-lsp-tests-no-go-mod = No go.mod holds { $path }

## Running tasks (task_run.rs).

workbench-lsp-tasks-untrusted = Trust this folder to run its tasks
workbench-lsp-tasks-trust-folder = Trust Folder

## A tasks file's problems (task_defs.rs), as the Problems panel lists them.

# $file is the tasks file's path, from the folder; $problem one of the
# messages below.
workbench-lsp-tasks-file-problem = { $file }: { $problem }
# $entry is the task's label, or its place in the file: #2.
workbench-lsp-tasks-entry-problem = task { $entry }: { $problem }
workbench-lsp-tasks-not-an-array = `task` has to be an array of tables: [[task]]
workbench-lsp-tasks-unknown-key = unknown key `{ $key }`, ignored
workbench-lsp-tasks-repeated-label = repeats an earlier task's label, left out
# An entry that's left out: $problem is why, one of the entry problems
# below.
workbench-lsp-tasks-left-out = { $problem }, left out
workbench-lsp-tasks-not-a-table = not a table
workbench-lsp-tasks-no-label = no label
workbench-lsp-tasks-label-not-string = `label` isn't a string
workbench-lsp-tasks-empty-label = an empty label
workbench-lsp-tasks-cwd-not-string = `cwd` isn't a string
workbench-lsp-tasks-cwd-outside = cwd `{ $cwd }` is outside the project root
workbench-lsp-tasks-default-not-boolean = `default` isn't a boolean
workbench-lsp-tasks-matcher-not-string = `problem_matcher` isn't a string
workbench-lsp-tasks-empty-command = an empty command
workbench-lsp-tasks-command-wrong-type = `command` isn't a string or an array of strings
workbench-lsp-tasks-no-command = no command
workbench-lsp-tasks-env-not-table = `env` isn't a table
workbench-lsp-tasks-env-bad-name = `{ $name }` can't name an environment variable
workbench-lsp-tasks-env-not-string = env `{ $name }` isn't a string
# $group is what the entry has, not one of the three.
workbench-lsp-tasks-unknown-group = unknown group `{ $group }`, not build, test or run
workbench-lsp-tasks-group-not-string = `group` isn't a string
workbench-lsp-tasks-default-needs-group = `default` needs a group, ignored
# $first is the default task's label; $group its group, as tasks.toml
# names it.
workbench-lsp-tasks-default-taken = `{ $first }` is already the default { $group ->
        [build] build
        [test] test
       *[run] run
    } task

## A task's output read for problems (task_problems.rs).

# $matcher is what the task names.
workbench-lsp-tasks-unknown-matcher = unknown problem matcher `{ $matcher }` (expected rustc, tsc, gcc, go, eslint or python)
# A compiler's message and its code: mismatched types (E0308).
workbench-lsp-tasks-problem-code = { $message } ({ $code })
# A test's panic: the test's name and the panic's message.
workbench-lsp-tasks-panic = { $test }: { $message }
