---
task_uid: "01m55mhzspvdc25trtvqk28daj"
task: "T163"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test substitute_multiline_matches_neovim (top level, in crates/fe-vim/tests/substitute_multiline_oracle.rs) passes on crates/fe-vim/tests/corpus/substitute_multiline.toml, at least 12 cases recorded with nvim 0.12.x, among them (asserted by name) join_comma_rows, join_all_rows, newline_to_r_splits, g_flag_across, n_flag_counts, range_end_past, marks_on_joined_rows, undo_restores, single_row_unchanged."
    required: true
    manual: false
---

# fe-vim: :s replaces matches that cross rows, joining them, as nvim's :%s/,\n/, / does (Mac T899)

## Problem

After the multi-row search cards, `:s` still matches one row at a time (`crates/fe-vim/src/substitute.rs`
batches one edit per row), so `:%s/,\n/, /` (join rows ending with a comma) fails, though it is one of the
most common nvim idioms. The user approved multi-row patterns in the "Vim parity" track on 2026-10-17.

## Change

In `crates/fe-vim/src/substitute.rs` and `crates/fe-vim/src/search/replace.rs`: for a pattern that
`spans_rows()`, `:s` over a range finds matches through `find_across` starting in each row of the range (a
match may run past the range's last row, as nvim's does: probe), replaces them (a replacement `\r` still
breaks a row), and rows the replacement joins drop out of the range as nvim's do (`:%s/\n//` joins every row
in one command: probe the count it reports and the cursor). Flags `g`, `c` (with T838's prompt), `n`, `e`,
`i`/`I` and the count argument work as on one row; one undo step; marks on joined rows follow as a `J`
moves them. Keep the per-row path for single-row patterns.

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/substitute_multiline.toml`, recorded
into `substitute_multiline.expected.json` by the ignored `record_corpus_with_neovim` in `tests/oracle.rs`,
and a new `crates/fe-vim/tests/substitute_multiline_oracle.rs` modelled on `tests/line_motions_oracle.rs`.

substitute.rs is changed by T837, T838 and the ex search-case card: this card depends on that card.

## Implementation notes

- The walk (`search::replace::Walk`, read through a `Finder` window as `find_across` reads rows) follows
  nvim 0.12.4's `do_sub` (`src/nvim/ex_cmds.c`): each row of the range from its start; after a replaced
  match over rows the search goes on in its last row even without `g` ("do_again"), unless that row is past
  the range's last, which ends the command; with `n`, or answered `n`, a match over rows ends its row's
  searches; an empty match where the last ended is skipped a character on; a match `\zs` starts on a later
  row is taken, and the next row searched afresh is the one after the row searched. Positions are in the
  text before any replacement (the `c` prompt maps them through each of its own edits, `Walk::edited`).
- `:s/\n//` with no flag, or one of `g l p #`, is nvim's `sub_joining_lines`: the range and the line after
  it joined as `:join!`, the caret where `gJ` leaves it, marks moving as `J` moves them, the last flags left
  as they were. Every other replacement that joins rows deletes the marks on the rows after the first
  (nvim's `mark_adjust(..., MAXLNUM, ...)`), and puts `'<` and `'>` on the row after them; it is two
  edits for that (`joining`), one if the match ends on the last row.
- nvim reports counts only for typed commands with more than one line or more than 'report' changes; the
  recorder doesn't read messages, so `substitute_multiline_reports_nvims_counts` checks the counts probed
  by hand.
- T160 F1/F2: a pattern taken from the last search or the last substitute keeps whether it was `*`'s or
  `#`'s (`LastSubstitute::word`, and `ex::search_word` for `:g` and `/pat/` addresses), and the last
  search takes that provenance on; the corpus's `star_*` and `typed_substitute_repeated_after_star` cases
  cover reuse after each command.
