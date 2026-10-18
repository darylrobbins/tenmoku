---
task_uid: "01m55efcwzpbmsn8yeaxb0d38q"
task: "T152"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test character_code_atoms_match_as_nvim (new crates/fe-vim/tests/search_code_atoms.rs, top level, HeadlessVim only) passes, as nvim --clean 0.12.6 does: on \"a.b Foo €x\\ttab\\n  *star  00 é\" from (0, 0), /{pattern}\u003cCR\u003e lands at (0, 4) for \\%d70 \\%x46 \\%o106 \\%x46\\%x6f\\{2} \\v%x46oo \\V\\%x46oo \\c\\%x66oo; at (0, 5) for o\\%x6f; at (0, 8) for \\%u20AC \\%u20acx \\%U000020AC \\%d8364 \\%u20AC\\{1}; at (0, 1) for \\%x2e and \\%x2E; at (0, 12) for \\%d9tab; at (1, 2) for \\%x2a\\+ \\%o52 \\%o052s; at (1, 8) for \\%o4000; at (1, 12) for \\%d233 \\%d00233 \\%u0e9; and \\%d, \\%xg, \\%u and \\%d2147483648 stay at (0, 0) with the message 'invalid pattern: invalid character after \\%[dxouU]'; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Codes 0 and 10, a surrogate and a value past U+10FFFF are refused as unsupported; every other \\% atom but \\%( stays unsupported as at the base; collections ([\\x41] and the rest) and every pattern the base takes parse and match as they do at the base"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test edited (the oracle corpora included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: \%d123, \%o40, \%x2a, \%u20AC and \%U1234abcd outside a collection match the character they code, as nvim's do, instead of refusing the pattern (Mac T807)

## Problem

nvim's `\%d123`, `\%o40`, `\%x2a`, `\%u20AC` and `\%U1234abcd` (`:help /\%d`) match the character whose code
they give, anywhere in a pattern. fe-vim reads these codes only inside a collection
(`crates/fe-vim/src/search.rs`, the collection char reader). Outside one, the `'%'` arm of `Lexer::operator`
refuses every `\%` atom but `\%(`. So at Mac main 92c57c57, `/\%x46<CR>` on `a.b Foo` fails with "pattern not
supported: \%x" and the caret stays at (0, 0), where `nvim --clean --headless` (0.12.6) lands on the `F` at
(0, 4). `\%d70`, `\%o106`, `\%u20AC`, `\%U000020AC`, `\v%x46` and `\c\%x66oo` fail the same way.

nvim's reader (regexp.c: getdecchrs, getoctchrs, gethexchrs) works like this, checked against nvim 0.12.6:

- `d` takes every decimal digit. `x`, `u` and `U` take at most 2, 4 and 8 hex digits, in either case.
- `o` takes at most 3 octal digits and stops once the value reaches 0o40. So `\%o4000` is a space followed
  by `00`, and `\%o052s` is `*s`.
- No digit, or a value past 2147483647, is E678 ("Invalid character after \%[dxouU]").
- The character is an ordinary literal. A multi may follow (`\%x2a\+`), and `\c` folds it (`\c\%x66oo`
  matches `Foo`).

search.rs is byte-identical at mac/main ecdf8603 and Mac main 92c57c57, so no sync is needed.

## Change

In `crates/fe-vim/src/search.rs` `Lexer::operator`, add an arm before the catch-all `'%'` arm, for `\%` (or
`%` under `\v`) followed by `d`, `o`, `x`, `u` or `U`:

- Read the digits by nvim's rule above and push `Token::Char` of the value.
- No digit, or a value past `i32::MAX`, is `PatternError::Invalid("invalid character after \\%[dxouU]")`.
- A value of 0 or 10 is refused as unsupported. In nvim both match a NUL, an artefact of its line storage,
  and fe-vim matches row by row with no `\n`.
- A value that is no `char` (a surrogate, or past U+10FFFF) is refused as unsupported too. nvim matches
  nothing there.

Every other `\%` atom stays unsupported. The crate doc's "Search patterns" list is in lib.rs, which T769 holds,
so leave it for a docs follow-up. T764's `[:upper:]`/`[:lower:]` already left that list behind.
