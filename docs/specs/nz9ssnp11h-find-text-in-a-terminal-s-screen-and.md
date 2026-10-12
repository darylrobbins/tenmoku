---
task_uid: "01m4s5kb10y558trnz9ssnp11h"
task: "T60"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-terminal test search::next_and_previous: after printing 40 numbered lines into a 10-row grid, find(\"line 05\", forward) returns a match on a negative (history) line, the next snapshot shows it with display_offset \u003e 0 and its cells in Snapshot::selection; find forward again goes to the next match after it, wrapping past the last back to the first, and find backward returns to the previous one"
    required: true
    manual: false
  - id: "C2"
    text: "fe-terminal test search::literal_regex_and_case: literal \"a.b\" doesn't match \"axb\" but regex \"a.b\" does; \"Foo\" matches \"Foo\" not \"foo\" (smart case), case_sensitive Some(false) makes it match \"foo\", Some(true) makes \"foo\" not match \"FOO\"; regex \"(\" is Err(QueryError)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-terminal test search::no_match_and_visible: a query with no match is Ok(None) and leaves the selection and display offset as they were; find_all_visible at the bottom returns only matches on screen rows (none from history), and a screen full of \"a\" with query \"a\" returns exactly 1,000"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-terminal passes and scripts/check-no-gpui.sh prints ok for fe-terminal; no alacritty_terminal type appears in fe-terminal's public API; no Cargo.toml or Cargo.lock change; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Find text in a terminal's screen and scrollback in fe-terminal: literal or regex, next and previous with wrap, the match selected and scrolled into view (Mac T573)

Filed on argon from Mac T573 (card ~/coordination/argon-cards/T573.json rev 1). Mac task uid 01m4s5bxs0w4y4pp1425dah126. Title the host commit "... (Mac T573)".

## Problem

The terminal panel (fe-terminal + fe-terminal-view, T378–T382) has scrollback and selection but no way to find text
in it: VS Code's, Zed's and every terminal's cmd-f. alacritty_terminal 0.26 already ships a regex search over the
grid and history (`term::search::RegexSearch`, `Term::search_next`), unused by `crates/fe-terminal`. Nothing on the
board covers terminal find.

## Change

GPUI-free, in fe-terminal; alacritty types stay out of the API (as `crates/fe-terminal/src/lib.rs` requires):

- New `crates/fe-terminal/src/search.rs` (`mod search;` and its `pub use` in `crates/fe-terminal/src/lib.rs`):
  `TerminalQuery { text: String, regex: bool, case_sensitive: Option<bool> }` (a literal is escaped; `None` keeps
  alacritty's smart case, `Some` forces it with `(?-i)`/`(?i)`), `GridPoint { line: i32, col: usize }` (line 0 is
  the screen's top row, negative lines are scrollback, as alacritty counts them) and `TerminalMatch { start, end }`
  (inclusive). A query that doesn't compile is `Err(QueryError(String))`.
- `Terminal::find(&self, query: &TerminalQuery, forward: bool) -> Result<Option<TerminalMatch>, QueryError>` in
  `crates/fe-terminal/src/session.rs`: searches from the current selection's end (forward) or start (backward), else
  from the cursor, wrapping through the whole history once; on a match it selects it (`term.selection`, so
  `Snapshot::selection` paints it with no view change) and scrolls the display so its first line is visible (the
  view unscrolled if it's on screen). No match: selection and scroll unchanged, `Ok(None)`.
- `Terminal::find_all_visible(&self, query) -> Result<Vec<TerminalMatch>, QueryError>`: the matches intersecting
  the visible rows at the current display offset, at most 1,000, for highlighting.
- Wide characters and wrapped lines match as alacritty's search does (a match may span a soft wrap).

The find bar UI in fe-terminal-view is a later card (its `view.rs`, `input.rs` and `mouse.rs` are held by T567).

## Tests

New file `crates/fe-terminal/tests/search.rs`, driving `/bin/sh -c 'printf …'` sessions like
`crates/fe-terminal/tests/session.rs`.

## Implementation notes

- `crates/fe-terminal/src/search.rs` compiles the query with `RegexSearch::new` on each call (a literal is escaped
  by hand, since fe-terminal has no `regex` dependency) and runs `Term::search_next` with no line limit, so a find
  wraps through the whole history once. As in alacritty's own search, the next find starts one cell past the
  selection's end (forward, matching on a match's start) or one before its start (backward, on its end).
- An empty query matches nothing: `find` is `Ok(None)` and `find_all_visible` is empty.
- `find_all_visible` iterates from the start of the top visible row's wrapped line to the end of the bottom row's,
  so a match wrapping into view from above is returned whole.
- `TerminalQuery::literal` and `TerminalQuery::regex` build smart-case queries.
