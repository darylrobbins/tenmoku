---
task_uid: "01m4p2yzdt5g5y33ev9qg4ttqx"
task: "T417"
rev: 7
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim tests tests::delimiters::*: i[ a[ (and i] a]) and i\u003c a\u003c (and i\u003e a\u003e) select inside or around the count-th enclosing pair as i( does; the backtick objects select as the double-quote objects do on the row; it and at select inside or around the count-th enclosing tag pair, matching names case-insensitively and skipping self-closing and unmatched tags; a missing pair fails with nothing changed Out of scope, deferred to follow-up T564 (review F13-F16 (user override) and F18-F19): tag names containing ZWJ-joined or combining grapheme clusters, placeholder code points already present in the text, the timeout test's use of the process-wide search limit, a bare '\u003c' directly before '\u003c/\u003e' (F18), and tag-name pattern atoms the reused parser rejects, such as \\k (F19). Performance (review F22): no superlinear work in any phase except pattern-name pairing (tag names holding regex metacharacters such as \\ . * [ ~), which is bounded by the command clock and fails the object cleanly when the clock runs out; the gathering, searching, name-compare and tag-pass loops, Block::inner and the visual endpoints all check one command clock; the caret row is read from the gathered bytes, not Cursor::at. Out of scope, deferred to T564 (F22 remainder): making single linear passes interruptible (UTF-8 validation, whole-name byte compare, is_ascii, table reverse/dedup, the count-skipping reverse pass, pattern preparation), deterministic per-phase expiry tests and allocation measurement beyond Run entries, and the shared Cursor::at long-row cost every fe-vim motion and text object pays. Pattern-name pairing's algorithmic cost (compile once per name, prefilter candidate '\u003c' positions) is also deferred to T564."
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test delimiters_matches_neovim (tests/delimiters_oracle.rs): tests/corpus/delimiters.toml has at least 40 cases covering every object with d c y, counts, nesting, the cursor on a bracket, on a tag name and between tags, rows spanned, missing pairs, vit and va[ repeated, and dot after cit; each matches nvim 0.12.4"
    required: true
    manual: false
  - id: "C3"
    text: "Every earlier corpus expected file is byte-identical and every earlier fe-vim test passes; nothing outside src/delimiters.rs, its test file, oracle test and corpus changes, except: src/tests/feature_stubs.rs (only removing T417's 8 delimiter objects, inner_bracket … a_backtick, from the select loop of new_variants_change_nothing, their round_trips check kept), src/visual/object.rs (only an arm in select_object sending the delimiter objects to delimiters::select_visual) and src/text_object.rs (only making brackets and quotes pub(crate)); docs/specs/ev9qg4ttqx-fe-vim-i-a-i-a-it-at-and-backtick-text.md is exempt: Regatta's own Spec/Respec commits and the lane's '## Implementation notes' section in it (no other edits to that file)"
    required: true
    manual: false
---

# fe-vim: i[ a[ i< a< it at and backtick text objects, checked against nvim

# fe-vim: i[ a[ i< a< i` a` it at text objects

## Problem

fe-vim's text objects are `iw aw iW aW i( a( i{ a{ i" a" i' a'` only (`text_object.rs`;
`reducer.rs` `text_object()`; gap analysis 2026-10-11). `ci[`, `di<`, `cit` and `` ci` `` are
missing. The seam card this depends on declared `TextObject::InnerBracket … ABacktick` and the
empty `src/delimiters.rs` they delegate to.

## Change

Everything lives in `src/delimiters.rs` (`delimiters::FEATURE` already in `FEATURES`):

- `FEATURE.object` maps `[ ]` to the bracket objects, `< >` to the angle objects, `t` to the tag
  objects and `` ` `` to the backtick objects.
- `i[ a[ i< a<` select inside/around the `count`th enclosing pair as `i(` does, reusing
  `motion.rs`'s pair helpers (`nth_unmatched`, `Scan`) with `[]` and `<>`.
- `` i` a` `` select as `i"`/`a"` do on the row (`text_object.rs`'s `quotes` logic).
- `it at` select inside/around the `count`th enclosing tag pair (`:help tag-blocks`): tag names
  match case-insensitively, self-closing (`<br/>`) and unmatched tags are skipped, `it` on an empty
  element selects nothing, and a repeated `it`/`at` in visual mode grows to the enclosing pair as
  nvim's does.
- `src/delimiters.rs`'s module docs describe the objects.

**Clean room**: `nvim --clean` (0.12.4) and `:help i[`, `:help i<`, `:help it`,
`:help tag-blocks`, `:help i\``; never source, hjkl, Zed or Helix.

### Corpus

`tests/corpus/delimiters.toml`, at least 40 cases: each object with `d c y`, with counts, nested,
with the cursor on a bracket, on a tag name and between tags, across rows, missing pairs (the
failing key last), `vit`/`va[` repeated, and `.` after `cit`.

## Tests (new files)

- `src/tests/delimiters.rs` (the seam's stub).
- `tests/delimiters_oracle.rs` (`#[allow(dead_code)] mod support;`) with
  `delimiters_matches_neovim`.

## Size

About 250 non-test lines (tags most of it).

## Out of scope

Quotes escaped by 'quoteescape' beyond what `i"` already does, the GPUI check. Any change outside
`src/delimiters.rs` and its tests and corpus.

## Implementation notes

The rev 1 block (2026-10-11) was over three seam gaps outside the card's files. Rev 2 allowed the
three edits made: `feature_stubs.rs` skips the eight delimiter objects in its operate/select
loop (keeping their round trips), `visual/object.rs` sends them to `delimiters::select_visual`,
and `text_object.rs` makes `brackets` and `quotes` `pub(crate)`.

Tag rules fitted from `nvim --clean` 0.12.4 probes (exhaustive visual grids on six strings, about
6,000 selections, then a 4,000-case random differential run):

- A tag is `</name>` (exactly), `<name>`, or `<name` + blank + anything up to the first `>` (rows
  included); one whose `>` follows `/` is self-closing and doesn't count. A name is a run of
  bytes other than blanks and `>`, so it may hold `<` (`<x<y>z</x<y>` pairs); an opening tag's
  also ends at `/`, and one with `!` in it is no tag (`<!x>`, `<x!y>`). A closing tag is `</`,
  then anything but `>`, to a `>` on the same row (`</>`, `</ <b>`, `</a b>` and `</!x>` all
  cancel going back; `</a\nb>` doesn't). Names compare by simple case folding without
  CaseFolding.txt's `S` entries, as `\c` does: `Σ ς σ` and `K k` with the Kelvin sign match, but
  `ẞ` and `ß`, `ı` and `I`, and `ᾈ` and `ᾀ` don't.
- An opening tag's partner is the first `</name>` after it that one name's nesting leaves
  unmatched. Searching back from the caret for the opening tag, a closing tag of *any* name
  cancels the next opening tag back. So a stray `</b>` before the caret hides the `<p>` around it
  (`<p>a</b>c</p>` on `c` fails), and `<br>` is an unmatched opening tag. The `count`th opening
  tag back that nothing cancels is taken, or past it the first with a partner around the caret
  (`d2it` on `b` in `<div><p>a<br>b</p></div>` is `<p>`'s inside).
- A name is a pattern: an opening tag's partner is the first match after it of `</name>` that
  the matches of `<name` in between leave unmatched, case-blind, where `<name` must end in a
  word char (class 2 or more, as `\>` takes it) followed by a blank, `>` or the row's end. So
  `<a.b>` pairs with `</axb>` and nests `<axb>` but not `<a*>`, `<a\d>` pairs with `</a5>`,
  `<a*>` with `</>`, and a plain name ending in a non-word char nests nothing (`<a->`, `<a+>`,
  `<a。>`). The closing tag found ends at its first `>` (`</a.>` takes `</a>` of `</a>>`).
  Matches overlap (`<a.*>x<a>y</a>z</a>` nests the `<a>`), and the word end is checked where
  the name's own match ends. Flags in the name change neither the case folding (`<a\C>` still
  pairs with `</A>`) nor the magic of what nvim puts after it (`<a\M>`, `<a\V>` nest). A
  name that isn't a pattern on its own (`<a[>`, `<a~>` with no substitute yet, `<a\(b>`,
  `<a\|b>` with the branch outside a group, `<a\v>` ending very magic, a collection with no `]`
  such as `<a[]>`, a very magic `~`) pairs with nothing; it still counts back from the caret.
  Groups and branches inside them are fine (`<a\(b\|c\)>`), and a collection's contents are its
  own (`<a[\|]>`, `<a[~]>`); in the nomagic modes `[` and `~` are literal (`<a\V[>`).
  fe-vim reads names holding `\ . * [ ~` with its own `/` pattern translation, trying each `<`
  against each span it could take, under one search deadline: running out fails the object.
  A char joined by a ZWJ or emoji modifier, which that translation would fold as separate
  chars, is put as one private-use char in both the name and the span, so it matches only the
  same base char with at least its joined chars (`<a‍b.>` pairs with `</a‍Bx>`, not `</A‍Bx>`).
  The translation lacks `\k`, so a name with it pairs with nothing here.
- Names with composing chars compare grapheme by grapheme (nvim 0.12's clusters, so a ZWJ or an
  emoji modifier composes too): a grapheme of several chars matches only the same base char,
  exactly, with at least the opening name's composing chars, in any order and ignoring repeats
  (`<á>` pairs with `</á̂>`, not the other way round; `<aͅ>` not with `</Aͅ>`). So pairing isn't
  symmetric, and such an opening tag finds its partner by a forward search of its own. Composing
  chars right after an opening tag's `<` belong to the `<`: `<ͅa>` is named `a` and pairs with
  `</a>`, but no `<a>` counts it as nested. `<ͅ>`, with no name left, still counts back from the
  caret, but if it's the tag taken (the `count`th, or a later one past unpaired ones) the object
  fails. A closing tag's leading composing char is a char of its name (`</ͅ>` closes `<Ι>`).
- The caret, from a row's indent, starts at the first non-blank; blanks elsewhere don't move it.
  If it's within a `<…` on its row (on the `<` or after it, with no `>` between), it goes on to
  the next `>`, on any row, and the tags starting up to there count: on an opening tag that
  counts the tag and any tag in its attribute values (`dit` at 0 in `<a t="<b>x</b>">y</a>`
  deletes `x`), and a bare `<` sends it on (`<b>x<<i>z</i></b>` on the first `<` is `z`). With
  no `>` after, the object fails. Within a `</…` it goes back: the tags starting before the `<`
  count, so on a closing tag that tag doesn't (and a `</b>` in an attribute value wins over the
  opening tag around it). There's no other search forward.
- `it` starts past the opening tag's first `>` outside a `"…"`/`'…'` attribute value, or at the
  tag itself if none closes it. An empty inside is an empty range at the closing tag (`dit`
  moves there; `vit` selects its `<`).
- Visual, one position: as the operator does, but an `it` equal to the selection takes the tags.
  Wider: search back from the position before the selection (where a tag *starting* counts, so
  the position inside `</b>` cancels `<b>`), skip `count - 1`, then go out to the first block
  whose closing tag starts at or after the selection's end. `it` gives the inside, or the whole
  block if that inside is the selection.

Known divergences, all shared with the existing `i( i{ i" a"` objects that the spec asks these to
match (checked on paren/brace/double-quote twins of each fuzz case): nvim's visual `i(` from a
wider selection with no pair around it takes the next pair forward; visual `a{` from a cursor
past the row's last char; nvim's repeated or widening visual `a"`/`i"` adds or selects the next
string (`va"a"` on `"a" "b"` selects both); with `<…>` inside a quoted string, repeated visual
`i<` differs as repeated `i(` does with `(…)`.

Rev 4 review (round 1): F17 fixed (only `i[ i<` take a row's line break; `va[ va<` ending a
row stop at the bracket). F20 fixed: an unnested lone tag of a plain name gets its partner from
one pass back over the tags; the other lone and pattern tags are paired only when the search
back reaches them, all under one deadline per command that fails the object if it runs out.
F13-F16, F18 and F19 are deferred to T564 by C1.

Rev 4 review (round 2): F20 fixed (a count passes over openings unpaired; the deadline is checked
before a pattern is compiled), F21 fixed (gone on from a `<`, a block whose closing tag starts at
or before the caret isn't around it, so the caret on a `<` in `</x<y>` fails and a block further
out is taken), F22 fixed: tags keep their names as ranges with a key, and the names past each `<`
of a run, which end together, are read once per run; the scan itself runs under the command's
deadline.

Rev 4 review (round 3): F22 made bounded throughout. One clock per command starts before the
caret's row is read and is looked at every 16 Ki steps of gathering the text, searching for `<`
and `>`, reading names, comparing them and the passes over the tags; past it the object fails.
A run's table keeps the name only where a tag's name can start (the run's start, past a `<` or
`</` in it, its end), so a long name costs a few entries, not one per byte; names are compared
as streams of units, never built whole. ASCII runs are read byte by byte. The timeout tests stop
the clock through a thread-local switch, not the process-wide search limit.

Rev 6: the caret's row is read from the gathered bytes (clocked searches back and on for its
line breaks), not through `Cursor::at`; a block's inside start is found under the same clock and
kept on the block, and a visual selection checks the clock once its endpoints are made. The F22
remainder (interruptible single linear passes, per-phase expiry tests and allocation measurement,
the shared `Cursor::at` long-row cost) is deferred to T564 by C1.

Rev 6 review (round 1): F23 fixed (a run's name entries are looked up with a cursor that steps on
from the last lookup, since tags ask in ascending order). Rev 7: pattern-name pairing is exempt
from the no-superlinear rule (it tries each span a `<` could take and fails the object when the
command clock runs out); compiling once per name and prefiltering candidates moved to T564.

Round 4 review (F10 joined chars, F12 collection and magic context) added the collection,
magic and joined-char rules; 10,000 more fuzz cases with them left only the `</x<y>` family and
one bare `<` before `</>` (`<ab c><</>x…</ab>` from the `<`, where nvim doesn't let the `</>`
cancel).

Round 3 review (F6-F9, F11 pattern-name details) refitted the pattern rules above; 10,000 more
fuzz cases with groups, flags, collections and greedy names left no tag difference outside the
`/`-translation ones.

Round 2 review (F5 malformed closing tags, F6 pattern names) added the closing-tag and pattern
rules above; 15,000 more fuzz cases with pattern names and malformed closing tags left only the
families below.

Round 1 review (F1 tags in attributes, F3 composed names) refitted the caret and name rules
above; a scratch differential fuzz (one nvim per case, 30,000 random cases over tags in
attributes, bare `<` and `</`, `!`, composed and ZWJ names, rows and indents) then matched except
the shared divergences and one tag family: a malformed `</` followed by `<` or a blank, or the
caret on or past a `<` inside a closing tag's name (`</x<y>`, `</<y z</B>`, `</\t<`), where nvim
fails in ways no simple rule fitted (about 1 in 2,000 cases of that generator).
