# fe-workbench's commit diff: a Graph commit's change to all its files as one
# tab, and the commit lists' row texts the History and Git Log panels share
# (i18n design, sections 4 and 5).

## The tab.

# The tab's title before the files are read: the commit's short id and its
# summary.
workbench-diff-title = { $short } - { $summary }
# Once they are: the title above, then how many files the commit changed.
workbench-diff-title-files =
    { $count ->
        [one] { $title } (1 file)
       *[other] { $title } ({ $count } files)
    }
# The window's title while the tab is active.
workbench-diff-window-title = { $title } — { $app }
workbench-diff-loading = Loading…

## The toolbar's buttons, their names and tooltips.

workbench-diff-previous-change = Previous Change
workbench-diff-next-change = Next Change
workbench-diff-collapse-all = Collapse All
workbench-diff-expand-all = Expand All

## A file's section.

# Why a section shows no diff, after its path.
workbench-diff-binary-not-shown = Binary file not shown
workbench-diff-large-not-shown = Large file not shown
# A renamed file's header: the old path, then the new.
workbench-diff-renamed = { $from } → { $path }
# A section header's text when it shows no diff: the path, then why.
workbench-diff-not-shown = { $path } · { $why }
# A renamed file's directory in its header: its old path after the new
# directory, or alone for a file at the top of the working tree.
workbench-diff-renamed-from = ← { $from }
workbench-diff-dir-renamed-from = { $dir } ← { $from }
# A fold of unchanged rows, which a click shows.
workbench-diff-hidden-lines =
    { $count ->
        [one] { $count } hidden line
       *[other] { $count } hidden lines
    }
# A hunk's header row: the symbols around it, outermost first and joined by
# ›, then how many rows it removes and adds.
workbench-diff-hunk-header =
    { $count ->
        [one] { $symbol } · { $count } change
       *[other] { $symbol } · { $count } changes
    }
# With no symbol around it.
workbench-diff-hunk-changes =
    { $count ->
        [one] { $count } change
       *[other] { $count } changes
    }

## A commit row in the History and Git Log panels: how long ago it was made,
## and its texts.

workbench-diff-just-now = just now
workbench-diff-minutes-ago =
    { $count ->
        [one] { $count } minute ago
       *[other] { $count } minutes ago
    }
workbench-diff-hours-ago =
    { $count ->
        [one] { $count } hour ago
       *[other] { $count } hours ago
    }
workbench-diff-days-ago =
    { $count ->
        [one] { $count } day ago
       *[other] { $count } days ago
    }
workbench-diff-weeks-ago =
    { $count ->
        [one] { $count } week ago
       *[other] { $count } weeks ago
    }
workbench-diff-months-ago =
    { $count ->
        [one] { $count } month ago
       *[other] { $count } months ago
    }
workbench-diff-years-ago =
    { $count ->
        [one] { $count } year ago
       *[other] { $count } years ago
    }
# A commit row's first text: the short id, two spaces, the summary.
workbench-diff-commit-summary = { $short }  { $summary }
# Its second: the author, then how long ago.
workbench-diff-commit-byline = { $author }, { $ago }
