---
task_uid: "01m4xevk3hpdmd7gre51qpv8nq"
task: "T112"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "find_replace_tests::replace_next: with the primary selection exactly a match, find.replace_next replaces it and selects the next match; with a selection that is not a match it only selects the next match"
    required: true
    manual: false
  - id: "C2"
    text: "find_replace_tests::replace_all_one_undo: regex on, (\\w+)=(\\d+) replaced with $2=$1 over three rows rewrites every match, and one cmd-z restores the original text"
    required: true
    manual: false
  - id: "C3"
    text: "find_replace_tests::toggles: match case off replaces Foo and foo, whole word skips foobar, regex off treats $1 in the replacement literally"
    required: true
    manual: false
  - id: "C4"
    text: "find_replace_tests::vim: with vim on, opening the bar with the replace row from normal mode, typing a replacement (vim mode unchanged, keys only type), Replace All, then find.close leaves focus in the editor in normal mode, and one vim u undoes every replacement"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
  - id: "C6"
    text: "find_replace_tests (and tests/find_replace_interrupted.rs where the time limit is lowered): a replace during an open vim insert or replace session keeps that session open and makes exactly the requested replacement, as its own undo step. Next-match stepping on an expensive row, a truncated list or several rows never skips a match or wraps wrongly, and an interruption leaves text, selection and vim state unchanged. A replacement containing a lone carriage return is normalized as Buffer::edit does, with no panic."
    required: true
    manual: false
---

# Replace and Replace All in the find bar, honouring its case, word and regex toggles, as one undo step, and vim-friendly (Mac T505)

## Problem

`crates/fe-workbench/src/find_bar.rs` has a query, three toggles and next/previous, but no replace (day-one audit,
2026-10-12; user: "card the day-one ones"). The previous card adds the engine (`fe_vim::search::replace`).

## Change

In `crates/fe-workbench/src/find_bar.rs`:

- A second row under the query, hidden until shown: a `TextInput` placeholder "Replace", a Replace button and a
  Replace All button. A chevron at the bar's left toggles it. New `FindOp`s and commands in `registry()`:
  `find.toggle_replace` "Toggle Replace", `find.show_replace` "Replace…" (opens the bar with the row shown, focus in
  the replace input; the selection seeds the query as `find.show` does), `find.replace_next` "Replace",
  `find.replace_all` "Replace All". The pane already routes every `FindOp` generically (`Pane::run_find`), so
  pane.rs is untouched. Keys and menu items are a later card.
- The replace input's element adds the flag `replace_input` to its key context (beside `text_input`), so the
  keys card can bind Enter there to `find.replace_next`.
- `find.replace_next`: if the primary selection is exactly a match (`Pattern::replacement_at`), replace it with its
  expansion and select the next match after it; otherwise just select the next match, as VS Code does.
- `find.replace_all`: `replace_all` over the active editor's text, applied as one edit batch (one transaction) so
  one cmd-z, or one vim `u`, restores the text; carets end after the last replacement. `Interrupted` changes
  nothing and shows "Search took too long" in the bar's count label. The count label then reads "Replaced N".
- Case, whole-word and regex toggles apply to both, and the replacement is literal unless regex is on.
- Vim: the bar's element sets `find_bar` and the input `text_input`, so with vim on typed keys only type (as the
  find input does today). Opening the bar from vim normal mode and closing it (`find.close`, escape) puts focus
  back in the editor in normal mode, not insert; a replace never puts the editor into insert or visual mode (it leaves visual mode; see Rev 3 for an insert or replace session already open).

## Tests

New file `crates/fe-workbench/src/find_replace_tests.rs`, declared from find_bar.rs
(`#[cfg(test)] #[path = "find_replace_tests.rs"] mod replace_tests;`), reusing `tests/find_bar.rs`'s setup.

## Rev 2 (neon lead respec after review round 3)

The scope now lists the files outside find_bar.rs that rounds 1-2 needed (pane.rs, terminal_find.rs,
tests/mod.rs, tests/find_bar.rs, fe-gpui view/host.rs, fe-buffer buffer.rs,
tests/find_replace_interrupted.rs), for the reasons in the Implementation notes. Fix round 3's two open
findings in this revision:

