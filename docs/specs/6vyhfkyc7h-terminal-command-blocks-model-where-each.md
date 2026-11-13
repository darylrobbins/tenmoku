---
task_uid: "01m7ac1vdrx2gbb86vyhfkyc7h"
task: "T610"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in cargo nextest run -p fe-terminal --test command_blocks pass: blocks_record_rows_and_exit_codes, a_running_block_has_no_end, blocks_survive_history_trimming_and_the_alternate_screen, a_missing_D_leaves_no_exit_code, the_cap_of_1000_drops_the_oldest, command_and_output_text_are_readable"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-terminal passes (shell_integration and the pty_loop tests unchanged)"
    required: true
    manual: false
---

# Terminal command blocks, model: where each command starts and ends, its exit code and duration (OSC 133 B/C/D) (Mac T1594)

# Terminal command blocks, model: where each command starts, ends and how it ended

Spec: `docs/design/design-system.md#terminal` ("Each command and its output is
a block with a left edge in green or red, the exit code and duration on its
prompt line"). `crates/fe-terminal/src/shell.rs` already scans OSC 133 A, B, C
and D; `ShellEvent::CommandEnd(Option<i32>)` is kept with `#[allow(dead_code)]`
"for the card that marks output", and only `PromptStart` is used
(`scroll_to_prompt`, `docs/terminal-shell-integration.md`).

## Change (`crates/fe-terminal`, no UI)

- `ShellState` records a `CommandBlock { prompt_row, input_row, output_start_row,
  end_row: Option<..>, exit_code: Option<i32>, started: Instant, finished:
  Option<Instant> }` per prompt, using the same absolute row numbers the
  prompt marks use (so trimming and the alternate screen keep them right);
  a block still running has no end. Capped at 1,000 blocks. `CommandEnd` loses
  its `allow(dead_code)`.
- `Terminal::command_blocks()` / `Snapshot` expose the blocks overlapping the
  visible rows, with rows relative to the viewport, plus the command line text
  (B to C) and the output text on request.
- A shell that does not emit D leaves blocks with no exit code (drawn neutral by
  the view card).
- `docs/terminal-shell-integration.md` documents the block model.

## Not in this card

Painting, history suggestions, Send to an agent, installing shell integration
scripts (they exist; unchanged).

Platform: **Linux-OK**.

## Implementation notes

- The blocks reach the view through `Terminal::command_blocks()` and
  `Terminal::snapshot_with_blocks()` (the snapshot and the blocks under one
  lock), not a new `Snapshot` field: tests in fe-terminal and
  fe-terminal-view build `Snapshot` literals, which a field would break.
  `crates/fe-terminal/src/lib.rs` joined the scope to export `CommandBlock`.
- Text on request: `Terminal::command_text(id)` (B to C) and
  `Terminal::command_output(id)` (C to the end, or the cursor while running).
- `ShellEvent::CommandEnd` had no `#[allow(dead_code)]` left on this trunk
  (the tab's status already read it); nothing to remove.
- `input_row` and `output_start_row` are `Option`s: a prompt waiting for a
  command has neither C, and a theme that drops B has no input row.
- `started` is the C's time (the A's until then); `duration()` is `None`
  without a C. A D for a block with no C (bash after an empty line) ends it
  with no exit code.
