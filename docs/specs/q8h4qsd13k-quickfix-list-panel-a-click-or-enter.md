---
task_uid: "01m6vd068cwc51rmq8h4qsd13k"
task: "T517"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "dock::quickfix_panel::quickfix_panel_tests::a_click_across_a_list_replacement_opens_nothing: mouse-down on row 1 of list A, list B pushed and drawn, mouse-up at the same point: nothing opens and B's current item stays 0; a fresh click on row 1 then opens B's item 1"
    required: true
    manual: false
  - id: "C2"
    text: "dock::quickfix_panel::quickfix_panel_tests::a_row_opened_then_replaced_before_it_runs_opens_nothing: Enter on row 1 then a push in the same update, before the deferred open runs, opens nothing and leaves B's current item at 0; the same for a pane's location list"
    required: true
    manual: false
  - id: "C3"
    text: "dock::quickfix_panel::quickfix_panel_tests::a_click_across_cnext_still_opens_its_row: mouse-down on row 2, :cnext, mouse-up opens row 2's item and makes it current"
    required: true
    manual: false
  - id: "C4"
    text: "dock::quickfix_panel::quickfix_panel_tests::a_pending_g_does_not_survive_leaving_the_panel: with vim on, a three-item list with current item 1 and the panel focused, g then Escape (the editor has focus), then focusing the panel directly (not through quickfix.show) and g leaves the selection on row 1; gg then still goes to row 0. The pending prefix is cleared on focus-out, not only on Escape"
    required: true
    manual: false
  - id: "C5"
    text: "row element IDs carry the shown list's identity, which changes on a push, :colder/:cnewer and a switch between the quickfix and a location list but not when the current item moves; the existing quickfix_panel_tests, quickfix_tests and quickfix_commands_tests pass"
    required: true
    manual: false
  - id: "C6"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-workbench pass"
    required: true
    manual: false
---

# Quickfix List panel: a click or Enter across a list replacement (:grep, :make, :colder) opens nothing, not the new list's row at that index (Mac T1406 F2)

Carried medium from T492 F2 (Mac T1406), filed at the Mac lead's request.

# Quickfix List panel: a click or Enter across a list replacement opens nothing, not the new list's row

## Problem

T492's review (F2, medium, at `crates/fe-workbench/src/quickfix_panel.rs:286`): a row's element ID is only
its index, `ElementId::NamedInteger("quickfix-row", ix)` (`quickfix_panel.rs:272`, `:286`), and its click
handler (`:302`) and Enter/`o` (`:222`, `:225`) call `QuickfixPanel::open` (`:182-194`), which defers
`Workbench::open_quickfix_row(source, ix, …)` (`quickfix_commands.rs:315-351`). That resolves `ix` against
whatever list is current when the deferred call runs (`quickfix_commands.rs:327`).

The lists change under the panel asynchronously: `:grep`, `:vimgrep` and `:make` push a new list when their
run ends (`quickfix_fill.rs:282`, `:364`, through `Workbench::push_quickfix_list_to`,
`quickfix_commands.rs:111-129`), and `:colder`/`:cnewer` switch the shown one (`quickfix.rs:321`, `:331`). So:

- a list replaced between mouse-down and mouse-up keeps the same row IDs, so the click lands, on the new
  list's item at that index;
- a list replaced after the click or Enter but before the deferred call runs (`quickfix_panel.rs:188`) opens
  the new list's item too.

The References panel guards against exactly this (`references_panel.rs:405-415`: each run has a
`generation`, carried by its rows and checked in `open`); this panel doesn't.

## Change

- Give the list the panel shows an identity that changes whenever a different list is shown in its place:
  a push (`QuickfixStack::push`, `quickfix.rs:290`; `push_location`, `:395`), `:colder`/`:cnewer`
  (`quickfix.rs:321`, `:331`), a location list's pane going, and the panel switching between the quickfix
  list and a location list (`QuickfixPanel::show_list`, `quickfix_panel.rs:131`). Moving the current item
  (`:cnext`, `:cc N`, a landing) is not a different list and must not change it. A revision on
  `QuickfixStack` (bumped on push/older/newer) plus the panel's `source` is one way; a per-list ID is
  another. Choose one; mind that `QuickfixStack` and `QuickfixList` derive `PartialEq`
  (`quickfix.rs:102`, `:280`), which `quickfix_tests.rs` uses.
- Put that identity in each row's element ID (`quickfix_panel.rs:286`), so a mouse-down on one list's row
  and a mouse-up on the next list's row are different elements and no click fires.
- Capture the identity when a row is activated (click, Enter, `o`) and check it when the deferred call runs:
  `open_quickfix_row` (or the deferred closure) does nothing if the shown list is no longer that one. Nothing
  opens, no message shows, and the new list's current item stays where it was.

## Tests

Add to `crates/fe-workbench/src/quickfix_panel_tests.rs` (full path
`dock::quickfix_panel::quickfix_panel_tests::…`; it is declared from `quickfix_panel.rs:427-429`, which
`dock.rs:51-52` declares). Use only APIs the base has (`push`, `three`, `landed`, `panel`, `drawn` there,
`Workbench::push_quickfix_list`), so the regression tests compile at the base. For mouse-down and mouse-up
across a change, see `git_log_open_tests.rs:479-490`.

- `a_click_across_a_list_replacement_opens_nothing` (regression): list A shown and drawn; mouse-down on row 1;
  list B pushed (a different three items) and drawn; mouse-up at the same point. Nothing opens (the active
  editor and caret are unchanged), and B's current item is still 0. A fresh click on row 1 then opens B's item
  1.
- `a_row_opened_then_replaced_before_it_runs_opens_nothing` (regression): in one update, Enter on row 1 of list
  A, then list B pushed, before the deferred open runs; nothing opens and B's current item is still 0. The
  same with a location list (`location.show`, pushing to the pane's location list).
- `a_click_across_cnext_still_opens_its_row`: mouse-down on row 2, `:cnext` (current moves to 1), mouse-up:
  row 2's item opens and becomes current. Moving the current item isn't a replacement.

## Also: a pending `g` must not survive leaving the panel (T516 F1)

T516 (Escape and `<C-w>` leave the panel) binds Escape in `fastedit.toml` for the `quickfix_panel` context, so
Escape no longer reaches `QuickfixPanel::on_key_down`, which used to clear `pending_g`. The panel has no
focus-out cleanup, so `g`, Escape (focus goes to the editor), then focusing the panel again directly (not through
`quickfix.show`, which resets the selection) and `g` completes `gg` and jumps to the first row. T516's review
found it (F1, medium) and left it here because this card owns `quickfix_panel.rs`.

- Clear the panel's pending prefix when it loses focus (`on_focus_out`, or the blur path the panel already has),
  so any way of leaving drops it, not only Escape. A test that relies on focus events needs
  `window.activate_window()` first: GPUI test windows deliver no focus changes until activated.
- Test: `a_pending_g_does_not_survive_leaving_the_panel` (regression): vim on, a three-item list with current
  item 1, panel focused; `g`, then Escape (the editor has focus), then focus the panel directly by its focus
  handle or a dock-focus command, not `quickfix.show`; `g` leaves the selection on row 1. `gg` then still goes
  to row 0.

## Not in scope

- Escape and `<C-w>` leaving the panel themselves (T492 F1): the card this one depends on (T516).
- A list grown in place while a `:grep` streams (if any fill appends to the shown list rather than pushing):
  note it in the spec if found, but the finding is about replacement.
