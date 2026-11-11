# The Search panel (search_panel.rs) and the project-wide replace behind it
# (search_replace.rs): its fields, toggles, header actions, status lines,
# the Replace All prompt, and why a file was left as it was.

## The fields

workbench-search-placeholder = Search
workbench-search-replace-placeholder = Replace
workbench-search-include-placeholder = files to include
workbench-search-exclude-placeholder = files to exclude

## Toggles and buttons: their accessible names, shown in their tooltips

workbench-search-match-case = Match Case
workbench-search-whole-word = Match Whole Word
workbench-search-regex = Use Regular Expression
workbench-search-details = Toggle Search Details
# The chevron at the left of the query that shows and hides the replace field.
workbench-search-toggle-replace = Toggle Replace
# The `AB` toggle in the replace field: each replacement takes the case of
# the match it replaces.
workbench-search-preserve-case = Preserve Case
workbench-search-replace-all = Replace All
# A result row's button: replace in that file or folder.
workbench-search-replace-in-row = Replace
# A result row's button: take that file or folder out of the results.
workbench-search-dismiss = Dismiss

## The dock header's actions: tooltips

workbench-search-refresh = Refresh
workbench-search-clear = Clear Search Results
workbench-search-view-as-list = View as List
workbench-search-view-as-tree = View as Tree
workbench-search-expand-all = Expand All
workbench-search-collapse-all = Collapse All

## The empty state, when nothing matched

workbench-search-empty-heading = No results
workbench-search-empty-hint = Try a different query, or loosen the filters.

## The status line under the fields

workbench-search-no-folder = No folder open
workbench-search-searching = Searching…
# `$message` is the glob that didn't parse, `$glob`, and why.
workbench-search-bad-glob = Invalid glob { $glob }: { $message }
# Search stopped at its limit of `$max` results.
workbench-search-truncated =
    { $max ->
        [one] Showing the first result
       *[other] Showing the first { $max } results
    }
workbench-search-no-results = No results
# The `$results` found in `$files` files.
workbench-search-summary =
    { $results ->
        [one]
            { $files ->
                [one] 1 result in 1 file
               *[other] 1 result in { $files } files
            }
       *[other]
            { $files ->
                [one] { $results } results in 1 file
               *[other] { $results } results in { $files } files
            }
    }
workbench-search-replacing = Replacing…
workbench-search-cannot-read-folder = Can't read the project folder

## Replace All's prompt

# `$occurrences` matches in `$files` files, replaced with `$template`.
workbench-search-confirm-replace =
    { $occurrences ->
        [one]
            { $files ->
                [one] Replace 1 occurrence across 1 file with “{ $template }”?
               *[other] Replace 1 occurrence across { $files } files with “{ $template }”?
            }
       *[other]
            { $files ->
                [one] Replace { $occurrences } occurrences across 1 file with “{ $template }”?
               *[other] Replace { $occurrences } occurrences across { $files } files with “{ $template }”?
            }
    }
workbench-search-confirm-detail = Files that aren't open are saved, and can't be undone.
workbench-search-confirm-ok = Replace
workbench-search-confirm-cancel = Cancel

## The status line after a replace

workbench-search-replaced-nothing = Replaced nothing
workbench-search-replaced =
    { $occurrences ->
        [one]
            { $files ->
                [one] Replaced 1 occurrence in 1 file
               *[other] Replaced 1 occurrence in { $files } files
            }
       *[other]
            { $files ->
                [one] Replaced { $occurrences } occurrences in 1 file
               *[other] Replaced { $occurrences } occurrences in { $files } files
            }
    }
# The message above, and the files left as they were: `$skipped` is a list of
# workbench-search-skipped-file.
workbench-search-with-skipped = { $message }; skipped { $skipped }
# A file left as it was, `$rel` from the project folder, and `$why`.
workbench-search-skipped-file = { $rel } ({ $why })

## Why a file was left as it was

workbench-search-why-moved = the project folder has moved
workbench-search-why-outside = it isn't in the project
workbench-search-why-gone = it no longer exists
workbench-search-why-left-project = it's no longer in the project
workbench-search-why-not-regular = it's no longer a regular file
workbench-search-why-unsupported = can't check it on this system
workbench-search-why-changed = it changed on disk since it was read
workbench-search-why-cannot-tell = can't tell whether it changed
workbench-search-why-binary = it's binary
workbench-search-why-unreadable = can't read it: { $error }
workbench-search-why-not-saved = not saved: { $error }
# `$label` is the file's encoding.
workbench-search-why-invalid = it isn't valid { $label }: { $error }
workbench-search-why-unwritable = it can't be written as { $label }: { $error }
# `$why` is one of the reasons above, for a file replaced in its open editor.
workbench-search-why-not-in-editor = { $why }: saved, but not in its editor
