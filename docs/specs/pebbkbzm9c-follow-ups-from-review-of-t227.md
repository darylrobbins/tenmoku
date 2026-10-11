---
task_uid: "01m4p1zv0hpw8c8kpebbkbzm9c"
task: "T393"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T227

Findings of the review of T227 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/increment.rs:288: Zero padding includes the sign in the preserved width. Consequently, <C-a> on "-001" produces "0000" instead of Neovim's "000", and <C-x> on "000" produces "-01" instead of "-001". The same defect affects visual increments and decrements crossing zero.
  fix: Preserve the original digit width independently of the old and new signs, and add normal and visual corpus cases crossing zero with zero-padded decimals.
  instances: crates/fe-vim/src/increment.rs:288

- F2 medium crates/fe-vim/src/increment.rs:337: Block bounds use only endpoint start columns, losing both endpoint character widths and the $ goal. On "a 1\nb 222\nc 3", <C-v>jj$<C-a> from (0,2) produces "b 322" on the middle row instead of "b 223". On "\t1\n   3\n  2", <C-v>kk<C-a> from (2,2) skips the middle number although Neovim includes it within the tab endpoint's width.
  fix: Calculate block bounds using the full display width of endpoint graphemes and preserve selections extending to each row's end; record both examples and their progressive variants.
  instances: crates/fe-vim/src/increment.rs:337, crates/fe-vim/src/increment.rs:373

- F3 medium crates/fe-vim/src/increment.rs:424: The minimum endpoint offset is not always the visual selection's starting caret. Vj<C-a> on "x 1\nx 2" from (0,2) leaves the caret at column 2 instead of Neovim's column 0. <C-v>jh<C-a> on "a 1\nb 2" from (0,2) leaves it at column 2 instead of the block's left column 1. This incorrect start is also recorded for undo.
  fix: Normalize the start by selection kind: row start for linewise, top-left display position for blockwise, and minimum endpoint for charwise; use that position for landing and undo history.
  instances: crates/fe-vim/src/increment.rs:424

- F4 medium crates/fe-vim/src/increment.rs:506: Repeating a visual increment has an extent but no live visual mode, so it follows normal-mode landing and moves to the number's last digit. Vj<C-a>jj. on four rows containing "19" leaves the caret at (2,1), while Neovim leaves it at (2,0). This changes subsequent motion and repeat targets.
  fix: Use visual landing semantics whenever an extent is active, including replay, and add repeat cases with multidigit numbers or numbers after the starting caret.
  instances: crates/fe-vim/src/increment.rs:506

- F5 medium crates/fe-vim/src/increment.rs:354: Charwise replay loses selection geometry: it stores grapheme counts, clamps endpoints at the last character, and cannot distinguish $ from a finite selection. On "1\n12345", v$<C-a>j. produces "2\n22345" instead of Neovim's "2\n12346". On "界1\n  2", vl<C-a>j. fails to increment the second row because Neovim repeats the selection's display width.
  fix: Represent and replay charwise extents using Neovim's display-column and end-of-line semantics, including endpoints beyond the last character; add wide-character, finite-EOL, and $ replay cases.
  instances: crates/fe-vim/src/increment.rs:354, crates/fe-vim/src/increment.rs:363, crates/fe-vim/src/increment.rs:387

- F6 medium crates/fe-vim/src/increment.rs:436: Normal-mode searches are deduplicated by row, and caret landing also identifies edits solely by row. With carets on both numbers in "1 2", only the first number changes and both carets land on it, collapsing the selections despite the documented per-caret behavior.
  fix: Resolve each caret's number, deduplicate only overlapping number rewrites, and retain a caret-to-rewrite mapping; test distinct numbers and shared numbers on one row.
  instances: crates/fe-vim/src/increment.rs:436, crates/fe-vim/src/increment.rs:513

- F7 medium crates/fe-vim/src/increment.rs:320: The public command accepts unchecked replay dimensions. A replay with rows=0 underflows, and large rows or cols overflow additions in window construction. These schema-valid arguments can panic builds with overflow checks and produce invalid selection geometry in other builds.
  fix: Validate replay dimensions before constructing windows, reject invalid extents with CommandError, and use checked or saturating arithmetic for row and column calculations.
  instances: crates/fe-vim/src/increment.rs:320, crates/fe-vim/src/increment.rs:334, crates/fe-vim/src/increment.rs:356

- F8 medium crates/fe-vim/src/increment.rs:372: Block window construction calls start_column for every grapheme, and start_column rescans all preceding graphemes. This makes each selected row quadratic even for a narrow block; a 100,000-character row entails roughly five billion width evaluations and can freeze the editor.
  fix: Walk each row once while accumulating display columns, stopping when the block's right boundary is passed.
  instances: crates/fe-vim/src/increment.rs:372
