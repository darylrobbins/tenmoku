---
task_uid: "01m53var5y2vjfmqdvwt5c39br"
task: "T124"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test visual_quote_selection::quotes_over_a_selection_match_nvim passes: each of the 11 cases in the spec's first table selects nvim's anchor..cursor (yanked text, cursor, mode v) where fe-vim failed, and it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "The same file's test quotes_nvim_already_matched_stay passes: vi\" at 7 (5..11), vi\"i\" at 7 (4..12), vlllllli\" at 6 (6..12), vllli\" at 9 (9..12) and vhhhhhhhi\" at 10 (10..3) on foo \"bar baz\" qux are unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test or corpus expectation edited (visual_matches_neovim, visual_block_objects and the operator-pending quote tests included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: i" a" i' a' typed over a visual selection of more than one position select or extend it as nvim does, instead of failing (Mac T731)

Filed on argon from Mac T731 (card ~/coordination/argon-cards/T731.json rev 1; note zp). Mac task uid 01m53v4rv17r3txwdd3vcv9tfn. Base: argon trunk fd842fd (= mac/main a6fea65 synced). Title the host commit "... (Mac T731)".

## Problem

A quote object (`i"` `a"` `i'` `a'`) typed in charwise visual mode over a selection of more than one
position fails in fe-vim, leaving the selection as it was, in most cases where nvim selects or extends.
`crates/fe-vim/src/visual/object.rs` `select_object`, its last branch: for a quote object, the object at
the cursor is only used when it equals the selection (then the next one out, with its quotes); any other
object is dropped (`false => (!quotes).then_some(found)`) and the call fails.

nvim follows `current_quote` in vim's `src/textobject.c`: for a non-empty selection on one row it checks
whether the selection is exactly the inside of a quoted string (`inside_quotes`) and whether it holds a
quote (`selected_quote`), finds the string the selection's end is in or after, and then moves the
selection's start to the string's start only when the selection was empty, was just inside the quotes,
or neither started at nor just after a quote and held none; the end always goes to the string's end
(past the closing quote for `a`, `2i"`, or a selection that was just inside).

Probed at a6fea65f against `nvim --clean --headless` 0.12.6 (the oracle's settings), selections as
anchor..cursor columns on one row:

| buffer | cursor | keys | nvim | fe-vim |
|---|---|---|---|---|
| `foo "bar baz" qux` | 6 | `vli"` | 5..11 | 6..7, fails |
| `foo "bar baz" qux` | 2 | `vllli"` | 2..11 | 2..5, fails |
| `foo "bar baz" qux` | 6 | `vi"a"` | 4..13 | 5..11, fails |
| `foo "bar baz" qux` | 7 | `vhi"` | 11..5 | 7..6, fails |
| `foo "bar baz" qux` | 6 | `vlla"` | 4..13 | 6..8, fails |
| `foo "bar baz" qux` | 15 | `vhhhhhi"` | 15..5 | 15..10, fails |
| `x 'b c' d` | 3 | `vli'` | 3..5 | 3..4, fails |
| `x 'b c' d` | 3 | `vla'` | 3..7 | 3..4, fails |
| `"ab" x "cd"` | 1 | `vi"i"i"` | 0..9 | 0..3, fails |
| `"ab" x "cd"` | 1 | `vlllli"` | 1..9 | 1..5, fails |
| `foo "bar baz" qux` | 5 | `vlli"` | 5..11 | 5..7, fails |

Already the same as nvim, and must stay so: `vi"` at 7 (5..11), `vi"i"` at 7 (4..12), `vlllllli"` at 6
(6..12), `vllli"` at 9 (9..12), `vhhhhhhhi"` at 10 (10..3).

## Change

- Give quote objects in a charwise visual selection of more than one position their own path, ported
  from `current_quote`'s visual-mode rules (`vis_bef_curs`, `inside_quotes`, `selected_quote`, the
  on-a-quote search for the next string, the search from the row's start for the string holding the
  selection's end, and the start/end placement above, including backward selections), in a new
  `crates/fe-vim/src/visual/quote.rs` declared in `crates/fe-vim/src/visual.rs`. `select_object` calls it
  for `InnerDoubleQuote | ADoubleQuote | InnerSingleQuote | ASingleQuote` when the selection isn't one
  position; a search that finds nothing fails as now. Reuse `text_object.rs`'s row scan and escape
  handling (`Scan`, the `quotes` helpers), widening visibility there if needed, rather than copying them.
- Unchanged: an empty (one-position) selection, blockwise selections (T650's block rules), backtick
  objects (they go through `delimiters::select_visual`), bracket and word objects, and operator-pending
  `i"`.
- New test file `crates/fe-vim/tests/visual_quote_selection.rs`, `HeadlessVim` cases with expectations
  probed from `nvim --clean --headless` 0.12.6 as in `tests/visual_block_objects.rs`: every row of both
  tables above, checked by yanking (`y`) and comparing the unnamed register, the cursor and the mode.