- **F6 (vim finalization before a replace is certain).** Ending a vim insert or replace session can change the
  text (Esc removes unused indentation, a count puts copies in), and validation can then drop the replacement it
  was meant to make. Example: `o` below an indented line, regex `^ +$`, replacement `-`: Replace All changes text
  and reports "Replaced 0", and Replace Next makes no replacement. A replace must either keep the requested
  replacement through finalization (decide on and anchor the matches after the session ends, against the text as
  Esc leaves it), or change nothing at all. Add regressions for both commands: matching unused indentation, and a
  counted insert whose copies conflict with a match.
- **F8 (expensive rows in next-match stepping).** Don't collect every match in a row before looking at the wanted
  one. Step canonical matches incrementally and stop once the wanted match is known. Share one deadline across
  all rows a navigation visits. Treat a truncated (timed-out) list as an interruption: report "Search took too
  long" and leave the selection and text unchanged, never a skip or a wrong wrap. Add isolated regressions (in the
  interrupted-test binary if they lower the time limit) for an expensive row suffix (`a.*z|a` on a long row of
  `a`s), a truncated list, and a search spanning several rows.

## Rev 3 (neon lead decision after rev 2 review round 2)

Option (c) of the worker's note, decided by the neon lead: **a replace from the find bar leaves an open vim insert
or replace session open.** The replace is its own undo step beside the session (as now), vim stays in insert or
replace mode, and Esc is not run. So next-match planning is exact against the text as the replace leaves it, and
every interruption is atomic: text, selection and vim state are unchanged. Remove the end_typing / Esc-projection
anchoring machinery that rev 2 added for F6, since there is nothing to project. Rev 2's F6 is met by this: no
session-ending edit happens before or after a replace. Rev 2 review round 2's F8 and F10 (planning against Esc's
edits) no longer apply. Keep F12's fix (normalize the expanded replacement as Buffer::edit does). Opening the bar
from normal mode and closing it still returns to normal mode (C4). A replace still leaves visual mode.
C6's vim part now reads: a replace during an open insert or replace session keeps the session open and makes
exactly the requested replacement.

## Implementation notes

- **Files outside the scope, by necessity.** Three small changes the spec's assumptions missed:
  - `pane.rs`: `Pane::run_find` made the bar only on `FindOp::Show`, so a first `find.show_replace` did nothing.
    It now makes it on `Show | ShowReplace` (one line, and its doc).
  - `terminal_find.rs`: its `TerminalFind::run` matches `FindOp` exhaustively, so the four new ops had to be
    added there; the terminal has no replace, so they do nothing, as `find.toggle_whole_word` does.
  - `tests/mod.rs` and `tests/find_bar.rs`: to reuse that file's setup as the spec asks, its module and the
    helpers the new tests call are `pub(crate)`. Only visibility changed.
  - `fe-gpui` `view/host.rs`: a public `EditorView::leave_vim_visual_mode` (the crate's `cancel_vim`, as a click
    does), which a host had no way to do. (Rounds 1-3 also added `leave_vim_typing_mode`; rev 3 removed it.)
  - `fe-buffer` `buffer.rs`: `Buffer::with_transaction_aside` is public (was `pub(crate)`), so a replace is a
    transaction of its own beside an open one of the user's without going through `ProposedChange`, whose
    `locate` is quadratic in insertions (100,000 `$` matches took about 54 s in a debug test, against under 1 s).
  - `tests/find_replace_interrupted.rs`: the Interrupted path lowers the process-wide search time limit, so it
    runs in an integration-test binary of its own, where no other test's search runs meanwhile.
- **Vim's current match.** With vim on, a found match leaves the caret on its start, not the match selected, so
  `find.replace_next` takes the match starting at the caret (as `find_count`'s "current" does with vim on).
- **Carets.** A replace puts the caret after the last replacement; with vim in normal mode, a caret past the end
  of its row goes back onto the row's last character. `find.replace_next` finds the next match from the
  offset after the replacement, so a replacement containing the query isn't found again.
- **The label.** "Replaced N" (or "Search took too long", as an error) shows until the next find command or
  query change. The `Interrupted` path isn't tested: the time limit is process-wide, and lowering it would
  interrupt other tests' searches running in parallel under `cargo test`.
- **Review round 1.** Both replaces catch the model up with edits made through another view of the buffer
  before reading offsets; apply through `ProposedChange::accept_all`, which sets any open transaction of the
  user's aside (vim's insert session, here or in a pane sharing the buffer), so the replace is its own undo step;
  and end a vim insert or replace session first (`find.replace_next` anchors the match across it, as `Esc` moves
  the caret). After an empty match (`$`) is replaced, the next is found past it, not at the same place again.
