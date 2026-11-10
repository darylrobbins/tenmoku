# A pane's find bar (find_bar.rs): its query and replace fields, their
# toggles, the match count and the replace row's buttons.

## The fields

workbench-find-placeholder = Find
workbench-find-replace-placeholder = Replace

## Toggles and buttons: their accessible names, shown in their tooltips

workbench-find-match-case = Match Case
workbench-find-whole-word = Match Whole Word
workbench-find-regex = Use Regular Expression
workbench-find-in-selection = Find in Selection
workbench-find-previous = Previous Match
workbench-find-next = Next Match
# The chevron at the bar's left that shows and hides the replace row.
workbench-find-toggle-replace = Toggle Replace
# The `AB` toggle in the replace field: each replacement takes the case of
# the match it replaces.
workbench-find-preserve-case = Preserve Case
workbench-find-replace = Replace
workbench-find-replace-all = Replace All

## The count beside the query

workbench-find-no-results = No results
# The selected match, `$current` (from 1), of the `$total` matches.
workbench-find-count =
    { $total ->
        [one] { $current } of { $total }
       *[other] { $current } of { $total }
    }
# As workbench-find-count, with more matches than the `$total` counted.
workbench-find-count-capped =
    { $total ->
        [one] { $current } of { $total }+
       *[other] { $current } of { $total }+
    }
# `$total` matches, none of them selected.
workbench-find-count-unknown =
    { $total ->
        [one] ? of { $total }
       *[other] ? of { $total }
    }
# As workbench-find-count-unknown, with more matches than the `$total`
# counted.
workbench-find-count-unknown-capped =
    { $total ->
        [one] ? of { $total }+
       *[other] ? of { $total }+
    }
# In place of the count after Replace All replaced `$count` matches.
workbench-find-replaced =
    { $count ->
        [one] Replaced { $count }
       *[other] Replaced { $count }
    }
# In place of the count when a replace gave up, changing nothing.
workbench-find-too-long = Search took too long

## The equivalent Vim command, at the row's right end

# Its accessible name. `$command` is the command as Vim takes it, such as
# `/dirty` or `:%s/dirty/needs_paint/gI`: not translated.
workbench-find-vim-command = Copy the Vim command { $command }
workbench-find-vim-command-tooltip = The same in Vim: click to copy
