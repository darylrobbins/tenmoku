# fe-git's messages: why a git operation failed.

git-error = git: { $message }
git-missing = git executable not found on PATH
git-command-failed = git { $args } failed ({ $code }): { $stderr }
git-outside-worktree = { $path } is outside the working tree
git-invalid-path = { $path } is not a valid path
git-is-directory = { $path } is a directory; discard its files
git-head-unreadable = HEAD unreadable after commit
git-no-trace-name = no unused trace directory name
git-head-nowhere = HEAD points nowhere
git-rev-list-unexpected = unexpected rev-list output: { $text }
git-commit-ambiguous = commit { $commit } is ambiguous
git-commit-missing = no commit { $commit }

git-switch-would-overwrite =
    { $count ->
        [one] local changes would be overwritten by switching branches ({ $count } files)
       *[other] local changes would be overwritten by switching branches ({ $count } files)
    }
git-switch-invalid-name = invalid branch name
git-switch-not-found = no such branch
git-switch-failed =
    git failed:
    { $output }

git-branch-exists = a branch of that name already exists
git-branch-current = the current branch can't be deleted
git-branch-not-merged = the branch isn't fully merged
git-branch-failed =
    git branch failed:
    { $output }

git-merge-would-overwrite =
    { $count ->
        [one] local changes would be overwritten by the merge ({ $count } file)
       *[other] local changes would be overwritten by the merge ({ $count } files)
    }
git-merge-failed =
    git merge failed:
    { $output }

git-commit-empty-message = empty commit message
git-commit-nothing-staged = nothing staged to commit
git-commit-hook-failed =
    { $hook } hook failed:
    { $output }
git-commit-failed =
    git commit failed ({ $code }):
    { $output }

git-remote-none = no remote to fetch from
git-remote-no-upstream = the current branch has no upstream branch
git-remote-detached = HEAD is detached
git-remote-conflicts =
    { $count ->
        [one] conflicts in { $count } files
       *[other] conflicts in { $count } files
    }
git-remote-not-fast-forward = not possible to fast-forward
git-remote-rejected =
    push rejected:
    { $output }
git-remote-auth =
    authentication failed:
    { $output }
git-askpass-cancelled = the credential prompt was cancelled
git-remote-failed =
    git failed:
    { $output }
git-signing-refused = signing is on and could ask for a passphrase, so this wasn't done; run it from a terminal
git-signing-commit = commit signing: { git-signing-refused }
git-signing-push = push.gpgSign: { git-signing-refused }

git-stash-nothing = No local changes to save
git-stash-conflict =
    conflicts applying the stash; the stash entry is kept:
    { $conflicts }
git-stash-failed =
    git stash failed:
    { $output }
