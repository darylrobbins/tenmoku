---
task_uid: "01m54r2dmw09rrzcpd38k8eg6r"
task: "T133"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test upper_and_lower_classes_match_as_nvim (tests/search_case_classes.rs, top level) passes, as nvim --clean does: on \"aZ é É ß Ω ω\" :s/[[:upper:]]/#/g gives \"a# é # ß # ω\", :s/[[:lower:]]/#/g and :s/\\c[[:lower:]]/#/g give \"#Z # É # Ω #\", :s/[^[:lower:]]/#/g gives \"a##é###ß###ω\", :s/[a[:upper:]]/#/g gives \"## é # ß # ω\"; on \"ǅ İ ı ß ẞ ﬁ ª 𝐀 Ⅰ ⓐ µ\" :s/[[:upper:]]/U/g gives \"U U ı ß U ﬁ ª 𝐀 U ⓐ µ\" and :s/[[:lower:]]/L/g gives \"L İ L L ẞ ﬁ ª 𝐀 Ⅰ L L\"; /[[:upper:]]\u003cCR\u003e on \"x\\nÉa\" lands at (1,0); d/[[:lower:]]\u003cCR\u003e on \"AB cD\" leaves \"cD\"; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "The class tables are built once (OnceLock), and the classes already supported keep their behaviour, \\c folding them within ASCII as now (:s/[[:alpha:]]/#/g on \"abc\" gives \"###\"); [:print:], [:ident:], [:keyword:] and [:fname:] stay unsupported"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test edited (the oracle corpora included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: [:upper:] and [:lower:] in a search or :s collection match upper- and lowercase letters, Unicode ones too and unfolded by \c, as nvim does, instead of refusing the pattern (Mac T764)

## Problem

A `[:upper:]` or `[:lower:]` class in a collection (`[[:upper:]]`, `[^[:lower:]]`, `[a[:upper:]9]`) makes
fe-vim refuse the whole pattern, so `/`, `?`, `:s` and an operator over such a search fail. nvim matches them.
`crates/fe-vim/src/search.rs` `named_class` lists `alnum alpha blank cntrl digit graph punct space xdigit
return tab escape backspace` and nothing else, so `collection` reports `PatternError::Unsupported("[:upper:]")`.

nvim's two classes take Unicode letters, not only ASCII, and `\c` (or 'ignorecase') doesn't fold them:
`\c[[:lower:]]` still skips `Z`. Fitted from `nvim --clean --headless` 0.12.6 probes of about 10,000 code
points (U+0020-U+24FF, U+2C00-U+2E7F, U+A640-U+A7FF, U+AB30-U+AB6F, U+FB00-U+FB4F, U+FF00-U+FFEF, U+10400-U+104FF,
U+1D400-U+1D4FF, U+1E900-U+1E95F; the only misfits were characters newer than the probing tables):

- `[:upper:]` holds a character whose lowercase (`char::to_lowercase`) isn't itself, or that is another
  character's one-character uppercase: `Z É Ω ẞ Ⅰ Ⓐ İ ǅ`, not `𝐀` (no case mapping).
- `[:lower:]` holds a character whose uppercase is one character other than itself, or that is another
  character's one-character lowercase: `a é ω ß ı µ ⓐ ǅ`, not `ﬁ` (uppercase `FI`), `ª` or `𝐀`.

Reproduced at Mac main 838c0b35 with `HeadlessVim` against nvim, caret at (0,0):

| buffer | keys | nvim | fe-vim |
|---|---|---|---|
| `aZ é É ß Ω ω` | `:s/[[:upper:]]/#/g<CR>` | `a# é # ß # ω` | pattern refused, text unchanged |
| `aZ é É ß Ω ω` | `:s/[[:lower:]]/#/g<CR>` | `#Z # É # Ω #` | refused |
| `aZ é É ß Ω ω` | `:s/[^[:lower:]]/#/g<CR>` | `a##é###ß###ω` | refused |
| `aZ é É ß Ω ω` | `:s/\c[[:lower:]]/#/g<CR>` | `#Z # É # Ω #` | refused |
| `aZ é É ß Ω ω` | `:s/\c[[:upper:]]/#/g<CR>` | `a# é # ß # ω` | refused |
| `aZ é É ß Ω ω` | `:s/[a[:upper:]]/#/g<CR>` | `## é # ß # ω` | refused |
| `ǅ İ ı ß ẞ ﬁ ª 𝐀 Ⅰ ⓐ µ` | `:s/[[:upper:]]/U/g<CR>` | `U U ı ß U ﬁ ª 𝐀 U ⓐ µ` | refused |
| `ǅ İ ı ß ẞ ﬁ ª 𝐀 Ⅰ ⓐ µ` | `:s/[[:lower:]]/L/g<CR>` | `L İ L L ẞ ﬁ ª 𝐀 Ⅰ L L` | refused |
| `ab Cd` | `/[[:upper:]]<CR>` | caret (0,3) | fails, caret (0,0) |
| `x\nÉa` | `/[[:upper:]]<CR>` | caret (1,0) | fails |
| `AB cD` | `d/[[:lower:]]<CR>` | `cD` | fails, unchanged |

The classes already supported must stay as they are (`:s/[[:alpha:]]/#/g` on `abc` gives `###`, and `\c`
still folds them within ASCII).

## Change

In `crates/fe-vim/src/search.rs`:

- Give `Collection` a third member list for `[:upper:]` and `[:lower:]`, which `Collection::syntax` puts in
  the class as it is, folding it under neither `\c` nor 'ignorecase' (the `ascii` list folds within ASCII and
  `folded` by the case orbits; these do neither). `collection` fills it for the two names (setting `shape` to
  `None`, as the other classes do) and they can't start or end a range, as now for every class.
- The ranges come from one table per class, built once (`std::sync::OnceLock`) from the rules above over all
  `char`s, as sorted, merged ranges: the reverse lookups ("another character's one-character uppercase /
  lowercase") are a pass over all chars collecting their one-character mappings.
- Update the `named_class` doc ("the ASCII ones only") to say where `upper` and `lower` are handled.

Not in scope: `[:print:]`, `[:ident:]`, `[:keyword:]` and `[:fname:]`, which depend on nvim options and stay
unsupported.

Regression test: a new integration test `crates/fe-vim/tests/search_case_classes.rs`, at the top level of the
file, `upper_and_lower_classes_match_as_nvim`, driving `fe_vim::HeadlessVim` through the table's rows (text,
caret, and that the pattern isn't refused). Probe any added row with `nvim --clean --headless` first.
