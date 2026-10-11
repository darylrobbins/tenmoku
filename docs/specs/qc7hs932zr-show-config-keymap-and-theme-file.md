---
task_uid: "01m4pdmpj7gxgn88qc7hs932zr"
task: "T56"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test config_problems::tests::status_item_follows_the_diagnostics: setting kiln's ConfigDiagnostics to two errors and a warning shows a left status item config.problems reading '3 config problems' in every open workbench, one problem reads '1 config problem', and an empty set removes the item"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test config_problems::tests::list_opens_at_the_problem: workbench.show_config_problems, and a click on the item, opens a picker listing errors before warnings with file name:line:column; typing filters it; confirming a row opens that file in the active pane with the caret at the diagnostic's 1-based line and char column, including a line with multi-byte chars before the column"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test config_problems::tests::a_broken_settings_file_shows_and_clears: through kiln-platform's config service (test-support batch channel), an invalid value written to the user's settings.toml shows the item while the last good settings stay applied, and fixing the file removes it"
    required: true
    manual: false
  - id: "C4"
    text: "workbench.show_config_problems is in command_catalog() and the palette's command_list(); crates/fastedit/src/main.rs, status_bar.rs and Workbench::reload_settings are unchanged"
    required: true
    manual: false
---

# Show config, keymap and theme file problems in the status bar, with a list that opens each at its line (Mac T394)

## Problem

M5's exit criterion is "invalid config keeps the last good values **and shows a diagnostic**"
(plan, M5). The first half holds (fe-config and kiln-core tests); the second doesn't. A typo in
`settings.toml`, `keymap.toml` or the theme file is only logged:

- kiln's config service logs each file's diagnostics and publishes them all as the
  `kiln_platform::config::ConfigDiagnostics` global (`kiln/crates/kiln-platform/src/config.rs:126-139`,
  `apply` at :311: "observe it to show them as they change"). Nothing in fastedit observes it.
- `crates/fastedit/src/main.rs:159` copies the settings diagnostics into each `SettingsEvent`, and
  `Workbench::reload_settings` (`crates/fe-workbench/src/workbench.rs:1416`) never reads them.
- `main.rs:112` logs keymap issues (`log_keymap_report`).

So a user who breaks their settings sees nothing change and no reason why.

## Change

Add `crates/fe-workbench/src/config_problems.rs` (tests in `config_problems_tests.rs`, declared
from it with `#[path]`):

- **Status bar item.** Each `Workbench` observes `ConfigDiagnostics` (in `new`, as it observes its
  other globals) and keeps a status item with id `config.problems` on the left: `1 config problem` /
  `N config problems` while there are any, none when there are none. Its command is
  `workbench.show_config_problems`. Set it through `StatusBar::set_item` / `remove_item`; don't
  change `status_bar.rs`.
- **The list.** `workbench.show_config_problems` ("Show Config Problems", workbench context,
  registered from `config_problems.rs` and added to `command_catalog` and `command_list` in
  `lib.rs`) opens a kiln-shell `Picker` over a `ConfigProblemsPicker` delegate, hosted by the
  workbench like `language_picker`. Each row is the diagnostic's message, with
  `file name:line:column` and its severity as the second line; errors first, then warnings, each
  in file order. Typing filters by message and file name. Confirming a row opens the file in the
  active pane (`Pane::open_at`) with the caret at the diagnostic's line and column (kiln's
  `Diagnostic::line`/`column` are 1-based, the column in chars). With no problems the command does
  nothing.

`ConfigDiagnostics` covers the user's settings, keymap and theme files and the launch root's
project settings. A window on another root reads its project file in `main.rs::settle` and its
diagnostics stay logged only: out of scope. Keymap *issues* (`KeymapIssue`: overrides, shadows,
prefix delays) aren't errors and stay logged. The theme file's diagnostics join
`ConfigDiagnostics` once T246 routes it through kiln's config service; nothing here depends on that.

Don't change `main.rs` or `reload_settings`.

## Implementation notes

- A pick reads the file itself, off the main thread, as kiln's config service reads an untrusted
  config file: opened with `O_NONBLOCK`, checked as a regular file on the open descriptor, and read
  through `take(1 MiB + 1)`, so a FIFO, `/dev/zero` or a `/proc` file whose size says 0 is refused
  with a warning naming why. The pane is then handed that buffer as a host hands it a read
  (`Pane::opening` + `Pane::file_read`), so it never reopens the path; a file already open just
  shows its tab, and a buffer open in another pane is shared.
- The caret then goes to the diagnostic's line and char column against the editor's own text
  (`config_problems::place_caret`), so unsaved edits and a buffer shared from another pane count.
  `Pane::open_at`'s `Reveal` isn't used: its columns are bytes.
- The read's completion goes ahead only if no later pick was made, the pane it was picked in is
  still the workbench's (the pick holds it weakly), and the pane's open serial hasn't moved: a
  file opened meanwhile stays, while an older read landing meanwhile doesn't cancel the pick.
- A row's identity carries the list's version, bumped by each change to the problems, so a click
  pressed on a row isn't taken by the one that replaced it.
- The picker's rows are kiln-shell's fixed `ROW_HEIGHT` (24px), so the two lines are drawn at 11px
  and 10px with tight line heights to fit.
- The item sits on the left at priority 50, after vim's mode (0) and message (10).
- While the list shows, a change to the diagnostics refreshes it under the same query; when they
  clear, the list is dismissed with the item.
- The workbench syncs the item once at the end of `new`, so a window opened while there are
  problems shows them at once.
