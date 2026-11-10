# fe-workbench's navigation and its pickers: go to definition and its kin,
# workspace and file symbols, go to line, quick open, Harpoon, local
# history, buffers, buffer lines, recent and config files, the ignore assist
# and the workbench's own file prompts (i18n design, sections 4 and 5).

## Go to definition and its kin.
##
## $kind is what was asked for: definition, declaration, type-definition or
## implementation. $source is search when a search provider gave the answer,
## which a status then names as $provider, and exact otherwise.

workbench-navigation-definitions-placeholder = Definitions
workbench-navigation-symbols-placeholder = Go to symbol in workspace…
# The status bar item naming the provider of the last answer.
workbench-navigation-source = by { $provider }
workbench-navigation-no-folder = No folder open
workbench-navigation-no-symbol = No symbol at the caret{ $source ->
        [search] { " " }(by { $provider })
       *[exact] { "" }
    }
workbench-navigation-not-found = No { $kind ->
        [declaration] declaration
        [type-definition] type definition
        [implementation] implementation
       *[definition] definition
    } found for `{ $word }`{ $source ->
        [search] { " " }(by { $provider })
       *[exact] { "" }
    }
workbench-navigation-not-found-yet = No { $kind ->
        [declaration] declaration
        [type-definition] type definition
        [implementation] implementation
       *[definition] definition
    } found for `{ $word }` yet: the project is still being indexed{ $source ->
        [search] { " " }(by { $provider })
       *[exact] { "" }
    }
workbench-navigation-found = { $count ->
        [one] { $count } { $kind ->
            [declaration] declaration
            [type-definition] type definition
            [implementation] implementation
           *[definition] definition
        }
       *[other] { $count } { $kind ->
            [declaration] declarations
            [type-definition] type definitions
            [implementation] implementations
           *[definition] definitions
        }
    } of `{ $word }`{ $source ->
        [search] { " " }(by { $provider })
       *[exact] { "" }
    }
# $error says why.
workbench-navigation-failed = Go to { $kind ->
        [declaration] declaration
        [type-definition] type definition
        [implementation] implementations
       *[definition] definition
    } failed: { $error }

## Workspace symbols.

workbench-navigation-indexing = Indexing… { $count ->
        [one] { $count } file
       *[other] { $count } files
    }
workbench-navigation-symbols-failed = Symbol search failed: { $error }
workbench-navigation-results = { $count ->
        [one] { $count } result
       *[other] { $count } results
    }{ $source ->
        [search] { " " }(by { $provider })
       *[exact] { "" }
    }
# A symbol's kind, as its row shows it.
workbench-navigation-symbol-kind = { $kind ->
        [module] module
        [class] class
        [struct] struct
        [enum] enum
        [interface] interface
        [trait] trait
        [impl] impl
        [type] type
        [function] function
        [method] method
        [constant] constant
        [macro] macro
        [table] table
        [heading] heading
        [field] field
        [variant] variant
        [variable] variable
       *[other] symbol
    }

## Go to Line/Column.

workbench-navigation-go-to-line = Go to line { $line }
workbench-navigation-go-to-line-column = Go to line { $line }, column { $column }
# Under an empty query: the caret's line, of the text's.
workbench-navigation-go-to-line-status = Line { $line } of { $lines }
workbench-navigation-go-to-line-out-of-range = Type a line number between 1 and { $lines }

## Harpoon, its commands and its list.

workbench-navigation-harpoon-no-folder = Open a folder first
workbench-navigation-harpoon-no-state-dir = Harpoon marks can't be kept: no state directory
workbench-navigation-harpoon-no-file = No file to mark
workbench-navigation-harpoon-outside-folder = { $path } isn't in the open folder
# $slot is the slot's number, 1 to 9.
workbench-navigation-harpoon-marked = Harpoon: marked { $file } as { $slot }
workbench-navigation-harpoon-already-marked = Harpoon: { $file } is already { $slot }
workbench-navigation-harpoon-full = Harpoon: the list is full ({ $count ->
        [one] { $count } file
       *[other] { $count } files
    })
workbench-navigation-harpoon-cant-mark = Harpoon: can't mark { $file }
workbench-navigation-harpoon-save-failed = Harpoon: can't save the list: { $error }
workbench-navigation-harpoon-unmarked = Harpoon: unmarked { $file }
workbench-navigation-harpoon-not-marked = Harpoon: { $file } isn't marked
workbench-navigation-harpoon-slot-empty = Harpoon: nothing marked as { $slot }
workbench-navigation-harpoon-file-missing = Harpoon: file missing: { $file }
workbench-navigation-harpoon-placeholder = Marked files…
# Beside a row whose file is gone.
workbench-navigation-harpoon-row-missing = missing
workbench-navigation-harpoon-empty = No marked files: { command-workbench-harpoon_add } marks the active one

