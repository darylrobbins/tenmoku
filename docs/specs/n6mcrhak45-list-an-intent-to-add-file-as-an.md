---
task_uid: "01m4t9kmhqm5gjjpn6mcrhak45"
task: "T106"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-git test status_split_intent_to_add::intent_to_add_file_is_unstaged_added: after git add -N new in a repository with one commit, Repo::status_split() is exactly [(new, None, Some(ChangeKind::Added))]"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git tests status_split_intent_to_add::staged_new_file_is_staged_added ([(new, Some(Added), None)] after git add -N then git add) and status_split_intent_to_add::intent_to_add_file_deleted_from_disk_is_unstaged_deleted ([(new, None, Some(Deleted))]) pass"
    required: true
    manual: false
  - id: "C3"
    text: "Repo::status and crates/fe-git/src/lib.rs are unchanged; the existing fe-git tests pass unchanged; cargo test -p fe-git and cargo clippy -p fe-git --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# List an intent-to-add file as an unstaged addition in fe-git's split status, as git status does, not as staged (Mac T639)

## Problem

`Repo::status_split` (crates/fe-git/src/index.rs) splits each changed file into its staged (HEAD to
index) and unstaged (index to working tree) halves for the Changes panel. It maps gix's
`EntryStatus::IntentToAdd` to `staged: Some(Added)`, but an intent-to-add entry (`git add -N`) is not
staged: `git status --porcelain` shows it as " A" (worktree column), `git diff --cached` doesn't list it,
and `git commit` leaves it out. A panel built on `status_split` would list the file under Staged
Changes although a commit won't include it, and offer to unstage it rather than stage it.

Checked 2026-10-13 at e6988aba: after `git add -N new`, `git status --porcelain` prints " A new" and
`status_split()` gives `(new, Some(Added), None)`. An intent-to-add file deleted from disk already comes
out as git shows it (" D": `(None, Some(Deleted))`).

## Change

In crates/fe-git/src/index.rs `status_split`, map `EntryStatus::IntentToAdd` to
`slot.1 = Some(ChangeKind::Added)` (unstaged), not `slot.0`. Say so in the doc comments of
`status_split` and `SplitStatus` ("an intent-to-add file is `unstaged: Some(Added)`, as git status
shows it"). `Repo::status` (lib.rs, combined view) is unchanged and out of scope.

## Tests

New file crates/fe-git/tests/status_split_intent_to_add.rs, in a repository with one commit, git run
with `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_NOSYSTEM=1`:

- `intent_to_add_file_is_unstaged_added` (regression, fails at base): after writing `new` and
  `git add -N new`, `status_split()` is exactly `[(new, None, Some(Added))]`.
- `staged_new_file_is_staged_added` (control): `git add -N new` then `git add new` gives
  `[(new, Some(Added), None)]`.
- `intent_to_add_file_deleted_from_disk_is_unstaged_deleted` (control): `git add -N new` then deleting
  the file gives `[(new, None, Some(Deleted))]`.
