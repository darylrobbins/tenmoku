---
task_uid: "01m551gvppx8vt5vdc1kwy4zxj"
task: "T137"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Test fe-terminal::search_smart_case_escapes an_escape_letter_does_not_make_a_regex_case_sensitive (tests/search_smart_case_escapes.rs, top level) passes: on a row reading xBAR! Bar bar!, TerminalQuery::regex(r\"\\Sar\") and TerminalQuery::regex(r\"ar\\W\") with no case setting each find 3 matches, as nvim --clean with ignorecase smartcase finds; it fails at the base (2 each)"
    required: true
    manual: false
  - id: "C2"
    text: "Unchanged: a regex's own uppercase letter (\\SAR) still makes it case-sensitive, a literal query's letter after a backslash still counts, Some(true) and Some(false) still override smart case, and fe-terminal::search literal_regex_and_case passes as is"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-terminal passes with no existing test edited; cargo clippy -p fe-terminal --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-terminal: a smart-case regex search stays case-insensitive when its only uppercase letters follow a backslash (\S, \W), as vim's smartcase does, instead of turning case-sensitive (Mac T779)

## Problem

A terminal query with no case setting (`TerminalQuery::case_sensitive: None`) is meant to be smart case:
case-insensitive unless it has an uppercase letter (`crates/fe-terminal/src/search.rs`, `TerminalQuery`'s
doc), "as vim's `/` with `smartcase`" (T614's spec for the vi copy mode, whose `/` and `?` build
`TerminalQuery::regex` in `crates/fe-terminal-view/src/vi.rs`). `TerminalQuery::pattern` passes `None` through
untouched and leaves the choice to alacritty's `RegexSearch::new` (alacritty_terminal 0.26.0
`src/term/search.rs:39`: `search.chars().any(|c| c.is_uppercase())`), which counts the letter of an escape.
So `\S`, `\W`, `\D` or `\B` in a regex make the whole query case-sensitive.

Reproduced at bcb3ea60: on a row reading `xBAR! Bar bar!`, `TerminalQuery::regex(r"\Sar")` finds 2 matches
(`Bar`, `bar`) and `TerminalQuery::regex(r"ar\W")` finds 2. Vim's 'smartcase' skips the character after a
backslash: `printf 'one\nxBAR\nBar\n' > f; nvim --clean --headless -c 'set ignorecase smartcase' -c
'call cursor(1,1)' -c "echo search('\Sar') getpos('.')[1:2]" -c 'qa!' f` prints `2 [2, 2]`, the `BAR` on
line 2, matched ignoring case. So vim finds 3 for `\Sar` and 3 for `ar\W` (`AR!`, `ar `, `ar!`).

## Change

In `crates/fe-terminal/src/search.rs`, make `TerminalQuery::pattern` always emit an explicit flag:
`(?-i)` or `(?i)`, with `None` decided by a new `TerminalQuery::has_uppercase`. That is true when `text`
has an uppercase letter, except that in a regex (`self.regex`) the character right after a `\` is skipped,
as vim's 'smartcase' skips it. A literal is unchanged: its backslash is an ordinary character, so the
letter after it counts. `Some(_)` keeps overriding smart case either way.

Test in a new `crates/fe-terminal/tests/search_smart_case_escapes.rs`, with the `Session::sh` harness
from `crates/fe-terminal/tests/search.rs` copied in (no shared test module).
