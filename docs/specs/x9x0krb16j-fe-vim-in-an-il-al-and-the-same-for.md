---
task_uid: "01m5qphzcxj13e8ex9x0krb16j"
task: "T230"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Integration test seek_next_and_last_pairs (top level, crates/fe-vim/tests/seek_objects.rs): din( dan( dil( dal( cin\" yin[ din{ on one row and across rows act on the first pair after (n) or the last pair before (l) the caret, never the one containing it"
    required: true
    manual: false
  - id: "C2"
    text: "seek_counts_and_bounds: 2din( takes the second next pair; with no pair within 50 rows each way the command fails and changes nothing; a pair 51 rows away is not found; a 100,000-row buffer with no pair stays under the command deadline. Documented limit: the whole-row pair and quote preprocessing is not interruptible by the deadline on one very long row (T244, T230 F2)"
    required: true
    manual: false
  - id: "C3"
    text: "seek_repeat_and_visual: . repeats din( at the new caret, vin( selects the next pair, and an operator on an unmatched quote row follows the plain quotes rule. Documented limit: a second seek object in visual mode (vin( then in() does not grow the selection as the plain object does when anchor and cursor coincide (T244, T230 F3)"
    required: true
    manual: false
  - id: "C4"
    text: "seek_leaves_plain_objects_alone: di( and da\" behave as before, and the existing delimiters_oracle and feature_stubs tests pass unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-vim (the gate runner) and cargo clippy -p fe-vim --all-targets pass"
    required: true
    manual: false
---

# fe-vim: in( an( il( al( (and the same for [ { < quotes): the next or last pair's object, as targets.vim and mini.ai's n and l forms (Mac T1155)

## Problem

The user asked on 2026-10-24 for targets.vim-style next/last objects (`in(`, `an(`, `il(`, `al(`) or mini.ai's `n`/`l` forms, as Neovim defaults (LazyVim ships mini.ai; targets.vim and IdeaVim-ish setups give the same keys). `di(` fails when the caret is not inside parentheses; `din(` deletes inside the next pair, `dil(` the last one.

## Current state

- The char after `i` or `a` is mapped to a text object at once: `text_object(ch, inner).or_else(|| feature::object(self, ch, inner))` (`crates/fe-vim/src/reducer.rs:519`). `n` and `l` take no further key, so a two-key object has no hook (seam gap: found here, see memory fe-vim-feature-seam-gaps). Features wait for more keys with a `FeaturePrefix` returned from `key`/`g`/`bracket`, not from `object`.
- The pair and quote finders exist: `text_object::brackets` / `brackets_from` / `quotes` (`crates/fe-vim/src/text_object.rs:276,373,444`); `visual/object.rs:67-71` routes feature objects in visual mode.

## Change

- reducer.rs: in the text-object prefix, a feature may take the char before `feature::object`: new `Feature::object_prefix: Option<fn(&Pending, char, bool) -> Option<Step>>` (feature.rs) returning `Step::Wait(pending.with_feature_prefix(..))`, as `ds` does. No behaviour changes for other chars.
- New `crates/fe-vim/src/feature/seek.rs`: `in`/`an` (`n` after `i` or `a`) and `il`/`al` (`l`), then the object char: `( ) b [ ] { } B < >` and `" ' \``. The result is the same `Span` the plain object gives, for the first such pair that starts after the caret (`n`), or the last whose end is before it (`l`), not the one containing the caret; a count takes the nth. The search is bounded to 50 rows each way (mini.ai's `n_lines` default) and the command deadline. Charwise as the plain object is; quotes pair by the row's quote parity as `quotes` does. A new `TextObject::Seek(SeekObject)` variant (args.rs, Copy payload: direction, inner, object char), dispatched in text_object.rs, with its visual arm.
- Not here: seeking for the plain `i(` when the caret is outside any pair (targets.vim's default; diverges from nvim, would need its own setting), word/sentence/tag seeking, `]`/`[` motions.

## Tests

New `crates/fe-vim/tests/seek_objects.rs`: `din(`, `dan(`, `dil(`, `cin"`, `yin[` on one row and across rows; the caret inside a pair skips it for `n`; counts; nesting; failing with no pair in 50 rows; `.` repeat; visual `vin(`; and an nvim-oracle-free check that `di(` is unchanged (the existing delimiters corpus tests still pass).

## Implementation notes

- The reducer's text-object prefix tries its own objects, then `feature::object_prefix`, then `feature::object`; `Pending::object` (new) commits an object as `vim.select` in a visual mode or `vim.operate` otherwise, for the reducer and for a feature's `prefix` hook. `seek` waits on a `FeaturePrefix` named `seek` holding `[i|a, n|l]`; a char that names no pair cancels.
- `SeekObject { last, inner, pair }` holds the pair by its opening char (`(` for `( ) b`). It is re-exported from the crate root (`lib.rs`) beside `TextObject`, since it is that enum's public payload.
- Both ends of a pair must lie within 50 rows of the caret's. Rows are read in chunks of the text (`text_object::each_byte`), with no whole-row copy. Brackets are paired in one pass over those rows with the plain object's rules: escaped ones don't count, and one inside a double-quoted string pairs only within that string, the string parity coming from a first chunked pass as `motion::Scan` computes it. Pairs are kept in reading order, so no sort is needed: by opening bracket for `n`, and by closing bracket (the order matches are found) for `l`. The span comes from `text_object::pair_span`, split out of `brackets_at`. Quotes pair from the row's start as plain `i"` pairs them on a quote (any quote opens a string, only an unescaped one closes it), and seek's `quote_span` gives `quoted`'s span. Every phase (reading rows, choosing the pair, reading blanks or the row before a closing bracket for the span) looks at the deadline every 4 KiB or 4096 pairs. A pair found once the deadline has passed is none.
- Visual (`visual/object.rs::select_seek`): over one position the `count`th pair's span is selected directly, as the plain object's span would be (`object_ends` with the plain object, which now also gives an empty `` i` `` both backticks, and the shared `keeps_block` rule); over more, or over one position an object selected (`visual_by_object`, a one-char inside), the object is the plain one, so `vin(i(` and `vin(in(` grow as `vi(i(` does.
- Under `cargo test`'s threads, four lib tests (`search::across_tests::*`, `cmdline::offset::tests::counted_searches_over_rows_list_a_dense_row_once`) and `search_deadline_and_ranges::an_interrupted_search_returns_interrupted_not_a_result` can fail: they share the process-wide regex call budget. They pass alone and under nextest; nothing here touches them.
- Review F2 (rounds 1-3) is fixed here, not deferred: the deadline bounds every phase (unit test `feature::seek::tests::a_long_row_stops_at_the_deadline`, a 16 MiB row whose deadline passes part way). Follow-up T243, filed when it was deferred, is superseded.
