---
task_uid: "01m6w0akcn1phtqasvvqjnr7w6"
task: "T1596"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in cargo nextest run -p fe-terminal --test history pass: newest_match_by_prefix, duplicates_collapse, lines_starting_with_a_space_are_skipped, the_cap_of_2000_drops_the_oldest, nothing_is_written_to_disk"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in terminal_panel::terminal_suggestion_tests (cargo nextest run -p fe-workbench; the module is crates/fe-workbench/src/tests/terminal_suggestion.rs, declared with #[path] from terminal_panel.rs) pass: the_remainder_shows_faint_after_the_cursor, right_and_end_at_the_end_of_the_line_send_the_remainder, nothing_shows_in_the_alternate_screen_in_vi_mode_or_while_a_program_runs, the_setting_off_hides_it"
    required: true
    manual: false
  - id: "C3"
    text: "UPDATE_GOLDEN=1 regenerates the settings schema and the generated settings.ftl and settings-template.ftl; cargo nextest run -p fe-config passes with them"
    required: true
    manual: false
---

# Terminal history suggestion: a faint remainder at the live prompt, accepted with right or end, in memory only

# Terminal history suggestion at the live prompt

Spec: `docs/design/design-system.md#terminal` ("the live prompt shows a faint
history suggestion").

## Change

- History: the command lines of finished `CommandBlock`s (B to C text) from
  every terminal of the app, **in memory only** (a command line can hold a
  secret; nothing is written to disk), newest first, deduplicated, at most
  2,000; lines starting with a space are skipped (as `HISTCONTROL=ignorespace`).
  `crates/fe-terminal/src/history.rs`.
- The suggestion: while the cursor is at the end of the typed command (between
  OSC 133 B and the cursor) and no program is running, the newest history entry
  that starts with the typed text and is longer shows its remainder in
  `text.faint` after the cursor, drawn by `fe-terminal-view` as an overlay,
  not as terminal cells. `Right` or `End` at the end of the line accepts it
  (sent as input); any other key leaves the shell untouched.
- `terminal.history_suggestions` (default true) turns it off.
- Not shown in the alternate screen, with vi mode on, or when the shell does
  not mark prompts.

## Not in this card

Reading the shell's own history file, fuzzy search, a persisted history.

Platform: **Linux-OK**. No user-visible text.

## Implementation notes

- Needed two small edits outside the listed scope: `fe-terminal/src/{shell,session}.rs` (`finished_commands`, `typed_command`: B-to-cursor text is private shell state), `fe-terminal-view/src/lib.rs` re-exports, `fe-workbench/src/tests/mod.rs` (declares `tests::terminal_suggestion`), the workbench settings label and the generated user docs page.
- History is recorded per view as blocks finish (by block id), only while suggestions are on; lane (agent) sessions neither record nor suggest. An empty prompt suggests nothing.
