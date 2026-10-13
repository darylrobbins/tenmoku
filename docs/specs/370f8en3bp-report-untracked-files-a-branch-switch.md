---
task_uid: "01m4t7b1hrt5jxh2370f8en3bp"
task: "T637"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "switch to a branch that has a file where an untracked file sits inside a directory of the same name returns WouldOverwrite with that directory's absolute path, leaves HEAD and the untracked file unchanged (test switch_reports_untracked_files_in_a_directory_as_would_overwrite)"
    required: true
    manual: false
  - id: "C2"
    text: "create_branch(\"@{-1}\") returns InvalidName and creates or switches nothing, whether the previous branch still exists or was deleted (test create_branch_rejects_names_git_would_expand)"
    required: true
    manual: false
  - id: "C3"
    text: "All existing fe-git tests pass, including switch_branch.rs"
    required: true
    manual: false
---

# Report untracked files a branch switch would lose inside a directory as WouldOverwrite, and reject branch names git would expand like @{-1}, in fe-git

## Problem

Two misclassifications in `crates/fe-git/src/branch.rs`, both confirmed at base e6988aba (git 2.42.1)
with the tests below:

1. **`Repo::switch` returns `Failed`, not `WouldOverwrite`, when the switch would lose untracked files
   inside a directory.** With `main` having nothing at `d`, branch `other` adding a file `d`, and an
   untracked `d/u` on `main`, git refuses with

   ```
   error: Updating the following directories would lose untracked files in them:
   	d

   Aborting
   ```

   `overwritten_paths` only looks for "would be overwritten", so `run_switch` falls through to
   `SwitchError::Failed { output }`. The branch picker (T556) then can't offer "stash and switch" for a
   case `stash_and_switch` (which stashes untracked files) would handle.

2. **`Repo::create_branch` accepts `@{-1}`.** `git check-ref-format --branch @{-1}` *expands* the
   name to the previous branch and exits 0, and `git switch -c @{-1}` expands it the same way. So:
   - if the previous branch still exists, the call returns `Failed { "fatal: a branch named 'other'
     already exists" }` instead of `InvalidName`;
   - if the previous branch was deleted, it silently **creates and switches to a branch with the old
     name** (`gone`), not the name the user typed.

   `@{-N}` is the only form that passes the check this way here (`HEAD`, `@{u}`, `main@{1}`, `-` are
   already rejected by git); `@` passes and git really creates `refs/heads/@`, which is left alone.

## Change

In `crates/fe-git/src/branch.rs` only:

- `create_branch`: treat the name as invalid unless `git check-ref-format --branch <name>` succeeds
  **and** its stdout (trimmed) is exactly `name`, so any name git would expand is `InvalidName`. Keep the
  leading-`-` check. Nothing is created or switched on `InvalidName`.
- `overwritten_paths` (or `run_switch`): also recognise git's "would lose untracked files in them"
  refusal and return its tab-indented entries, so `switch` returns `WouldOverwrite` with those entries
  joined to the workdir (for the case above, `[workdir/d]`: the directory git lists). Update the
  `WouldOverwrite` doc comment to say the paths may be directories.
- No other refusal changes class; `Failed` stays the fallback.

## Tests

New file `crates/fe-git/tests/branch_errors.rs` (copy at
`/private/tmp/claude-501/-Users-daryl-code-fastedit/b79f4486-f1fb-414c-9618-92ef66c7b61b/scratchpad/branch_errors.rs`;
it builds against the base API and both tests fail there):

- `switch_reports_untracked_files_in_a_directory_as_would_overwrite`: the scenario in Problem 1;
  expects `WouldOverwrite { paths: [workdir/d] }`, HEAD still `main`, `d/u` intact.
- `create_branch_rejects_names_git_would_expand`: `create_branch("@{-1}")` is `InvalidName` with the
  branch list unchanged, both while the previous branch exists and after it was deleted (HEAD stays
  `main`, no `gone` branch appears).

A sketch fix (stdout comparison + second marker string) passes these and all 45 existing fe-git tests.
