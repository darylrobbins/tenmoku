---
task_uid: "01m54a65c7s0bmw9b92r66ghnv"
task: "T128"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test a_quote_object_on_a_quote_takes_an_escaped_quote_outside_a_string_as_an_opener (tests/quote_object_escaped_opener.rs, top level) passes: with the caret on an unescaped quote, i\" a\" i' a' i` a` pair the row's quotes walked from its start, outside a string an escaped quote opening one, as nvim --clean 0.12.4 does (\\\"a\" at 3, di\" deletes a; a\"b\" \\\"c\" at 8, di\" deletes c; \\\"a\" x\"y\" at 6, di\" deletes y; x\\\"a\"b\" at 6, di\" fails and changes nothing; \\'a' at 3, di' deletes a; \\\"a\" at 3, da\" leaves \\); it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Carets not on an unescaped quote keep today's pairs (\\\"a\" x\"y\" at 5 deletes ' x'; \\\"a\" at 1 deletes a; \"a\\\" at 3 fails), and a\"'s blanks and 2i\" are unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test edited (the text_objects and delimiters oracle corpora included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: i" a" i' a' on a quote pair the row's quotes from its start, an escaped quote outside a string opening one, as nvim does (Mac T748)

## Problem

With the caret on a quote, nvim's `i"` `a"` (and `i'` `a'`, `` i` `` `` a` ``) pair the row's quotes by
walking it from its start, and the escape char only escapes a quote *inside* a string: outside one, an
escaped quote (`\"`) still opens a string. fe-vim's `crates/fe-vim/src/text_object.rs` `quotes()` (line
375 at Mac c092189b) drops every escaped quote before pairing ("On a quote, the real quotes before it say
whether it opens"), so a quote after an escaped one outside a string pairs with the wrong neighbour or
fails.

Reproduced at Mac c092189b with `HeadlessVim`, against `nvim --clean --headless` (0.12.4 on neon, the
oracle's pinned version, and 0.12.6 on the Mac agree). One row, caret at a byte column, then the keys:

| row | caret | keys | nvim | fe-vim |
|---|---|---|---|---|
| `\"a"` | 3 | `di"` | `\""`, deletes `a` | nothing |
| `\"a\"b"` | 6 | `di"` | `\""`, deletes `a\"b` | nothing |
| `a"b" \"c"` | 8 | `di"` | `a"b" \""`, deletes `c` | nothing |
| `"x\"y" \"z"` | 10 | `di"` | `"x\"y" \""`, deletes `z` | nothing |
| `\"a" x"y"` | 6 | `di"` | `\"a" x""`, deletes `y` | deletes ` x` (pairs 3 with 6) |
| `x\"a"b"` | 6 | `di"` | fails, nothing changes | deletes `b` (pairs 4 with 6) |
| `\'a'` | 3 | `di'` | `\''`, deletes `a` | nothing |
| `` \`a` `` | 3 | ``di` `` | `` \`` ``, deletes `a` | nothing |
| `\"a" ` | 3 | `da"` | `\`, deletes `"a" ` | nothing |
| `\"a"` | 3 | `c2i"z<Esc>` | `\z` | nothing |

Rows where the caret is not on an unescaped quote already match: `\"a" x"y"` with the caret on `x` (5)
pairs 3 with 6 in both; `\"a"` with the caret on the escaped quote (1) takes `a` in both; `"a\"` with the
caret on the escaped closing quote (3) fails in both. So do the corpus cases in
`tests/corpus/text_objects.toml` (`i_quote_escaped`, `i_quote_on_escaped`, `i_quote_escaped_first`,
`i_single_quote_on_escaped_first`, ...).

## Change

In `quotes()`, when the caret is on an unescaped `quote`, pair it by walking the row's `quote` chars from
the row's start: outside a string any of them opens one, escaped or not; inside one, an escaped one is
skipped and an unescaped one closes it. If the walk reaches the caret's quote inside a string, the pair is
that string's opener and the caret; if outside, the caret opens and the next unescaped quote after it
closes (none: `Span::Failed`). Everything after the pair is chosen (`inner`, `a"`'s blanks, `2i"`) and
every other caret position (between quotes, on an escaped quote) stay as they are. Whether a quote is
escaped stays `Scan`'s `escaped` (so `\\"` is an unescaped quote).

Regression test: a new integration test `crates/fe-vim/tests/quote_object_escaped_opener.rs`, at the top
level of the file, `a_quote_object_on_a_quote_takes_an_escaped_quote_outside_a_string_as_an_opener`,
driving `fe_vim::HeadlessVim` through the table's rows (and the three matching rows above, which must keep
passing) and asserting the text and register nvim gives (probe any added row with `nvim --clean
--headless` first). `cargo nextest run -p fe-vim` stays green, the `text_objects` and `delimiters` oracle
corpora included.
