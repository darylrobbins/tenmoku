---
task_uid: "01m73ypxbqeyz5j10yc8j742mv"
task: "T345"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::changes_b3 (cargo nextest run -p fe-workbench) pass: a file's checkbox stages and unstages it, a group's checks all; letters and colours come from vcs.* (ADDED_HUE gone); the message box's placeholder is in the UI font; the counter turns error past 72; the button reads 'Commit 1 file' / 'Commit 3 files' with its key; 'and push' commits then runs git.push"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Changes and commit in B3: checkboxes for staging, VCS colours with letters, the commit box with its counter, Commit N files and push (Mac T1553)

# Changes and commit in B3: checkboxes, the commit box, Commit N files and push

Spec: `docs/design/design-system.md#changes-and-commit`. Audit findings PT1,
GP1–GP3 (the held cards P10's panel part and P19).

## Change (`crates/fe-workbench/src/git_panel.rs`, `git_commit.rs`, `source_control.rs`)

- Your changes have checkboxes: checked = staged (the checkbox toggles
  stage/unstage through today's staging calls); the group header's checkbox
  checks all. Discard and Open stay as row actions on hover.
- Status letters and colours from `vcs.*` (modified ruri, added celadon,
  deleted shinsha; `ADDED_HUE` goes), each with its letter so hue is never the
  only cue.
- The message box: placeholder "Message" in the UI font (GP1), a 72-character
  counter (exists as `n/72`: one counter, faint, shinsha past 72); below it
  Amend (checkbox), the primary button "Commit 1 file ⌘↵" (a plural message
  with the staged count and the key from the keymap), and "and push" as its
  split part (runs `git.push` after a successful commit).
- The panel title is "Changes".
- An audit scene `audit/changes.rs` (the audit's Git project) with probe
  points.
- The agent group, pre-commit checks and Draft from diff are separate work.

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `git_panel.rs`, `git_commit.rs` and `source_control.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/changes.ftl` (keys `workbench-changes-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Linux-OK**.

## Implementation notes

- **Checkboxes.** Each row and header has a `Checkbox`, checked when its
  files are staged: a Staged row's (or Staged Changes' header) unstages,
  a Changes row's (or header) stages, a Merge row's (or header) stages
  through `stage_merge_files`, which still asks about markers. The Stage
  and Unstage icon buttons are gone; Open File and Discard (Discard All on
  the Changes header) stay as hover buttons. The fugitive keys (`s`, `u`,
  `-`, `X`) are unchanged. `button_selector(.., Action::Stage | Unstage)`
  now returns the row's checkbox, so the existing staging tests click it.
- **Colours.** `letter_color(kind, cx)` reads `cx.theme().colors.vcs`
  (modified, added, deleted, untracked, conflict); `ADDED_HUE` is gone.
  The letter moved to the row's right end. The tree's badges and the tabs'
  tints call the same function, so they follow too.
- **Message box.** The placeholder "Message" and the counter are drawn over
  the editor in the UI font (`FontFamilies`' `ui`), not as after-line
  decorations in the editor's font; the box has no decoration provider now.
  The placeholder after an empty message was refused is "Enter a commit
  message" in the warning colour, as before. One counter, `n/72`, counts the
  caret's row (VS Code's input validation counts the current line), faint,
  `status.error` past 72; the old `n/50` subject hint is gone. Reading HEAD
  for the old placeholder ("…to commit on "main"") is gone with it. The wrap
  reserve stays, so text keeps clear of the counter.
- **Button.** `workbench-changes-commit` is a plural on the staged count
  (`[0]` is plain "Commit", which asks to stage everything, as before); the
  key is the keymap's first single key for `git.commit_box.commit` in
  `workbench › git_panel › editor`, shown after the label while the button
  is enabled and given to `Button::shortcut`. "and push" sits beside it as
  its split part; `CommitRequest.push` carries it through the stage-all
  question, and `committed` runs `git.push` (`run_git_command`) once the
  commit is made and the root hasn't changed. Where the sidebar is too
  narrow for Amend and the buttons on one row, the buttons wrap under Amend;
  narrower still, the commit button's label ends in an ellipsis (its
  tooltip keeps the key). Checked in the Linux app under Xvfb: staging from
  a row's checkbox, the counter at 72 and 73, the wrapped buttons.
- **i18n.** `l10n/en-US/changes.ftl`, added to `src/l10n.rs`; the three
  files are in `l10n-allow.toml`'s `migrated` list. `SIDEBAR.name` stays the
  English `&'static str` the dock's `SidebarPanel` has for every panel
  (allow-listed with that reason); where `source_control.rs` shows the
  title it uses `workbench-changes-title`.
- **Audit.** `audit/changes.rs` replaces `audit/git_panel.rs` (same scene,
  the audit's Git project). The audit runs only on macOS, so its two probe
  points (the empty box right of the placeholder, the panel below the
  graph) are adapted from git_panel's and not yet checked by a capture.
- **Files outside the listed scope**, forced by signatures and existing
  tests: `src/l10n.rs`, `l10n-allow.toml`, `src/tests/mod.rs` (the module
  line), `examples/screenshots/audit_scenes.rs` (the scene's registration),
  `src/project_panel.rs` and `src/workbench.rs` (`letter_color` takes
  `cx`), `src/git_commit_tests.rs` (placeholder, counter, labels),
  `src/git_panel_merge_tests.rs`, `src/project_panel_git_status_tests.rs`
  and `src/tab_git_status_tests.rs` (group titles are `String`s,
  `letter_color`'s signature).
- **Review round 1.** F1: the button's count is the repository's staged
  files (`listed`, the whole status), not the rows under the root, as a
  commit takes the whole index; conflicted files aren't counted. F2: a
  row's name shrinks to an ellipsis, so its letter stays in the row. F3:
  `Dock::follow_locale` retitles Source Control and its Changes view
  (`source_control::follow_locale`), which needed `ViewContainer::set_title`
  and, for the test, `ViewStack::title` in kiln-shell
  (`kiln/crates/kiln-shell/src/view_container.rs`, `view_stack.rs`). Tests:
  `the_button_counts_what_a_commit_takes_from_the_whole_repository`,
  `a_long_names_letter_stays_in_its_row`, `the_titles_follow_the_language`
  (a fake `qps-ploc` locale registered in the test binary).
