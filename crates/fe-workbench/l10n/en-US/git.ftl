# fe-workbench's Git strings outside the Changes panel and the log
# (i18n design, W2): `workbench-git-…`.

## Merge conflicts (git_conflicts.rs)

workbench-git-conflict-current-label = (Current Change)
workbench-git-conflict-incoming-label = (Incoming Change)
workbench-git-conflict-accept-current = Accept Current Change
workbench-git-conflict-accept-incoming = Accept Incoming Change
workbench-git-conflict-accept-both = Accept Both Changes
workbench-git-conflict-compare = Compare Changes
workbench-git-conflict-wrapped-first = Wrapped to the first conflict
workbench-git-conflict-wrapped-last = Wrapped to the last conflict
workbench-git-conflict-none = No merge conflicts found in this file
workbench-git-conflict-not-in = Editor cursor is not within a merge conflict
workbench-git-conflict-read-only = Cannot make changes, the editor is read-only
workbench-git-conflict-current-side = { $name }: Current Changes
workbench-git-conflict-incoming-side = { $name }: Incoming Changes

## Shared by the Git files

workbench-git-not-a-repository = Not in a git repository
workbench-git-no-file-name = E32: No file name
workbench-git-yes = Yes
workbench-git-no = No
workbench-git-cancel = Cancel
workbench-git-error = git: { $error }
workbench-git-not-supported-yet = Not supported yet: { $what }

## Ahead and behind the upstream (git.rs): on the branch widget and the
## Changes panel's header, `main ↑2 ↓1`

workbench-git-sync-ahead = ↑{ $count }
workbench-git-sync-behind = ↓{ $count }
# The branch widget's tooltip (title_bar.rs) on a branch with an upstream.
# The branch widget's tooltip on a branch with an upstream, as of the last fetch.
workbench-git-sync-switch-branch-counts = Switch Branch… ({ $ahead } to push, { $behind } to pull from { $upstream }, as of the last fetch)
# The branch widget's tooltip on a branch with no upstream.
workbench-git-sync-no-upstream = No upstream: Push publishes this branch

## Fetch, pull and push (git_remote.rs)

workbench-git-remote-already-running = A fetch, pull or push is already running; wait for it to finish
workbench-git-remote-force-detail = This replaces the remote branch, if it's still where your last fetch saw it.
workbench-git-remote-target-changed = the branch or its upstream changed since you were asked, so nothing was pushed; run it again
workbench-git-remote-no-commits = the branch has no commits yet; commit first
workbench-git-remote-git-directory = the git directory: { $error }
workbench-git-remote-force-question = Force-push { $branch } to { $upstream } with lease?
workbench-git-remote-force-pushed = Force-pushed { $branch } to { $upstream } with lease
workbench-git-remote-fetched = Fetched
workbench-git-remote-pulled = Pulled
workbench-git-remote-pushed = Pushed
workbench-git-remote-published = Pushed { $branch } to { $remote } and set its upstream to { $remote }/{ $branch }
workbench-git-remote-several-remotes = there are several remotes and none is called origin; run git push -u with the one you want from a terminal
workbench-git-remote-fetching = Fetching…
workbench-git-remote-pulling = Pulling…
workbench-git-remote-force-pushing = Force-pushing…
workbench-git-remote-pushing = Pushing…
workbench-git-remote-updating = Updating…
# Update Project's outcome: $upstream is `origin/main`.
workbench-git-remote-updated = Updated { $branch } from { $upstream }
workbench-git-remote-up-to-date = { $branch } is already up to date with { $upstream }
# Why Update Project merged nothing: the cause, after `Can't update: `.
workbench-git-remote-update-moved = the branch or its upstream changed while it fetched, so nothing was merged; run it again
workbench-git-remote-no-remote =
    Can't { $verb ->
        [fetch] fetch
        [pull] pull
        [update] update
        [force-push] force-push
       *[push] push
    }: there's no upstream or origin to use. Add a remote with git remote add origin <url>.
workbench-git-remote-no-upstream =
    Can't { $verb ->
        [fetch] fetch
        [pull] pull
        [update] update
        [force-push] force-push
       *[push] push
    }: the branch has no upstream. Run Git: Push to publish it first.
workbench-git-remote-detached =
    Can't { $verb ->
        [fetch] fetch
        [pull] pull
        [update] update
        [force-push] force-push
       *[push] push
    }: HEAD is detached. Switch to a branch first.
workbench-git-remote-conflicts =
    The { $verb ->
        [fetch] fetch
        [pull] pull
        [update] update
        [force-push] force-push
       *[push] push
    } stopped on conflicts in { $count ->
        [one] 1 file
       *[other] { $count } files
    }. Resolve them, then commit.
workbench-git-remote-diverged =
    Can't { $verb ->
        [fetch] fetch
        [pull] pull
        [update] update
        [force-push] force-push
       *[push] push
    }: the branch and its upstream have diverged, so it can't fast-forward. Merge or rebase them from a terminal.
