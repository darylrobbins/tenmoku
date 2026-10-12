# Vim search: exact match priority for repeats of mixed-ending and empty-first groups

Status: **decided**. Source card T351 (closes T349's known limit). Code:
`crates/fe-vim/src/search.rs`, `crates/fe-vim/src/search/backtrack.rs`.

## Problem

fe-vim translates a Vim pattern to one `regex`-crate pattern. A repeat of a
group whose ways end differently (`\%(a\|.\)*`: after `a` the next atom may
follow into a base's composing marks, after `.` it may not) or whose first way
is empty (`\(\|a\|b\)\+`) has to be written out in the order nvim tries things:
alternative order, greedy continuation against lazy stop, and which time last
set a capture. Written out, it is exact only for a few times (the "ordered
prefix", `ordered` in `search.rs`). Past it the exits sit inside the cycle, so
the order interleaves without bound, and one regex without lookahead or atomic
groups can't express it. T349 settled for a compact form past the prefix, which
picks another parse of the same text than nvim in some cases.

## Options

1. **A custom backtracking matcher**, used only when the translation can't keep
   nvim's priority.
2. **A sentinel-transformed haystack**: insert marker characters so one regex
   encodes iteration boundaries.

## Decision: the custom matcher

Option 1. Option 2 is rejected:

- A sentinel in the haystack can mark where a time ends, but not *which way*
  is tried first. Priority is a property of the pattern's choice points, and
  `regex` always takes leftmost-first over the text it is given, so the order
  would have to be put back into the pattern, which is the thing that cannot be
  written. Marking iteration boundaries only helps for the captures.
- Every offset the callers hold (matches, groups, columns) would have to be
  mapped back from the transformed text, and the sentinels would have to be
  something no row can hold and that no class, `.` or `\p{M}` takes. Composing
  marks, which this translation exists for, make that fragile.
- It would still need the compact form's text size (the matrix of ways grows
  with the square of a count).

The matcher (`search/backtrack.rs`):

- The pattern's tokens are read again into a tree (`Node`) and compiled to a
  small program: `Split` (ordered choice), `Jump`, `Save` (a group's ends),
  `Mark`/`Progress` (a loop's empty-time check) and `Leaf`.
- A `Leaf` is a character-taking atom (a literal, `.`, a class, a collection),
  run as the anchored `regex` the translator already writes for it, for each of
  the two ways the last atom ended (on a bare base or not). So what a character
  atom takes, its case folding and its composing characters, stay in one place.
  Only the order of choices is the matcher's own, and the way the last atom
  ended travels with the position and is restored with it.
- It runs with an explicit stack of choices still to try, so a long row can't
  overflow the call stack, under a step budget (`MAX_STEPS`), a stack budget
  (`MAX_STACK`) and the operation's `Deadline` (read every `CLOCK_EVERY`
  steps). Running out gives the deadline's expiry, as a slow regex does.
  Counts are written out, so the program is capped (`MAX_PROGRAM`).
- Greedy takes another time before stopping, lazy stops first, alternatives in
  order, and a time that takes nothing is the last (it is not retried), with
  its captures set, as nvim's does.

### When it is used

Every repeat of an atom with more than one way to end, and every unbounded
greedy repeat of a group that can match nothing and holds a capture, goes to
the matcher: the translator sets `inexact`, the regex text it wrote is never
compiled, and the `Pattern` is built from the tokens by `backtrack::compile`.
The written-out "ordered" form (bounded counts, `ordered`/`compose`) and the
compact form (`any`, `repeat_compact`, the matrix of ways) are deleted: a
differential run of ~2600 random patterns over ASCII rows against nvim 0.12.4
found the matcher exact on all of them, including 30 bounded repeats the
written-out form got wrong, so keeping two engines for these repeats had
nothing to buy. Everything else stays a plain `regex`.

## Consequences

- Exact priority for these patterns at any count, with no regex size limit
  (`MAX_MIXED_COMPILED` is gone).
- Backtracking can be exponential on a bad pattern and row. The budgets bound
  it; the result is an interrupted search, not a hang.
- A second, small interpreter to keep in step with the translator's reading of
  tokens (`Builder::branch`/`piece`/`atom` mirror `Translator`'s).
