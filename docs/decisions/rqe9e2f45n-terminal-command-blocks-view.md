# Terminal command blocks, drawn (T1595)

What T1595 decided while drawing the command blocks T1594 found (OSC 133
A–D): a 2 px edge in celadon or shinsha along each finished block's rows, and
the exit code and duration at the right of its prompt row.

## No padding: the edge runs under the first column

The spec says the edge "sits in the existing left padding, so the text grid
does not move". At the card's base there is no padding: `TerminalElement`
lays the grid from its bounds' left edge, and fe-terminal-view's tests pin
text origins to `cell.width * col` from x = 0.

Round 1 added an 8 px padding to every workbench session. Codex flagged it
(F1, medium): it moved every terminal's text and cost the pty columns, blocks
on or off, agent terminals too. The lead chose to keep base's geometry. So
there is no padding: the grid's origin and its width (and so the pty's
column count) are base's, and `the_text_grid_does_not_move` asserts it (the
first cell at the session's top left, `floor(width / cell)` columns, the same
text with blocks on and off).

The edge is `TerminalView::BLOCK_EDGE` (2 px) down the grid's left edge,
over the first column's cells, painted after the backgrounds and selection
and before the text: a glyph in the first column stays whole, and its first
pixel or two of ink may sit over the edge.

## What is marked

- An edge for a block that ended (`finished`) with an exit code: celadon
  (`status.success`) for 0, shinsha (`status.error`) otherwise. None while it
  runs, while its prompt waits for a command, or when it ended without a code
  (a `D` with none, or the next prompt with no `D`).
- A label from the host (`CommandBlockLook::label`, the panel's
  `block_label`): `exit 1 · 2.3 s` for a failure (`exit 1` when the shell
  marked no `C`, so no duration); `✓ 2.3 s` for a success that ran over a
  second; none otherwise. Text and the duration formatter
  (`workbench-terminal-duration-{seconds,minutes,hours}`, plural-free) are in
  `l10n/en-US/terminal.ftl`; fe-terminal-view holds no UI text.
- The label is shaped in the terminal's font in `text.muted`, right-aligned
  to the grid's right edge on the prompt row, and dropped unless a blank cell
  separates it from the row's last used cell (text, a colour of its own,
  inverse/underline/strikeout, or the cursor).
- A lane's session (an agent's terminal) is not marked; the design colours
  those elsewhere.

## Audit (C5)

`audit/terminal_blocks.rs`, run on the Mac (2026-11-21) with
`cargo run -p fe-workbench --example screenshots -- audit <dir>`:
`terminal_blocks_dark` and `terminal_blocks_light` render and pass all 6
probes each (two `status.success` and two `status.error` points on the edges,
two `surface.background` points). Round 1's probes were measured with the
padding; see the round 2 note below. The scene also needed its `mod` line and `SURFACES` entry in
`audit_scenes.rs`, the runner, which every new surface adds.

The whole audit on the same run wrote 91 captures. Only `inbox_dark` and
`inbox_light` failed, on one probe, (621, 403), `state.selected` over
`surface.raised`: the notification inbox's popup, which holds no terminal and
no code this card changed. `terminal_session`, whose session now has edges
too, still passes (its probes are right of the padding).

### Round 2

With the padding gone the edge sits under column 0, so the scene's probes
moved to rows whose first cell is blank, or to a row's top pixel above any
glyph's ink, and `cargo test`'s output gained its blank line and `running 2
tests`. Rerun on the Mac (2026-11-21, the runner temporarily cut to this
scene): `terminal_blocks_dark` and `terminal_blocks_light` render and pass
all 6 probes each, as does the chrome-theme pass over every bundled theme
(13 more captures). The capture shows the grid at the session's left edge,
as at base, with the edge under the first column.
