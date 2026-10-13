---
task_uid: "01m4t9ceg5qrsb23nrecbycmfv"
task: "T104"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-git test commit_comment_prefix::comment_char_set_after_comment_string_wins: with repository core.commentString=# then core.commentChar=; (git config --add) and a staged change, Repo::commit(\"#42 fix crash\\n\", false) returns the new HEAD id and git log -1 --format=%B is \"#42 fix crash\""
    required: true
    manual: false
  - id: "C2"
    text: "fe-git test commit_comment_prefix::repo_comment_char_overrides_global_comment_string: with global core.commentString=# (GIT_CONFIG_GLOBAL file) and repository core.commentChar=;, Repo::commit(\"#42 fix crash\\n\", false) commits that message"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git test commit_comment_prefix::uppercase_auto_comment_char_keeps_auto_line: with core.commentChar=AUTO, Repo::commit(\"AUTO fix crash\", false) commits that message"
    required: true
    manual: false
  - id: "C4"
    text: "fe-git test commit_comment_prefix::comment_string_set_after_comment_char_wins: with repository core.commentChar=; then core.commentString=#, Repo::commit(\"#42 only\\n\", false) returns Err(CommitError::EmptyMessage) with HEAD unchanged, and Repo::commit(\"; note\\n\", false) commits \"; note\""
    required: true
    manual: false
  - id: "C5"
    text: "The existing tests in crates/fe-git/tests/commit.rs and commit_comment_char.rs pass unchanged; cargo test -p fe-git and cargo clippy -p fe-git --all-targets -- -D warnings pass; no Cargo.toml, Cargo.lock or crates/fe-git/src/lib.rs change"
    required: true
    manual: false
---

# Take the last of core.commentChar and core.commentString, matching auto in any case, as fe-git's commit comment prefix (T101 review F1, F2) (Mac T635 follow-up)

## Problem

T101 (22bfee9) taught `Repo::commit` to read the comment prefix, but its review (codex, review 3m126q) left
two medium findings in `Repo::comment_prefix` (`crates/fe-git/src/commit.rs:53-84`):

- **F1, line 57.** It asks for `core.commentString` first and falls back to `core.commentChar`. The two keys
  are aliases and git uses the **last** value set for either, across system, global, repository and
  command-line config in that order. So a global `core.commentString=#` with a repository
  `core.commentChar=;` makes fastedit refuse `#42 fix crash` as `EmptyMessage`, though git commits it.
- **F2, line 65.** `auto` is matched case-sensitively. Git treats `AUTO`, `Auto` and so on as auto; with
  `core.commentChar=AUTO`, `Repo::commit("AUTO fix crash", false)` returns `Err(EmptyMessage)` because the
  prefix becomes the literal `AUTO`, though git commits the message (and fe-git before T101 did too).

Checked 2026-10-13 with git 2.53.0 in a scratch repo, system config off, `GIT_CONFIG_GLOBAL` a temp file,
`git commit --cleanup=strip -F -`:

- global `core.commentString=#`, local `core.commentChar=;`: `#42 fix crash` commits.
- local `core.commentString=#` then `core.commentChar=;` (same file): `#42 fix crash` commits; `; only` is
  refused as empty. Reverse order (`commentChar=;` then `commentString=#`): `#42 fix crash` is refused;
  `; note` commits.
- `core.commentChar=AUTO`: `AUTO fix crash` and `#42 fix crash` commit; `core.commentChar=Auto` and
  `core.commentString=AUTO` behave the same.
- `core.commentChar=auto` then `core.commentChar=;`: `; only` is refused (the later value wins over auto);
  `;` then `auto`: `; x` commits.
- `git config -z --get-regexp '^core\.comment(char|string)$'` lists every value of both keys in that
  precedence order, last wins, as `<lowercased key>\n<value>\0` records (command-line `-c` values last);
  exit 1 when neither is set.

## Change

In `crates/fe-git/src/commit.rs`, rewrite `Repo::comment_prefix` to make one `run_raw(..., Access::Read, ...)`
call: `git config -z --get-regexp ^core\.comment(char|string)$`.

- Exit 1: `Ok(Some("#"))`, as now.
- Exit 0: split stdout on `\0`, take the last non-empty record, and its value is the text after the first
  `\n` (empty when there is none). If the value `eq_ignore_ascii_case("auto")`, return `Ok(None)`; if
  empty, `Ok(Some("#"))` as now; otherwise `Ok(Some(value))`.
- Any other exit: the same `GitError::Command` as now.

Update `comment_prefix`'s doc comment to say the last of `core.commentChar`/`core.commentString` wins and
`auto` is matched in any case. `Repo::commit`'s empty check is unchanged. No public API change.

## Tests

New integration test file `crates/fe-git/tests/commit_comment_prefix.rs` (do not edit
`commit_comment_char.rs`), with test functions at the top level (no `mod`), built only on APIs that exist
at the base (`Repo::discover`, `Repo::commit`, `CommitError`). Use the same temp-repo setup and
child-process isolation as `commit_comment_char.rs`, except that the child's `GIT_CONFIG_GLOBAL` points at
a file the parent writes in a temp dir it keeps alive until the child exits (empty unless the test needs
global config). Set repository config with `git config` / `git config --add` in the temp repo.

Regression tests (each fails at the base, which returns `Err(EmptyMessage)`):
`comment_char_set_after_comment_string_wins`, `repo_comment_char_overrides_global_comment_string`,
`uppercase_auto_comment_char_keeps_auto_line`. The fourth test,
`comment_string_set_after_comment_char_wins`, covers the other alias order and passes at the base too.
