---
task_uid: "01m6ensys47qmtgecb5ytxcwq6"
task: "T1307"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::launch::launch_with_no_argument_opens_no_tabs: resolve_launch(None) opens a workbench whose pane has no tabs, no active editor, and whose window title is the app name"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test nothing_saved_opens_no_tabs (in tests/hot_exit_rootless.rs): a rootless relaunch with nothing saved restores no tabs"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test restore_leaving_panes_empty_adds_no_tab (in tests/hot_exit_rootless.rs): a rootless restore whose saved tabs all fail to reopen leaves the panes empty instead of adding a tab"
    required: true
    manual: false
  - id: "C4"
    text: "The WELCOME const and its re-export are gone (rg -n 'WELCOME' crates finds no public const), and fastedit test app_name still passes"
    required: true
    manual: false
  - id: "C5"
    text: "cargo clippy --workspace --all-targets passes"
    required: true
    manual: false
---

# Open with no tabs instead of the welcome text tab when Tenmoku starts with nothing to open

## Problem

Launching Tenmoku with nothing to open (no argument and no saved rootless windows, or after the last tab
of a rootless window closes during restore) puts the built-in `WELCOME` text in an untitled tab
(`crates/fe-workbench/src/workbench.rs:63`, added in `Workbench::new` at :364 and again by
`workbench_chrome.rs:487` after a restore leaves every pane empty). To the user it reads as a random text
file. The user asked on 2026-11-02: "Let's stop showing the random text file when we open without a file".

## Change

- Remove the welcome tab: with no root and no file, the window opens with no tabs, and the pane shows its
  existing empty state ("No open files", `Pane::message`). Remove both places that add it.
- Remove the `WELCOME` const and its re-export from `crates/fe-workbench/src/lib.rs`, and its entry in
  `crates/fastedit/tests/app_name.rs`'s shipped-text list.
- A rootless session saved by an older build whose windows hold only that unmodified welcome tab must not
  bring it back: check how hot exit / layouts record an untitled, unmodified buffer
  (`crates/fe-workbench/tests/hot_exit_rootless.rs`). If they record it, drop such a tab on restore (keep
  the old text as a private constant used only for that check); if they don't, say so in the PR and add
  no code for it.
- Update `crates/fe-workbench/src/tests/launch.rs` and
  `hot_exit_rootless.rs::nothing_saved_shows_the_welcome` to expect no tabs (rename the latter to
  `nothing_saved_opens_no_tabs`).

Don't redesign the empty state here; a richer one (shortcuts watermark) would be its own card.

## Implementation notes

- Hot exit only backs up dirty buffers, so an unmodified welcome tab saved by an older build has no backup to restore; the old layouts' tab fails to reopen and, now, leaves the pane empty. No code was added for it.
- Also updated two tests outside the listed scope that relied on the welcome tab: `pane_toolbar::the_welcome_buffer_is_untitled` (now adds an untitled buffer) and `vim_ls_tests::without_a_root` (no `[No Name]` row).
