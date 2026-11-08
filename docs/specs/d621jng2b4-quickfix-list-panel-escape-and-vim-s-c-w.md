---
task_uid: "01m6vd058asgwj6nd621jng2b4"
task: "T516"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "dock_focus::dock_leave::dock_leave_tests::escape_and_ctrl_w_leave_the_quickfix_panel: with vim on and a three-item quickfix list, quickfix.show focuses the panel; escape, ctrl-w h, ctrl-w p and bare ctrl-h (sidebar on the right) each focus the active editor; the list's current item is unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "dock_focus::dock_leave::dock_leave_tests::escape_leaves_an_empty_quickfix_panel: with vim on and no quickfix list, escape leaves the panel shown by quickfix.show for the editor; with an empty location list, the same for the panel shown by location.show. With no location list at all, location.show still reports E776: No location list and shows nothing, as nvim's :lopen does; that behaviour is unchanged and is not this test's to cover"
    required: true
    manual: false
  - id: "C3"
    text: "all fifteen dock-navigation bindings in fastedit.toml (2413-2486 at the base) include quickfix_panel in their context; no binding is added or removed; dock_leave.rs's focused_dock includes the quickfix panel's focus handle"
    required: true
    manual: false
  - id: "C4"
    text: "crates/fe-keymap/tests/dock_leave_keys.rs LISTS includes quickfix_panel and its macos and linux tests pass; fe-gpui command_tests, keymap_tests and vim::tests pass with the new context strings"
    required: true
    manual: false
  - id: "C5"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-workbench -p fe-keymap -p fe-gpui pass"
    required: true
    manual: false
  - id: "C6"
    text: "Known limit (review F1): Escape bound for quickfix_panel no longer reaches QuickfixPanel::on_key_down, so a pending g survives leaving the panel and refocusing it directly, and a later g completes gg. The fix is a focus-out clear in quickfix_panel.rs, which this card must not touch; follow-up T517 (which owns that file and depends on this card) clears it and adds the regression a_pending_g_does_not_survive_leaving_the_panel"
    required: true
    manual: false
---

# Quickfix List panel: Escape and vim's <C-w>/<C-h..l> keys leave it for the editor, as from References and Problems (Mac T1406 F1)

Carried medium from T492 F1 (Mac T1406), filed at the Mac lead's request.

# Quickfix List panel: Escape and vim's window keys leave it, as from References

## Problem

T492's review (F1, medium, at `crates/fe-workbench/src/quickfix_panel.rs:198`): the Quickfix List panel sets
the key context `quickfix_panel` (`quickfix_panel.rs:54`, `:198-205`, with the `vim` flag while vim is on),
but that context is in none of the dock-navigation bindings, and the panel isn't among the views
`dock_leave.rs` checks for focus:

- `crates/fe-keymap/keymaps/fastedit.toml:2413-2486`: `ctrl-w h/j/k/l` (and `ctrl-w ctrl-h/j/k/l`),
  `ctrl-w p`, `ctrl-w w`, bare `ctrl-h/j/k/l` and `escape` are bound only in
  `(project_panel || outline_panel || ReferencesPanel || problems_panel || search_results || git_panel [|| terminal_panel]) && vim && !editor && !text_input`.
- `crates/fe-workbench/src/dock_leave.rs:84-102`, `focused_dock`, lists the project tree, outline, search,
  Changes, References, Problems, history, git log and chat focus handles, not the quickfix panel's, so even a
  bound `dock.focus_left` from it would do nothing.

So after `:copen` focuses the panel (it is `focus_on_show`, `quickfix_panel.rs:42-51`), Escape and `<C-w>`
can't return to the editor. An empty panel (no list) is worse: it has no Enter landing to leave through
either.

## Change

- `crates/fe-keymap/keymaps/fastedit.toml`: add `quickfix_panel` to the context of all fifteen dock-navigation
  bindings at `:2413-2486` (the ten `ctrl-w` ones and the five bare `ctrl-h/j/k/l`/`escape` ones), beside
  `problems_panel`. No binding is added or removed, so the binding counts don't change.
- `crates/fe-workbench/src/dock_leave.rs`: chain `sidebar.quickfix()`'s focus handle into `focused_dock`
  (as `history()`/`git_log()`/`chat()` are, since the panel exists only once shown: `dock.rs:547`), and name
  the Quickfix List among the lists in the module doc.
- The three fe-gpui tests that match these exact context strings must list the new ones:
  `crates/fe-gpui/src/command_tests.rs:106-107`, `crates/fe-gpui/src/keymap_tests.rs:426-427`,
  `crates/fe-gpui/src/vim/tests.rs:469-470`.
- `crates/fe-keymap/tests/dock_leave_keys.rs:42-49`: add `&["quickfix_panel"]` to `LISTS`, so its `macos`
  and `linux` tests check every key there, with vim on and off.
- The panel's own keys (`quickfix_panel.rs:207-232`: Up/Down, Enter, and with vim `j`/`k`/`o`/`gg`/`G`) are
  unchanged; it handles no escape or ctrl key itself, so nothing there needs to change.

## Tests

Add to `crates/fe-workbench/src/dock_leave_tests.rs` (declared from `dock_leave.rs:147-149`, which
`dock_focus.rs:14-15` declares; full path `dock_focus::dock_leave::dock_leave_tests::…`). Fill a list with
`Workbench::push_quickfix_list` (`quickfix_commands.rs:96`), as `quickfix_panel_tests.rs:106` does.

- `escape_and_ctrl_w_leave_the_quickfix_panel` (the regression test): with vim on and a three-item quickfix
  list, `quickfix.show` (or `:copen`) focuses the panel; `escape` focuses the active editor. Show it again:
  `ctrl-w h` (toward the panes: the sidebar is on the right by default, as `dock_leave_tests.rs:167-172`
  shows) focuses the editor; again, `ctrl-w p` does; again, bare `ctrl-h` does. The quickfix list's current item is unchanged by each.
- `escape_leaves_an_empty_quickfix_panel`: with vim on and no quickfix list (the header says
  "No quickfix list"), `quickfix.show` focuses the panel and `escape` focuses the editor; the same for
  `location.show` with no location list.

## Not in scope

- The panel's stale-row click (T492 F2): its own card, which depends on this one.
- Escape with vim off: like the other lists, the dock keys are vim-only.
- `quickfix_panel.rs` itself: no change is needed there; if one turns out to be, block rather than widen the
  scope (the F2 card owns that file next).
