---
task_uid: "01m4rkcfatdkzrjngvmzwjxtgy"
task: "T564"
rev: 12
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "F16: the delimiters timeout test no longer changes the process-wide search limit while sibling tests run: it uses a per-call/thread-local limit or otherwise isolates itself, so 'cargo test -p fe-vim delimiters' passes in parallel 20 times in a row (record the command)."
    required: true
    manual: false
  - id: "C2"
    text: "F13: distinct grapheme clusters in a pattern tag name keep their own requirements: dit at column 0 on '\u003ca‍a‍́.\u003ex\u003c/a‍a‍x\u003e' (ZWJ and combining accent) leaves the text unchanged, as nvim 0.12.4 does; recorded as an oracle case."
    required: true
    manual: false
  - id: "C3"
    text: "F14: regex metacharacters (. * etc.) followed by a ZWJ are not treated as literals by the cluster substitution: dit on '\u003ca.‍\u003ex\u003c/a.‍\u003e' leaves the text unchanged and dit on '\u003ca*‍\u003ex\u003c/aaa‍\u003e' deletes x, as nvim does; recorded as oracle cases."
    required: true
    manual: false
  - id: "C4"
    text: "F15: placeholder code points never collide with characters in the text, and allocation cannot reuse a placeholder when the range runs out (fail cleanly instead): dit at column 0 on '\u003ca‍.\u003ex\u003c/󰀀x\u003e' leaves the text unchanged, as nvim does."
    required: true
    manual: false
  - id: "C5"
    text: "F18: from a bare '\u003c' directly before '\u003c/\u003e', the backward search keeps Neovim's boundary (the '\u003c/\u003e' doesn't cancel), as nvim 0.12.4 does on '\u003cab c\u003e\u003c\u003c/\u003ex…\u003c/ab\u003e'; recorded as an oracle case."
    required: true
    manual: false
  - id: "C6"
    text: "F19 (narrowed, rev after review round 3): lowercase tag-name pattern atoms valid in Vim but rejected by the reused search translation (\\k \\i \\f \\p, starting with \\k) are supported, not treated as unmatched tags, for names whose characters are not grapheme-substituted; oracle cases recorded. Out of scope (T1213): uppercase \\K \\I \\F \\P, and whether a substituted cluster placeholder belongs to a class atom, range or negated collection (review F1, F3, F9)."
    required: true
    manual: false
  - id: "C7"
    text: "Nothing outside the scoped files changes, except this lane's own docs/specs file."
    required: true
    manual: false
  - id: "C8"
    text: "F22 (narrowed, rev after review round 3): the clock is checked at least once per row and once per pattern name before UTF-8 validation, grapheme segmentation, name compare and each pattern compilation, and every loop over rows or names checks it at least every 4096 iterations; deterministic tests in src/tests/delimiters_work.rs expire the clock in the row loop, the name loop and pattern compilation and see the text object fail cleanly. Out of scope (T1213): a deterministic test for every individual checkpoint (review F6), per-grapheme / per-byte accounting inside a single row, compiler interruption and the cold keyword-table scan."
    required: true
    manual: false
  - id: "C9"
    text: "Pattern-name pairing compiles each name's pattern once and prefilters candidate '\u003c' positions with one unanchored search per row, so '\u003ca.b\u003e' followed by N '\u003cb\u003e' and then 'x\u003c/axb\u003e' runs in near-linear time, with a test (pattern_pairing_past_many_other_tags_finishes_in_time). Adversarial regex names need only stay bounded by the command clock at C8's check points."
    required: true
    manual: false
---

# Follow-ups from review of T417: tag-name grapheme clusters (F13-F15) and the delimiters timeout test's process-wide limit (F16)

## Implementation notes

- C1: the timeout test already used the thread-local `STOPPED` clock; the new phase tests use thread-local `LOOKS`. Nothing sets `search::set_time_limit`. Check: `for i in $(seq 20); do cargo test -p fe-vim delimiters; done`.
- C2-C4: a joined grapheme in a pattern name is now a class of stand-ins, one per combination of the name's joined graphemes a text grapheme satisfies (at most 8 distinct of one base char), so each keeps its own requirement. Stand-ins are private-use chars absent from the whole text (`private_use`), so none collides; with too few free ones, or too many joined graphemes, the name pairs with nothing. `.` and `*` (and the other magic chars) are not literals; a magic `.` joined to composing chars pairs with nothing, as nvim does.
- C5: a `<` whose first `>` follows a `/` (`<</>`) is not "in a tag", as nvim's `in_html_tag`, so the caret stays on it and the `</>` doesn't cancel the block before.
- C6: `\k \i \f \p` and their capitals (without digits) in a name become a collection of what nvim 0.12.4 matches with them (`\k` from the char table, the rest recorded ranges), spelled in the magic the name was in. A name that spells out to over 4 MiB fails the object.
- C8: tag text is kept as a `String` (no UTF-8 validation passes); every pass in `delimiters.rs` that is linear in the row or a name looks at the clock at most `WORK` (4096) units apart, tests reading the units passes really take between looks (`MOST_STEPS`) and expiring the clock at the first, a middle and the last look of each named phase. A grapheme longer than 8192 bytes (`SEGMENT`) can't be segmented in a bounded step, so a name, span or row holding one fails the object. A name that spells out to over 64 KiB fails the object, which bounds compilation (looked at in 4096-byte steps before it starts, as it can't be interrupted). The caret-row read (`Cursor::at`) is T594.
- C6/F1/F3: placeholders keep their base char's meaning: collections, escaped code points and class atoms in a name take in the stand-ins of the joined graphemes whose base they match, and private-use chars a name names by an escape are never stand-ins.
- C9: `Compiled` (per name, cached in `Tags`) holds the four patterns; one search per row finds where `<name`/`</name` match, and only those `<` are tried.
- Review round: a name holding `\z` (`\zs`, `\ze`) is not prefiltered (the row search's match start isn't the tag's `<`), with a regression test; each pattern compilation (open, close, and the two head patterns) is preceded by a clock look (F2).
- Out of scope, held by T1213: F1/F3/F9 (uppercase classes, range and negated-collection placeholder membership, a trailing `-` before a stand-in) and F6 (inner-operation interruption, per-grapheme accounting).
- C1 evidence: `for i in $(seq 20); do cargo test -p fe-vim delimiters; done` passed 20 of 20 (tests run in parallel by default; no test sets `search::set_time_limit`).
