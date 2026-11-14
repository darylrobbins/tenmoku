---
task_uid: "01m6w098hrtvce6jff58g3vb5q"
task: "T1571"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::runs_panel (cargo nextest run -p fe-workbench) pass: lists_runs_newest_first_with_icon_title_summary_and_age, a_running_row_draws_its_progress_line (filled to the fraction, else the whole line at 40% alpha), empty_state_names_the_command, clicking_a_row_opens_its_task_tab, rows_have_accessible_names, age_buckets_read_now_12s_3m_2h_4d"
    required: true
    manual: false
  - id: "C2"
    text: "Test space_r_runs_show (cargo nextest run -p fe-keymap --test runs_bindings) passes in the editor's normal context and which-key lists it"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac, the audit scene named in the body renders in both themes and its probes pass (recorded in the decision file) (audit/runs.rs)"
    required: true
    manual: false
---

# Runs panel: a bottom tool window listing runs with status icon, mono title, summary, age and the accent progress line; runs.show on space r

# Runs panel: the list of runs at the bottom

Spec: `docs/design/design-system.md#runs-builds-tests-and-agent-tasks-in-one-list`
and the Workbench anatomy rows (Rail bottom: Problems, Runs, Git log,
Terminal; Bottom panel: Problems, Runs, Terminal, Git log; key `␣r`).

## Change

- `crates/fe-workbench/src/runs_panel.rs` (new): a bottom tool window (the
  panel strip from T1548) listing `Runs` (T1570) newest first. Each row is
  24 px: a status icon (spinner while running, ✓ passed in celadon, ✕ failed
  in shinsha, a muted dash cancelled), the title in code small, a one-line
  summary in UI small muted, and the age on the right (`now`, `12s`, `3m`,
  `2h`, `4d`; refreshed once a second only while the panel is shown and a row
  is under a minute). A running row draws a 2 px accent line along its bottom
  edge: filled to `progress` when known, else the whole line at 40% alpha.
- Clicking a row opens the terminal tab of the task that made it, when there
  is one. An empty list is a centred empty state with a hint naming the
  command that starts a run.
- Registered in `crates/fe-workbench/src/dock.rs` with the rail icon from T1516
  and the command `runs.show` (palette, `␣r` in `crates/fe-keymap/keymaps/fastedit.toml`).
- Arrow keys and Enter work in the list; each row has an accessible name
  ("<title>, <status>, <age>").
- An audit scene `audit/runs.rs`, both themes.

## Not in this card

Per-kind detail panes (tests and build are their own cards), parsing, starting
runs, filters.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-runs-…` in
`crates/fe-workbench/l10n/en-US/runs.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**; the audit scene is recorded on the Mac.
