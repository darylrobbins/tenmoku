---
task_uid: "01m5h9h49cgw7av4hv4xc4afat"
task: "T202"
rev: 3
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests (one per finding, named in the PR) show :s across rows against a joined line, confirm mode not re-searching replacement text, visual marks past a join through EOF landing one row past the last row, and a \\zs match leaving untouched row boundaries intact, each as nvim does"
    required: true
    manual: false
---

# fe-vim: :s across rows matches the evolving joined line, confirm mode keeps its matching source, visual marks past a join through EOF, and \zs never crosses an untouched row boundary (T163 follow-up)

## Problem

T163 (`:s` replaces matches that cross rows) landed with four medium review findings left open (zz49), all in
crates/fe-vim/src/substitute.rs and visual.rs:

- F1 (substitute.rs near 685) batch matching keeps the original row boundaries after joining rows: search against the evolving joined line while collecting edits transactionally, so anchors see the boundaries a join creates.
- F2 (near 983) confirm mode searches the replacement text after every accepted match: keep the current pass's matching source and map matches through accepted edits, updating the source at the next pass.
- F3 (visual.rs near 151, substitute.rs near 537) visual marks on rows joined through EOF are anchored one row after the original: place them immediately past the resulting text's last row.
- F4 (near 744) matches grouped by pass can cross untouched row boundaries when `\zs` moves the start: only apply joining edits to boundaries actually consumed.

## Change

Fix each as nvim 0.12 does (probe with `nvim --clean`), with a test per finding.

## Implementation notes

- nvim's `do_sub` keeps a line's replacements in `new_start` and matches the line as it was until it writes it
  (`ml_replace`): after a replacement that joins rows (then it searches on in the joined line), before a match that
  starts on a later row, and once the row's searches end. fe-vim now models that `Line`: the batch path's finder
  reads a working copy of the text, written into as nvim writes, its window spliced in place (`Window::edit`) so a
  long run of joins reads the text about once; the `c` path keeps that copy (`Rows::read`) beside the buffer, which
  shows each replacement at once as nvim's prompt does. `Walk::next` takes `writing` to stop before a row searched
  afresh, and `Taken::edited` moves a later-row match past a line written before it (F1, F2).
- Each line written is one edit; lines written one after another on the line a join made stay one edit, so no edit
  spans a row boundary that no match took (F4).
- `'<` and `'>` on rows joined through the last are anchored at the text's end, so they land one row past the last
  row the substitute leaves, its `\r` line breaks counted (F3), with `c` or without.
- Carried finding (T163 F1 at substitute.rs:758): `:s` tells a running `:g` the rows it joined
  (`global::Change::Substituted`), and `:s/\n//`'s raw join notes `Change::Joined`, so `:g` drops exactly those
  rows, a join through the last row included.
- Tests: `crates/fe-vim/tests/substitute_multiline_joined_oracle.rs` over `tests/corpus/substitute_multiline_joined.toml`
  (recorded with nvim 0.12.4): `substitute_across_rows_matches_the_joined_line`,
  `substitute_confirm_keeps_its_matching_source`, `substitute_visual_marks_past_a_join_through_eof`,
  `substitute_zs_keeps_untouched_row_boundaries`, `global_substitute_skips_the_rows_it_joins`; and
  `tests::substitute::joining_every_row_reads_the_text_once`.
