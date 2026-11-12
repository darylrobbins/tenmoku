# fe-workbench's Changes panel and its commit box (i18n design, T345):
# `workbench-changes-…`.

## The panel (source_control.rs, git_panel.rs)

# The sidebar's Changes panel, and its view in Source Control.
workbench-changes-title = Changes
workbench-changes-not-a-repository = Not a git repository
workbench-changes-none = No changes

## The groups' headers

workbench-changes-group-merge = Merge Changes
workbench-changes-group-staged = Staged Changes
workbench-changes-group-changes = Changes

## A row's and a header's checkbox and buttons: their tooltips and
## accessible names

workbench-changes-open-file = Open File
workbench-changes-discard = Discard Changes
workbench-changes-discard-all = Discard All Changes
workbench-changes-stage = Stage Changes
workbench-changes-stage-all = Stage All Changes
workbench-changes-stage-all-merge = Stage All Merge Changes
workbench-changes-unstage = Unstage Changes
workbench-changes-unstage-all = Unstage All Changes

## The sync header's buttons (git_panel.rs): their tooltips and accessible
## names. Publish Branch is Push's on a branch with no upstream.

workbench-changes-pull = Pull
workbench-changes-push = Push
workbench-changes-publish = Publish Branch
workbench-changes-fetch = Fetch

## The commit box (git_panel.rs)

# The empty box's placeholder.
workbench-changes-message = Message
# The empty box's placeholder after a commit was refused for an empty
# message, and the warning under a box of only blanks or comments.
workbench-changes-enter-a-message = Enter a commit message
workbench-changes-amend = Amend
# The commit button. $count is how many files are staged; with none, a
# commit asks to stage every change first.
workbench-changes-commit = { $count ->
    [0] Commit
    [one] Commit { $count } file
   *[other] Commit { $count } files
}
# The commit button with the key that commits from the box. $label is the
# button's own text, and $key the key as this platform prints it, such as
# ⌘↩ or Ctrl+Enter.
workbench-changes-commit-with-key = { $label } { $key }
# The commit button while Amend is checked.
workbench-changes-commit-amend = Amend
workbench-changes-committing = Committing…
# The commit button's split part: it commits, then pushes.
workbench-changes-and-push = and push
# The counter in the box's corner. $count is how many characters the caret's
# row has, and $limit how many a row should have at most.
workbench-changes-counter = { $count }/{ $limit }

## Committing (git_commit.rs)

workbench-changes-message-kept = Message kept; :wq commits
workbench-changes-e37 = E37: No write since last change (add ! to override)
workbench-changes-e99 = E99: Current buffer is not in diff mode
workbench-changes-not-in-the-box = Not supported in the commit box
workbench-changes-no-repository = Not in a git repository
workbench-changes-nothing-staged = There are no staged changes to commit.
workbench-changes-stage-all-detail = Would you like to stage all your changes and commit them directly?
workbench-changes-stage-all-and-commit = Stage All and Commit
workbench-changes-cancel = Cancel
workbench-changes-ok = OK
# $hook is the hook's name, such as pre-commit.
workbench-changes-hook-failed = Git: { $hook } hook failed
workbench-changes-commit-failed = Git: commit failed