- **Review round 2.** Replace steps through the matches `replace_all` replaces (`matches_in`, row by row), the
  ones `Pattern::replacement_at` takes, not `Pattern::find_from`'s, so `x*` goes round instead of sticking at a
  row's end, and never inside a grapheme; past a replaced empty match, an empty match at the same place is
  skipped but a non-empty one there is next. Vim: a replace decides what it will replace first, and only then
  ends an insert or replace session (`Esc` may put a count's copies in or take unused indentation out); the
  matches are anchored across that (a non-empty one excluding text put in at its ends) and any whose text
  changed is dropped. A replace that replaces nothing, whether it only selects, finds nothing or runs out of
  time, leaves the text alone, and so leaves a typing session open as `find.next` does; it does leave visual
  mode.
- **Review round 3 (rev 2).**
  - F6: a replace that will replace ends a vim typing session, and the replacements it asked for are all made:
    each match is anchored inward across `Esc` (taking no text put in at its ends, but any put in inside it, and
    empty if its text was taken out), with no validation that could drop it. So unused indentation that matched
    is still replaced (`o`, `^ +$`, `-` gives `-`), and a count's copies put in inside a match go with it. A
    replace that replaces nothing still changes nothing.
  - F8: Replace finds the match at the caret and the next match before changing anything, under one `Deadline`
    for the whole step, stepping each row's matches with `matches_reaching_until` only as far as needed (a
    match reaching the start point, an empty one at it, then the one wanted). A row cut short by the deadline is
    an interruption: "Search took too long", the text and selection as they were. The next match is anchored
    across the replace. `^|b` on `bx`: the regex steps past the `b` after the empty match at the row's start,
    so neither Replace nor Replace All takes it.
  - `Pane::find_label` (`#[doc(hidden)]`) gives the integration-test binary the label.
- **Rev 2, review round 1.**
  - F8: each row is searched with `matches_reaching_until` at a cap of 1, so the search stops at the first match
    reaching the window, the one wanted; if that's one from before the start point reaching past it, or the empty
    one at it, one more search from just past it. The shared deadline is sampled after every search
    (`Deadline::expired`), so a match a call found after the deadline passed is an interruption too. The current
    match's expansion (`Pattern::replacement_at`, whose own deadline can't pass before the shared one) is taken
    only if the shared deadline hasn't passed.
  - F9: an interrupted Replace All leaves vim's mode and selection alone.
  - F10: the next match is found in the text as the replace will leave it (the text with the replacement
    made, `Text::replace`), before anything changes, and placed back through the replace (`Projected`: anchored
    where the replace leaves the text alone, an offset into the replacement where it doesn't). Vim's `Esc` can't
    be foreseen: when ending the session changed the text, the next match is selected only if it's still one,
    else the caret stays after the replacement.
  - F11: in normal mode a caret past the row's end steps back a grapheme with the model's own `Movement::Left`.
  - The time-limit binary's costly row has three matches (`aaa` then 4,000 `b`s) under `(?:\w+\W*){50}z|a`, each
    a measurable call, because the editor's background match count, which the test executor waits for, isn't
    time-limited.
- **Rev 3.** A replace leaves an open vim insert or replace session open, as the lead decided: it's its own undo
  step beside the session (`with_transaction_aside`), and `Esc` isn't run, so the text after it is just the
  replacement's and the next match is planned exactly (`Text::replace` of the normalized replacement). This
  supersedes the session-ending notes of the review rounds above: `end_typing`, its inward anchors and the
  `Projected` mapping of the next match through `Esc` are gone. Moving the caret, as a replace does, drops an
  insert's count, as an arrow key does in vim, so the `Esc` that ends the session later puts in no copies.
- **Read-only editors** are found in but never replaced in.
- `find.show_replace` applies in `editor || find_bar`, so it also shows the row from the query input.
