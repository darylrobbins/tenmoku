# Read runs keep a long line whole on Windows (T1976)

On Windows, a session's `OutputTap` now hears the grid's logical lines, not
ConPTY's bytes. A read run (`Workbench::run_task_reading`, the debugger's
pre-launch build) therefore gets a cargo `compiler-artifact` line whole,
however long and whatever the terminal's columns. On Unix the tap still
hears the pty's bytes, which are the child's own.

## Cause

ConPTY doesn't pass the child's bytes on. Windows' inbox conhost (Windows
Server 2025, build 26100, on the CI runner) renders the child's output into a
screen buffer of the terminal's size, with no scrollback, and sends VT that
redraws that screen. When a wrapped row scrolls, conhost sends `\r\n` to
scroll, moves back to the last cell of the row above with `CSI <rows-1>;<cols>H`,
prints that cell again, and goes on with the next row's text. Printing in the
last cell again makes the terminal's autowrap mark the row as wrapped.

So the grid gets the line whole, as soft-wrapped rows, while the byte stream
has a hard `\r\n` at every row that scrolled. `Capture` (task_run.rs) splits
the tap's bytes at `\n`, so it kept only the line's first row. The artifact
filter (`launch::is_artifact_message`) then kept that first piece alone,
without `"executable"`.

Evidence, from the probe test `crates/fe-terminal/tests/conpty_long_line.rs`
on the lead's draft PR #10 (run 44621966050, commit 861a4f928, job
135905693750). In an 80-column, 24-row ConPTY session:

- quiet (the 3,200-char line alone): the tap heard it as lines of
  `[1847, 89, 89, …, 89, 29]` chars; "the grid has it whole: true".
- flood (40 lines, the line, 400 lines): `[80, 89, 89, …, 89, 14]`; "the grid
  has it whole: true".
- The bytes from each scrolled row on are
  `…|000001730|\r\n\e[23;80H|000001740|…`. Each 89-char piece is the 8-byte
  `CSI 23;80H`, the last cell of the row above printed again, and 80 chars of
  the row. The first piece of the quiet run is longer because conhost drew
  the screen's first rows in one frame, without scrolling.

The same run's `tests::debug_session::a_read_run_keeps_its_output_whatever_the_scrollback`,
with the 3000-char `src_path` restored, kept a 227-char first piece of the
artifact. T1973's run 44558333306 (commit 152a3d2e1) failed the same way.

The tap's bytes also carry conhost's own escapes inside lines (`CSI ?25l`,
`CSI K`, cursor moves), which no program wrote. Reading the grid drops them
too.

## Fix

`GridTap` (crates/fe-terminal/src/pty_loop.rs), chosen at run time by
`session::GRID_TAP` (`cfg!(windows)`):

- **Rows that scroll away.** After each parser call, and before the shell
  state trims history to the scrollback, the tap hears the rows that call
  scrolled above the screen. A row above the screen is final: conhost has no
  scrollback and can't address it again. Each row's text goes to the tap as
  it's heard. A row marked `WRAPLINE` continues on the next. A row without
  it ends the logical line, and a `\n` follows. `Capture` assembles the line
  and bounds it with its 16 MiB cap, as it does the pty's bytes, so a line
  that never ends costs `GridTap` nothing (review F3). One parser call
  takes at most `MAX_ADVANCE` bytes, so it scrolls fewer rows than the
  `HEADROOM` the `Term` keeps past the scrollback, and no row is trimmed
  before it's heard, even with scrollback 0.
- **Row text** is read cell by cell (`row_text`), not through alacritty's
  selection API: `bounds_to_string` widens a selection's ends around wide
  chars, and at a leading spacer it reads the row above (review F2). A wide
  char is heard once, its spacer and any leading spacer left out.
  Zero-width chars follow their cell's char. A tab is heard once for the
  blanks it moved over, and trailing blanks are trimmed.
- **Exit.** As the loop ends, after a synchronized update is flushed and
  before the exit is published, the tap hears the screen's rows down to the
  last one with text. `Exited` waits for that exit, so `Capture` has every
  line before it takes them. Nothing is heard after that, even if the
  session resizes before its exit is read. The exit takes the terminal's
  lock before the tap's, as a resize does (review F1).
- **Resize.** `Terminal::resize` notes, before each step, where hearing goes
  on: the logical line, counted from the grid's top, and the text cells of it
  already heard. A text cell is any cell but a leading spacer, which a reflow
  adds or drops as wide chars move between rows (review F4). After the
  reflow, and before the shell state trims, it finds that place again by
  counting the same cells. Rows the reflow put above the screen unheard are
  heard then. Rows it brought back from history onto the screen are skipped
  when they scroll away again.

Nothing else changes. `Capture` and its 16 MiB cap are as they were, and Unix
sessions don't use `GridTap`. The lock order is terminal, then shell state,
then `GridTap`.

## Known limits

- **A resize can repeat lines.** conhost may answer a resize by drawing the
  whole screen again. Rows the terminal pushes into history for that redraw
  are heard, and the redrawn copies are heard again as they scroll. A read
  run's tab is sized when it's first laid out, before the build prints, so
  this only happens if the panel is resized while the build runs.
