---
task_uid: "01m725bxfe71vf3g0hz4k825j9"
task: "T341"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C2"
    text: "en-US byte-equal; cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
---

# fe-workbench Git strings outside the Changes panel and the log (i18n W2, rest) (Mac T1557)

# fe-workbench Git strings outside the Changes panel and the log (W2, rest)

Spec: card **W2** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9, for the fe-workbench files the B3 surface
cards don't restyle (they move their own files' text). Sections 3–8 are the
design. en-US stays byte-equal to today's, so no golden and no behaviour test
changes except tests that construct removed literals.

Files: `git.rs`, `git_blame.rs`, `git_blame_column.rs`, `git_conflicts.rs`, `git_diff.rs`, `git_files_picker.rs`, `git_remote.rs`, `git_stage.rs`, `git_stash.rs`.

Add `crates/fe-workbench/l10n/en-US/git.ftl` (keys `workbench-git-…`) to
`resources!` in `crates/fe-workbench/src/l10n.rs`, and these files to
`l10n-allow.toml`'s `migrated` list. English plural helpers become plural
messages; fragments become whole messages.


Platform: **Linux-OK**.

## Implementation notes

- `git_diff::E21` stays a `verbatim!` const: `vim_host.rs` and `vim_filter.rs` (outside this card) take it as a `&str`. Whichever card moves Vim's error text can key it.
- The commit-date numerics in `git_blame::date` and two git plumbing strings in `git_remote.rs` are in `l10n-allow.toml`; `git_files_picker.rs`'s test selector too.
- The Fetch/Pull/Push verb is a `$verb` selector inside each message, since Fluent term arguments must be literals.
