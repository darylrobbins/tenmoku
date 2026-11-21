---
task_uid: "01m6w0aaeenq10d6ev2agzskqt"
task: "T1591"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::pre_commit_checks (cargo nextest run -p fe-workbench, fake clock and a fake runner) pass: listed_tasks_run_in_order_after_the_staged_set_changes_and_holds_still_for_2s, a_new_change_cancels_the_previous_round, a_missing_label_shows_as_unavailable, nothing_staged_runs_nothing_and_says_so, a_failing_check_does_not_disable_commit, an_untrusted_root_runs_nothing"
    required: true
    manual: false
  - id: "C2"
    text: "Test the_default_is_empty (cargo nextest run -p fe-config --test pre_commit_setting) passes"
    required: true
    manual: false
  - id: "C3"
    text: "UPDATE_GOLDEN=1 regenerates the settings schema and the generated settings.ftl and settings-template.ftl; cargo nextest run -p fe-config passes with them"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Pre-commit checks above the commit message: listed tasks run as Runs when the staged set changes

# Pre-commit checks in the Changes panel, run as Runs

Spec: `docs/design/design-system.md#changes-and-commit` ("Above the message:
live pre-commit checks from Runs (rustfmt, clippy, tests)").

## Change

- `git.pre_commit_checks`: a list of project task labels (default empty, so
  nothing runs unasked), user and project settings
  (`crates/fe-config/src/settings.rs`, schema, template, generated catalogue
  files). Labels are resolved against `task_defs` (a missing label shows as an
  unavailable check, not an error).
- `crates/fe-workbench/src/pre_commit_checks.rs` (new): when the staged set
  changes (debounced 2 s) and the root is trusted, run each listed task through the
  background runner (T1572) as a Script Run, one at a time in list order,
  cancelling the previous round; a round finishes when the staged set is
  unchanged. Results: pending, running, passed, failed per check.
- The Changes panel (`git_panel.rs`, `git_commit.rs`) shows one row per check
  above the message: status icon, the label, a link that opens the Run in the
  Runs panel. A failing check does not block the commit; the commit button
  stays enabled (the open question below).
- With nothing staged, no checks run and the rows read "No staged changes".

## Not in this card

Blocking commits, git hooks (they already run on commit), choosing tasks from
the panel, checks on unstaged files.

Platform: **Linux-OK**.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-changes-…` in
`crates/fe-workbench/l10n/en-US/changes.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

## Implementation notes

- The checks live in the Changes panel (`GitPanel::pre_commit`), as
  `pre_commit_checks::PreCommitChecks`. The staged set is every file staged
  in the repository with its status, as the panel lists it, and what's staged
  of it: `git diff-index --cached --no-abbrev -z HEAD` (modes and blob ids;
  `git ls-files --stage` with no HEAD yet), read off the main thread each
  time the panel lists the status, which it does once `.git/index` changes.
  So restaging a file's new text from a terminal, a command, hunk staging or
  the panel starts over though it lists the same, and a read that lands
  before a staging does is followed by one after it (review r98wys F1). git
  runs with `core.fsmonitor=false` and `GIT_OPTIONAL_LOCKS=0`, so the read
  runs nothing the repository's config names.
- A task whose own folder `RunProcess` refuses (one marked untrusted inside
  the trusted root) is shown as not trusted and the next check runs; only
  the root's trust changing starts the round over (F2). The test runs the
  real runner.
- The listed labels are read from the user's settings file and the root's
  project file (`SettingsStore::load`, off the main thread) each time the
  set changes, as `refresh_undo_policy` reads them, rather than from the
  window's settings event: that would need `workbench_root.rs` and the app
  crate. A save of either file, or the root's trust changing, starts over.
- A round reads `task_defs::load` for the tasks, then runs each through
  `RunProcess` (`task_options`' argv: `/bin/sh -c` for a shell line) as a
  `RunKind::Script` run, waiting for its end through `RunsEvent`s. Tests
  run through `FakeCheckRunner`, which only begins a run in `Runs` for the
  test to finish.
- The model never reads the git state or the workbench from the panel's
  listing, which can happen while `GitState` updates: it resolves the
  workbench from its own tasks, run events and clicks.