workbench-git-remote-lease-rejected =
    Can't { $verb ->
        [fetch] fetch
        [pull] pull
        [update] update
        [force-push] force-push
       *[push] push
    }: the remote branch changed since your last fetch, or the remote refused it. Fetch, check what changed, and try again.
workbench-git-remote-rejected =
    Can't { $verb ->
        [fetch] fetch
        [pull] pull
        [update] update
        [force-push] force-push
       *[push] push
    }: the remote refused it, usually because it has commits you don't. Pull, then push again.
workbench-git-remote-auth =
    Can't { $verb ->
        [fetch] fetch
        [pull] pull
        [update] update
        [force-push] force-push
       *[push] push
    }: the remote wants credentials git can't ask for here, or refused them. Set up a credential helper or ssh key, then try again.
workbench-git-remote-failed = git failed
workbench-git-remote-cant =
    Can't { $verb ->
        [fetch] fetch
        [pull] pull
        [update] update
        [force-push] force-push
       *[push] push
    }: { $cause }

## Credential prompts for fetch, pull and push (git_askpass.rs)

# The prompt's name to a screen reader; $remote is the remote's name, as origin.
workbench-git-askpass-title = Sign in to { $remote }
# Under the masked field, whose placeholder is what git or ssh asked.
workbench-git-askpass-status = { $remote } asks for this. Enter sends it to git, Escape cancels; it isn't saved.

## The branches menu (git_branch_menu.rs), from the title bar's branch widget

# Its accessible name.
workbench-git-branches-menu = Git Branches
# Its search row, before anything's typed.
workbench-git-branches-search = Search branches
workbench-git-branches-no-match = No branches match
# The actions over the branches.
workbench-git-branches-update = Update Project
workbench-git-branches-commit = Commit…
workbench-git-branches-push = Push
# Push, for a branch with no upstream.
workbench-git-branches-publish = Publish Branch
workbench-git-branches-fetch = Fetch
workbench-git-branches-new = New Branch…
# The sections' titles.
workbench-git-branches-recent = Recent
workbench-git-branches-local = Local
workbench-git-branches-remote = Remote
# A branch's actions: $name is the branch, as `feat` or `origin/feat`;
# $into is the branch checked out.
workbench-git-branches-checkout = Checkout
workbench-git-branches-new-from = New Branch from '{ $name }'…
workbench-git-branches-merge = Merge '{ $name }' into '{ $into }'
workbench-git-branches-rename = Rename…
workbench-git-branches-delete = Delete
workbench-git-branches-pull = Pull
workbench-git-branches-update-branch = Update
# The name's picker's row: $start is the branch it's made from.
workbench-git-branches-create-from = Create branch '{ $name }' from '{ $start }' and switch to it
workbench-git-branches-rename-to = Rename '{ $old }' to '{ $name }'
# How they went.
workbench-git-branches-merged = Merged { $name } into { $into }
workbench-git-branches-merge-conflicts =
    The merge stopped on conflicts in { $count ->
        [one] 1 file
       *[other] { $count } files
    }. Resolve them, then commit.
workbench-git-branches-merge-overwrite = Can't merge { $name }: it would overwrite local changes to these files. Commit or stash them first.
workbench-git-branches-cant-merge = Can't merge { $name }
workbench-git-branches-renamed = Renamed { $old } to { $name }
workbench-git-branches-cant-rename = Can't rename { $old }
workbench-git-branches-deleted = Deleted branch { $name }
workbench-git-branches-cant-delete = Can't delete { $name }
# The question before deleting a branch that isn't fully merged.
workbench-git-branches-not-merged = { $name } isn't fully merged.
workbench-git-branches-not-merged-detail = Deleting it loses the commits on it that aren't on its upstream, or on HEAD's branch if it has none.
workbench-git-branches-delete-anyway = Delete
workbench-git-branches-cancel = Cancel

## Stash (git_stash.rs)

workbench-git-stash-none = There are no stashes in the repository.
workbench-git-stash-nothing-to-save = No local changes to save
workbench-git-stash-changed = The stash changed since it was listed; pick again
workbench-git-stash-cant-stash = Can't stash
workbench-git-stash-cant-pop = Can't pop stash@{ "{" }{ $index }{ "}" }
workbench-git-stash-cant-apply = Can't apply stash@{ "{" }{ $index }{ "}" }
workbench-git-stash-cant-list = Can't list the stashes
workbench-git-stash-message-placeholder = Stash message (optional)
workbench-git-stash-pick-pop = Pick a stash to pop
workbench-git-stash-pick-apply = Pick a stash to apply
workbench-git-stash-entry-kept =
    { "\u000A" }The stash entry is kept in case you need it again.
workbench-git-stash-conflicts = Conflicts applying stash@{ "{" }{ $index }{ "}" }
workbench-git-stash-row = stash@{ "{" }{ $index }{ "}" }: { $message }
workbench-git-stash-save = Stash local changes
workbench-git-stash-save-untracked = Stash local changes, untracked files included
workbench-git-stash-save-as = Stash local changes, as '{ $message }'
workbench-git-stash-save-untracked-as = Stash local changes, untracked files included, as '{ $message }'

