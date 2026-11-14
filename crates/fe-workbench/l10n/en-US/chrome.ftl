# fe-workbench's window chrome (i18n design, T1538): the title bar, the
# window's own prompts, and the branch widget's pickers: `workbench-chrome-…`.

## The title bar (title_bar.rs)

# The buttons' accessible names and tooltips; a tooltip adds the key.
workbench-chrome-back = Go Back
workbench-chrome-forward = Go Forward
# The project widget's accessible name. $name is the root folder's name.
workbench-chrome-project = Project: { $name }
workbench-chrome-open-recent = Open Recent…
# The branch widget's accessible name. $name is the branch, or HEAD's short
# id when detached.
workbench-chrome-branch = Branch: { $name }
# The same with how far the branch is from its upstream: $counts is `↑2 ↓1`.
workbench-chrome-branch-synced = Branch: { $name } { $counts }
workbench-chrome-branches-menu = Branches…
workbench-chrome-go-to-file = Go to File
workbench-chrome-settings = Settings

## The terminal (workbench_chrome.rs)

workbench-chrome-terminal-name-placeholder = Terminal name…

## Switching and creating branches (git_branch.rs)

workbench-chrome-branch-create-row = + Create new branch…
workbench-chrome-branch-not-a-repository = Not in a git repository
workbench-chrome-branch-invalid-name = Not a valid branch name
workbench-chrome-branch-overwrite = Your local changes would be overwritten by checkout.
workbench-chrome-branch-stash-and-checkout = Stash & Checkout
workbench-chrome-branch-force-checkout = Force Checkout
workbench-chrome-branch-cancel = Cancel
# After the first ten paths the switch would overwrite: how many more.
workbench-chrome-branch-overwrite-more = { $count ->
    [one] and { $count } more
   *[other] and { $count } more
}
workbench-chrome-branch-cant-list = Can't list the branches
workbench-chrome-branch-pick-placeholder = Select a ref to checkout
workbench-chrome-branch-name-placeholder = Branch name
workbench-chrome-branch-not-found = Not found: { $name }
workbench-chrome-branch-cant-create = Can't create branch { $name }
workbench-chrome-branch-cant-switch = Can't switch to { $name }
# A branch row's mark on the branch checked out.
workbench-chrome-branch-current = current
workbench-chrome-branch-create = Create branch '{ $name }' and switch to it

# How long ago a branch's tip was committed.
workbench-chrome-branch-ago-now = just now
workbench-chrome-branch-ago-minutes = { $count ->
    [one] { $count } minute ago
   *[other] { $count } minutes ago
}
workbench-chrome-branch-ago-hours = { $count ->
    [one] { $count } hour ago
   *[other] { $count } hours ago
}
workbench-chrome-branch-ago-days = { $count ->
    [one] { $count } day ago
   *[other] { $count } days ago
}
workbench-chrome-branch-ago-weeks = { $count ->
    [one] { $count } week ago
   *[other] { $count } weeks ago
}
workbench-chrome-branch-ago-months = { $count ->
    [one] { $count } month ago
   *[other] { $count } months ago
}
workbench-chrome-branch-ago-years = { $count ->
    [one] { $count } year ago
   *[other] { $count } years ago
}
