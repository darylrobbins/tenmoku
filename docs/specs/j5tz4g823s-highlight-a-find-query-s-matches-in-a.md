---
task_uid: "01m4schcj4j5emjdj5tz4g823s"
task: "T64"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-terminal-view test find::matches_highlighted: after a session prints foo three times on screen, set_find(literal foo) paints find_quads covering exactly those three cell runs and find_matches has 3; set_find(None) paints none"
    required: true
    manual: false
  - id: "C2"
    text: "fe-terminal-view test find::find_next_selects_and_scrolls: with 200 numbered lines printed and the view at the bottom, find_next(false) for an early line selects it, scrolls it on screen (display_offset \u003e 0) and its row paints the selection; find_next past the last match wraps"
    required: true
    manual: false
  - id: "C3"
    text: "fe-terminal-view test find::bad_query_and_no_session: set_find with an invalid regex returns Err(QueryError) and paints no find_quads; find_next before the session starts returns Ok(None); nothing panics"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-terminal-view and cargo clippy -p fe-terminal-view --all-targets -- -D warnings pass, with the existing fe-terminal-view tests unchanged"
    required: true
    manual: false
---

# Highlight a find query's matches in a TerminalView and step through them with find_next, for the terminal's find (Mac T585)

Filed on argon from Mac T585 (card ~/coordination/argon-cards/T585.json rev 1). Mac task uid 01m4scf5wk8w6vgrvcqmev71xy. Its dependency Mac T573 is argon T60 (3657624), already on argon main, so it isn't recorded here. Title the host commit "... (Mac T585)".

## Problem

T573 gave fe-terminal `Terminal::find` and `Terminal::find_all_visible` (`crates/fe-terminal/src/session.rs`,
types in `crates/fe-terminal/src/search.rs`), and its spec left the UI to "a later card" in fe-terminal-view.
Nothing in `TerminalView` (`crates/fe-terminal-view/src/view.rs`) holds a query or paints its matches, so the
terminal panel can't offer find. This card is the view half; the panel's find row is the next card.

Built on argon's trunk: T573 (argon T60) and T579 (argon T63) are there, not yet on the Mac's main.

## Change

GPUI view only; no fe-workbench change, and `TerminalStyle` gains no field (its literals live in fe-workbench
and the tests).

- New `crates/fe-terminal-view/src/find.rs` (declared from `crates/fe-terminal-view/src/lib.rs`, which
  re-exports `fe_terminal::{TerminalQuery, TerminalMatch, QueryError}` beside `TerminalOptions`), with these
  `TerminalView` methods:
  - `set_find(&mut self, query: Option<TerminalQuery>, cx) -> Result<(), QueryError>`: stores the query (`None`
    or an empty `text` clears it) and notifies. With a session it checks the query by running
    `find_all_visible` once; on `Err` the query is not stored and the error is returned. Before the session
    starts the query is stored unchecked.
  - `find_next(&mut self, forward: bool, cx) -> Result<Option<TerminalMatch>, QueryError>`: with a query and a
    session, `Terminal::find(query, forward)` (which selects the match and scrolls to it) and notifies;
    `Ok(None)` without either.
  - `find_query(&self) -> Option<&TerminalQuery>` and `find_matches(&self) -> &[TerminalMatch]`: the matches
    the last prepaint found on screen, for the panel's "No results".
- `crates/fe-terminal-view/src/element.rs`, prepaint: while a query is set, one `find_all_visible` per
  prepaint (fe-terminal caps it at 1,000). Each match becomes per-row cell spans (row = line +
  `Snapshot::display_offset`, clipped to the grid; a match across a soft wrap paints on every row it covers),
  painted as background quads under the text and before the selection, in `style.selection` with its alpha
  halved, so the active match (the selection `Terminal::find` set) shows on top. An `Err` paints nothing.
  `PaintReport` gains `find_quads: Vec<Bounds<Pixels>>`, filled only while recording, like `quads`.
- `crates/fe-terminal-view/src/view.rs`: the field holding the query and the last matches; nothing else
  changes in the view.

## Tests

New file `crates/fe-terminal-view/tests/find.rs`, driving `/bin/sh -c 'printf …'` sessions with
`record_paint` as `crates/fe-terminal-view/tests/view.rs` does.
