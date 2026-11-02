---
task_uid: "01m6ce8a7vdde0dygesn2ew8rj"
task: "T402"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test quick_scope_targets (crates/fe-vim/tests/quick_scope.rs) passes: 'foo bar baz' from the caret on f gives b at 4 and z at 10, both primary, forward; the backward, secondary, no-target, caret-on-blank, empty-row and non-ASCII cases give what the card's rules give"
    required: true
    manual: false
  - id: "C2"
    text: "Test quick_scope_is_bounded passes: on a 100k-char row targets returns the results the card's rules give for the first MAX_CHARS chars each way (a row whose later chars would change the result shows the cap). Known limit: the test does not count the chars the scan reads (instrumentation), and a grapheme-boundary check at the edge of the window may drop the last accepted char there; both are in the follow-up card"
    required: true
    manual: false
  - id: "C3"
    text: "Test find_direction_reports_pending_finds passes: Some(true) while f, t, df or 2t waits for its char, Some(false) for F, T and dF, None otherwise and once the char is typed"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-vim passes"
    required: true
    manual: false
---

# fe-vim: quick_scope::targets lists the caret row's f and F targets (each word's first char unique so far as primary, else its first seen twice as secondary) and Pending::find_direction tells a pending f or t from F or T (Mac T1292)

## Problem

The user approved quick-scope as a built-in vim feature on 2026-11-01: in normal mode, highlight on the caret's
row the first unique character of each word to the right and left as `f`/`F` targets (primary and secondary
colours), optionally only after pressing `f`/`F`/`t`/`T`. This card is the GPUI-free half: which chars to
highlight, and which way a pending find points. The rendering, setting and colours are the next card.

Today nothing computes targets, and a host can't tell a pending `f`/`t` from `F`/`T`: `Pending::kind()` gives
`PendingKind::Find` for both (`crates/fe-vim/src/reducer.rs:197`), and `Prefix` is private to the reducer.

## Change

New `pub mod quick_scope` (`crates/fe-vim/src/quick_scope.rs`, declared in `crates/fe-vim/src/lib.rs`):

```rust
pub const MAX_CHARS: usize = 1000; // quick-scope's g:qs_max_chars default
pub struct Target { pub offset: usize, pub primary: bool }
pub fn targets(text: &Text, caret: usize, forward: bool) -> Vec<Target>
```

The rules (these define the feature; quick-scope itself can't be run here):
- Only the caret's row, from the char after the caret (forward) or before it (backward), outward, at most
  `MAX_CHARS` chars, reading the row a chunk at a time: no work proportional to a huge row.
- Accepted chars are ASCII letters and digits (quick-scope's default `g:qs_accepted_chars`); a word is a maximal
  run of them; every other char separates words.
- Walking outward, count each accepted char's occurrences so far, case-sensitively (as `f` matches).
- The rest of the word the caret is on, in that direction, is counted but not highlighted. A caret on a
  separator skips nothing.
- In each later word, the target is the char, nearest the word's start (its leftmost), whose count at that point
  of the walk was 1: primary (`f{c}` reaches it). Failing that, the leftmost whose count was 2: secondary
  (`2f{c}`). Failing both, the word has none.
- Results in row order. A grapheme with combining marks is a separator.

`Pending::find_direction(&self) -> Option<bool>` (`crates/fe-vim/src/reducer.rs`): `Some(true)` while `f` or `t`
waits for its char (after an operator too), `Some(false)` for `F`/`T`, else `None`.

Tests: new `crates/fe-vim/tests/quick_scope.rs`, e.g. `foo bar baz` with the caret on `f`: forward targets are `b`
(4, primary) and `z` (10, primary); include a backward case, a secondary, a word with neither, a caret on a
blank, an empty row, a row of 100k chars (bounded), and a non-ASCII word.