## Diff views (git_diff.rs)

workbench-git-diff-no-other-modifiable = E793: No other buffer in diff mode is modifiable
workbench-git-diff-compare-with = Compare with…
workbench-git-diff-working-tree = Working Tree
workbench-git-diff-saved = Saved
workbench-git-diff-index = Index
workbench-git-diff-write-side = Not supported yet: writing a diff side to another file
workbench-git-diff-too-large = Too large to compare: { $name }
workbench-git-diff-cant-open = Can't open { $path }: { $error }
workbench-git-diff-no-such-file = Can't open { $path }: No such file
workbench-git-diff-saved-title = { $name } ({ $label } ↔ { $working })
workbench-git-diff-no-longer-open = { $name } is no longer open

## Staging, discarding and reverting (git_stage.rs)

workbench-git-stage-irreversible = This is IRREVERSIBLE! This file will be FOREVER LOST if you proceed.
workbench-git-stage-some-irreversible = This is IRREVERSIBLE! The untracked files among them will be FOREVER LOST if you proceed.
workbench-git-stage-nothing-to-discard = No changes to discard
workbench-git-stage-no-change-to-stage = No change to stage at this line
workbench-git-stage-no-change-to-revert = No change to revert at this line
workbench-git-stage-git-side = Stage from the Working Tree side
workbench-git-stage-nothing-staged = No staged change to undo
workbench-git-stage-index-changed = Index changed since; not undone
workbench-git-stage-moved = Not saved or staged: the editor no longer edits { $name }
workbench-git-stage-window-closed = Not saved: the window closed
workbench-git-stage-not-utf8 = Can't stage part of a file that isn't UTF-8
workbench-git-stage-cant-read = Not staged: can't read { $name }: { $error }
workbench-git-stage-now-conflicted = { $name } now has merge conflicts; not staged
workbench-git-stage-confirm-merge = Are you sure you want to stage { $name } with merge conflicts?
workbench-git-stage-confirm-merge-many =
    Are you sure you want to stage { $count ->
        [one] 1 file
       *[other] { $count } files
    } with merge conflicts?
workbench-git-stage-button = Stage
workbench-git-stage-confirm-discard = Are you sure you want to discard changes in { $name }?
workbench-git-stage-discard-file = Discard File
workbench-git-stage-confirm-delete = Are you sure you want to DELETE { $name }?
workbench-git-stage-delete-file = Delete File
workbench-git-stage-confirm-discard-many =
    Are you sure you want to discard changes in { $count ->
        [one] 1 file
       *[other] { $count } files
    }?
workbench-git-stage-discard-files =
    Discard { $count ->
        [one] 1 File
       *[other] { $count } Files
    }
workbench-git-stage-not-in-index = { $name } is not in the index
workbench-git-stage-unresolved = { $name } has unresolved conflicts
workbench-git-stage-not-a-file = { $name } is not a file in the index
workbench-git-stage-changed-since = Changed since you were asked, so not discarded: { $names }
workbench-git-failure = Git: { $first }

## Blame (git_blame.rs, git_blame_column.rs)

workbench-git-blame-not-committed = Not Committed Yet
workbench-git-blame-untracked = Not tracked by git
workbench-git-blame-too-large = Too large to blame
workbench-git-blame-just-now = just now
workbench-git-blame-ago-minutes =
    { $count ->
        [one] 1 minute ago
       *[other] { $count } minutes ago
    }
workbench-git-blame-ago-hours =
    { $count ->
        [one] 1 hour ago
       *[other] { $count } hours ago
    }
workbench-git-blame-ago-days =
    { $count ->
        [one] 1 day ago
       *[other] { $count } days ago
    }
workbench-git-blame-ago-weeks =
    { $count ->
        [one] 1 week ago
       *[other] { $count } weeks ago
    }
workbench-git-blame-ago-months =
    { $count ->
        [one] 1 month ago
       *[other] { $count } months ago
    }
workbench-git-blame-ago-years =
    { $count ->
        [one] 1 year ago
       *[other] { $count } years ago
    }
workbench-git-blame-label = { $summary }, { $author } ({ $ago })
workbench-git-blame-popover = **{ $author }** { $email }
    { $date } ({ $ago })

    { $message }

    { $short }
workbench-git-blame-said = git blame said: { $out }
workbench-git-blame-column = { $when } { $author }

## Go to tracked file (git_files_picker.rs)

workbench-git-files-listing = Listing files…
workbench-git-files-no-folder = No folder open
workbench-git-files-not-repo = Not a git repository
workbench-git-files-none = No files
workbench-git-files-no-match = No matching files
workbench-git-files-count =
    { $count ->
        [one] 1 file
       *[other] { $count } files
    }
workbench-git-files-placeholder = Go to tracked file…
