# fe-workbench's Git log, its row menu and the History panel (i18n design,
# T348): `workbench-git-log-…`.

## Shared by the panels

# What a list shows while a page or a commit's files are read.
workbench-git-log-loading = Loading…

## The log (git_log.rs)

# The bottom panel's tab: the branch the log lists.
workbench-git-log-title-branch = Graph: { $name }
workbench-git-log-title-detached = Graph: HEAD (detached)
workbench-git-log-title-all = Graph: All Branches
workbench-git-log-title = Graph
workbench-git-log-no-commits = No commits yet
workbench-git-log-no-matches = No commits match
workbench-git-log-filter-placeholder = Filter by message, author or SHA
workbench-git-log-clear-filter = Clear Filter
workbench-git-log-renamed-dir = { $dir } ← { $from }
# What `git_log.open_commit_link` says for a commit whose remote's host has no
# commit pages, or that has no remote.
workbench-git-log-no-web-page = This commit's remote has no web page to open
# A renamed file in the root: the directory cell names where it came from.
workbench-git-log-renamed-from = ← { $from }
# A file row's line counts.
workbench-git-log-lines-added = +{ $added }
workbench-git-log-lines-removed = -{ $removed }
# The pill that stands for the refs a row doesn't show; $count is how many.
workbench-git-log-refs-more = +{ $count }

## A commit's card

workbench-git-log-copy-hash = Copy Commit Hash
workbench-git-log-link-github = Open on GitHub
workbench-git-log-link-browser = Open in Browser
workbench-git-log-files-changed = { $count ->
    [one] { $count } file changed
   *[other] { $count } files changed
}
workbench-git-log-insertions = { $count ->
    [one] { $count } insertion(+)
   *[other] { $count } insertions(+)
}
workbench-git-log-deletions = { $count ->
    [one] { $count } deletion(-)
   *[other] { $count } deletions(-)
}

# $ago is how long ago, $date the date.
workbench-git-log-card-dates = { $ago } ({ $date })
# A commit's trailer: $key is its key, $name who it credits.
workbench-git-log-card-trailer = { $key }: { $name }
# The card's totals as one line: $files, $added and $removed are the three
# messages above.
workbench-git-log-card-totals = { $files }, { $added }, { $removed }

## A row's menu (git_log_menu.rs)

workbench-git-log-menu-copy-subject = Copy Subject
workbench-git-log-menu-open-changes = Open Changes
workbench-git-log-menu-open-file = Open File at This Revision
workbench-git-log-menu-compare = Compare with Working Tree
# $name is the file's name, $short the commit's short id.
workbench-git-log-not-in-commit = { $name } isn't in { $short }
# The tab title of a commit's parent, the left side of its diff.
workbench-git-log-parent-rev = { $short }^
# A file opened at a commit: its tab's title.
workbench-git-log-revision-title = { $name } @ { $short }

## The History panel (git_history.rs)

workbench-git-log-history-title = History
workbench-git-log-history-title-file = History: { $name }
workbench-git-log-history-no-commits = No commits for { $name }
# $limit is how many commits the list stops at.
workbench-git-log-history-limit = { $limit ->
    [one] Showing the newest commit
   *[other] Showing the newest { $limit } commits
}
workbench-git-log-history-renamed-from = renamed from { $path }
