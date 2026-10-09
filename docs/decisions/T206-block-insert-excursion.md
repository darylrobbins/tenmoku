# Esc types a block insert's text on a row the session typed on

Status: **built** in `fe-vim` (T206, T174 F26). Decided 2026-10-09.

## Decision

A block session (`c`, `I`, `A`) follows each of its other rows through the
buffer's edits by origin (`insert/block_rows.rs`). The session's own typing
within a row, which a cursor excursion allows, keeps it eligible; another
origin's edit on it, its line break taken out, or a line break typed in it
costs it the text, as before.

On such a row `Esc` doesn't reuse the place worked out when the session
opened: nvim 0.12.4 types at the block's display column in the row *as it is
now*, padding a row that was short (`A`), so fe-vim recomputes the place on
the current text (`visual::insertion`). A row nobody edited keeps the place
from the opening, so every earlier result is unchanged.

## nvim 0.12.4 results

Three rows `abcd` (the fourth case has `abcd`, empty, `abcd`), cursor at
(0,1), driven headless with `nvim_feedkeys`; `<Down>` and `<Up>` are the
cursor keys.

| keys | result | cursor |
| --- | --- | --- |
| `<C-v>jjlcXY<Down>Z<Up>Q<Esc>` | `aXYQd` / `aXYQdZ` / `aXYQd` | (0,3) |
| `<C-v>jjIXY<Down>Z<Up><Esc>` | `aXYbcd` / `aXYbcZd` / `aXYbcd` | (0,1) |
| `<C-v>jjAXY<Down>Z<Up><Esc>` | `abXYcd` / `abXYcdZ` / `abXYcd` | (0,1) |
| `<C-v>jjAXY<Down>Z<Up><Esc>` over an empty middle row | `abXYcd` / `Z XY` / `abXYcd` | (0,1) |

The last shows the recomputation: the session typed `Z` on the empty row, and
`XY` went in at display column 2 after a space of padding.