## File symbols.

workbench-navigation-file-symbols-placeholder = Go to symbol…
workbench-navigation-symbols-parsing = Parsing…
workbench-navigation-symbols-none = No symbols
workbench-navigation-symbols-no-matches = No matching symbols
# A file with more symbols than are listed.
workbench-navigation-symbols-truncated = The first { $count ->
        [one] symbol
       *[other] { $count } symbols
    }

## Buffers.

workbench-navigation-buffers-placeholder = Buffers…
workbench-navigation-buffers-none = No open buffers
workbench-navigation-buffers-no-matches = No matching buffers

## Search Lines in Buffer.

workbench-navigation-buffer-lines-placeholder = Search lines…
workbench-navigation-buffer-lines-no-editor = No editor
workbench-navigation-buffer-lines-none = No lines
workbench-navigation-buffer-lines-no-matches = No matching lines
# A buffer with more rows than are listed: $count rows, $shown is $count
# with its thousands grouped.
workbench-navigation-buffer-lines-capped = The first { $count ->
        [one] row
       *[other] { $shown } rows
    }

## Config files.

workbench-navigation-config-files-placeholder = Open config file…
workbench-navigation-config-files-no-folder = No config folder
workbench-navigation-config-files-listing = Listing files…
workbench-navigation-config-files-none = No config files
workbench-navigation-config-files-no-matches = No matching files

## Recent folders and files.

workbench-navigation-recent-open-in-new-window = Open in New Window
workbench-navigation-recent-none = No recently opened folders or files

## Local history.

workbench-navigation-local-history-placeholder = Select a Version
# A row: when the version was ($ago is now, minutes or hours, $count of
# them, else stamp, then $stamp says when), why it was kept ($reason is
# saved or external) and its size.
workbench-navigation-local-history-row = { $ago ->
        [now] just now
        [minutes] { $count ->
            [one] { $count } minute ago
           *[other] { $count } minutes ago
        }
        [hours] { $count ->
            [one] { $count } hour ago
           *[other] { $count } hours ago
        }
       *[stamp] { $stamp }
    } · { $reason ->
        [external] Changed on disk
       *[saved] Saved
    } · { $size }
# The left side's title in a version's diff.
workbench-navigation-local-history-diff-title = Local History { $stamp }
workbench-navigation-local-history-no-file = E32: No file name
workbench-navigation-local-history-none = No local history for { $name }
workbench-navigation-local-history-read-failed = Couldn't read local history of { $name }
workbench-navigation-local-history-version-failed = Couldn't read that version of { $name }
workbench-navigation-local-history-not-a-version = Open a Local History version to restore it
workbench-navigation-local-history-restore-moved = { $name } changed while it was restored
workbench-navigation-local-history-restore-moved-detail = It wasn't restored. Try again.

## The ignore assist's hover over a pattern.
##
## $count is how many paths it matches; $paths lists those shown, as a
## fenced code block, and $more counts those left out.

workbench-navigation-ignore-matches-none = This pattern matches no files
workbench-navigation-ignore-matches-none-read = This pattern matches no files in the part of the project read
workbench-navigation-ignore-matches = { $count ->
        [one] This pattern matches { $count } file
       *[other] This pattern matches { $count } files
    }:

    { $paths }
workbench-navigation-ignore-matches-more = { $count ->
        [one] This pattern matches { $count } file
       *[other] This pattern matches { $count } files
    }:

    { $paths }

    and { $more } more
# The project was only partly read: there may be more.
workbench-navigation-ignore-matches-at-least = { $count ->
        [one] This pattern matches at least { $count } file
       *[other] This pattern matches at least { $count } files
    }:

    { $paths }
workbench-navigation-ignore-matches-at-least-more = { $count ->
        [one] This pattern matches at least { $count } file
       *[other] This pattern matches at least { $count } files
    }:

    { $paths }

    and { $more } more

## The workbench's own file prompts and toasts.

workbench-navigation-ok = OK
workbench-navigation-save-as-in-use = “{ $name }” is open in another tab.
workbench-navigation-save-as-in-use-detail = Close it first, or save under another name.
workbench-navigation-save-as-failed = “{ $name }” couldn’t be saved.
workbench-navigation-settings-cant-open = Can't open settings
workbench-navigation-keymap-cant-open = Can't open the keymap
workbench-navigation-no-config-dir = There's no config directory
workbench-navigation-cant-create = Can't create { $path }
