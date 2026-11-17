### fastedit's messages: the shell command, theme settings, the updater, the
### speller and grammar installs.

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

## The status bar's bell: its accessible name and tooltip. $count is how many
## notifications need the user.

workbench-inbox-bell =
    { $count ->
        [0] Notifications
        [one] Notifications, { $count } needs you
       *[other] Notifications, { $count } need you
    }
workbench-git-stash-cant-pop = Can't pop stash@{ "{" }{ $index }{ "}" }
workbench-extensions-plugin-rebuild = the plugin API is a preview and can change in any release; rebuild the plugin against this release
# A plugin that doesn't link against this host's world: why, then what to do.
workbench-extensions-plugin-unlinked = { $error }; { workbench-extensions-plugin-rebuild }

## Command titles, and a command's description as its message's
## `.description` (fe-editor).

command-editor-move = Move
    .description = Moves every caret `count` times (at most 10,000) by `motion`, extending the selections if `extend`.
command-editor-indent = Indent

## Not in our catalogues yet, so written for this sample: a term with an
## attribute, term and attribute references, and the built-in functions with
## named arguments.

-app-name = Tenmoku
    .gender = neuter
workbench-about = About { -app-name }
workbench-about-title = { command-editor-move.description }
workbench-update-ready =
    { -app-name.gender ->
       *[neuter] { -app-name(case: "nominative") } is ready to restart
    }
workbench-update-size = { NUMBER($bytes, maximumFractionDigits: 1) } MB to download
workbench-update-checked = Last checked { DATETIME($date, month: "long", day: "numeric") }
