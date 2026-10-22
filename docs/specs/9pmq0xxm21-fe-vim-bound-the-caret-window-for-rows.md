---
task_uid: "01m5acbv5wae7zhn9pmq0xxm21"
task: "T197"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Cursor::at on a huge row of uninterrupted ASCII letters, Cyrillic, emoji, tabs or blanks reads a bounded window, with tests for each run kind"
    required: true
    manual: false
  - id: "C2"
    text: "Directive detection (% on #if/#else/#endif) reads the prefix incrementally until the first nonblank token, with a long-indentation regression"
    required: true
    manual: false
---

# fe-vim: bound the caret window for rows with uninterrupted letters, Cyrillic, emoji, tabs or blank runs, and read directive prefixes incrementally instead of a 16 KiB cutoff (follow-up from T102 F3, F12)

Split from T102 round 3 (codex F3 and F12), which T102 narrowed to a documented known limit. See rgt review show T102 for the full finding text and fixes.

## Implementation notes

- `Line::around` ends a window with no class change in reach at the nearest place between two chars that join nothing (`inert_edge`); `iw`/`aw` widen the row when a window's edge cuts their runs short (`Cursor::runs_reach_edge`).
- `%` reads a directive's head chunk by chunk (`row_directive`), for the caret's row and every row `conditional` visits.
- The huge-virtual-count oracle tests compare `gR` with plain `R`; `R` on its huge row is now ~200x faster (82 s to 0.35 s in debug), so the ratio bound no longer fits. Their slack went from 5 s to 60 s.