- **The alternate screen is not heard**, and on the primary grid the unheard
  rows of a cleared history (`CSI 3 J`) are lost. A build doesn't do either.
- **What the grid can't keep isn't heard**: trailing blanks, and the blanks
  after a tab, taken to the next stop every 8 columns (alacritty's default:
  `Term`'s tab stops are private). cargo's JSON escapes any tab inside a
  string, so its messages are unaffected.
- **A resize while a line with wide chars is being written can shift the
  rest of that line by a cell, in the grid itself.** alacritty 0.26's reflow
  keeps the text, but it can leave the cursor one text cell off when it adds
  or drops a leading spacer. A probe (420 feeds with a resize between each, at
  37×7, 80×24 and 113×30) found the cursor's offset in its line going from
  701 to 700, or from 479 to 480. The next char then overwrites a cell or
  leaves a blank one. The screen shows the same line, and the tap hears what
  the grid holds. ASCII lines, and lines finished before the resize, aren't
  affected. Like repeated lines, this only happens if the panel is resized
  while the build prints.

## Tests

- `pty_loop::tests::a_grid_tap_hears_whole_a_line_conpty_breaks_at_each_row`
  feeds a 3,200-char line in the byte pattern CI recorded, with conhost-style
  escapes in the lines that follow. It runs on every OS.
- `pty_loop::tests::a_grid_tap_hears_each_line_once_as_resizes_reflow_it`
  narrows and then widens the grid partway through a long line of wide chars
  and accents, with scrollback 0 and 100. It checks that the reflows moved
  wide chars to the next row. Counting leading spacers as text fails it.
- `pty_loop::tests::a_grid_tap_hears_a_wide_char_moved_off_the_first_row_once`
  covers 79 chars and a wide char on an 80-column grid with no history:
  `bounds_to_string`'s leading-spacer case.
- `pty_loop::tests::a_grid_tap_hears_wide_chars_and_accents_whole_as_their_rows_scroll`
  covers wide chars at row ends, with combining accents, scrolled away with
  scrollback 0.
- `pty_loop::tests::a_grid_tap_hands_on_a_line_without_its_end_row_by_row`
  feeds 200 rows with no newline, which reach the tap before the line ends,
  and then a normal line.
- `session::grid_tap_tests::a_grid_tapped_session_resized_as_it_exits_hears_its_lines_and_ends`
  runs a real Unix pty session with the grid tap on (`Terminal::spawn_hearing`).
  It resizes it over and over as its child writes and exits, 20 times, and
  fails if that deadlocks or a line isn't heard exactly once. With the old
  lock order and a 5 ms pause in the exit's hearing, it deadlocks in round 0.
- `crates/fe-terminal/tests/conpty_long_line.rs` (Windows) runs this test
  binary again as the child in an 80-column ConPTY session, so the line is
  written as cargo writes it.
- `tests::debug_session::a_read_run_keeps_its_output_whatever_the_scrollback`
  is back to a 3000-char `src_path` on every OS.

## CI runs

All on the lead's draft PR #10 (`ci/t1976-windows`).

- **Before the fix**:
  [run 44621966050](https://github.com/tenmoku-editor/tenmoku/actions/runs/44621966050)
  at 861a4f928, with the probe and the 3000-char `src_path` but no fix. The
  [windows job 135905693750](https://github.com/tenmoku-editor/tenmoku/actions/runs/44621966050/job/135905693750)
  fails `fe-terminal::conpty_long_line an_80_column_session_taps_a_3200_char_line_whole`
  and `fe-workbench tests::debug_session::a_read_run_keeps_its_output_whatever_the_scrollback`.
  T1973's
  [run 44558333306](https://github.com/tenmoku-editor/tenmoku/actions/runs/44558333306)
  at 152a3d2e1 failed the read-run test the same way.
- **After the fix**:
  [run 44644534814](https://github.com/tenmoku-editor/tenmoku/actions/runs/44644534814)
  at 23ff84602.
  - [windows job 135983646619](https://github.com/tenmoku-editor/tenmoku/actions/runs/44644534814/job/135983646619)
    passes `conpty_long_line` (`child` and
    `an_80_column_session_taps_a_3200_char_line_whole`), the read-run test
    (21.9 s), and `changing_log_payloads_applies_to_the_next_session`.
  - [linux job 135983646474](https://github.com/tenmoku-editor/tenmoku/actions/runs/44644534814/job/135983646474)
    and the macOS
    [check job 135983646893](https://github.com/tenmoku-editor/tenmoku/actions/runs/44644534814/job/135983646893)
    pass the read-run test and both `pty_loop::tests::a_grid_tap_*` tests.
    macOS passes all 11,278 tests.
- **Failures that are on main too**: Windows fails three
  `tests::run_process` cancel tests (`a_cancel_kills_on_time_while_the_sink_is_blocked`,
  `a_cancel_kills_on_time_while_the_main_thread_is_behind`,
  `cancel_kills_the_whole_process_group`). Linux fails
  `fe-project::fs_ops_undo another_entry_in_its_place_in_the_trash_is_not_taken`.
  Main's
  [run 44617679647](https://github.com/tenmoku-editor/tenmoku/actions/runs/44617679647)
  at this lane's base, 301b20dc6, fails the same four.
