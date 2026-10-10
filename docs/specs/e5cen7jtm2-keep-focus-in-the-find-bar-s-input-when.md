---
task_uid: "01m4kcax7yvrpe7pe5cen7jtm2"
task: "T2"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::find_bar::toggle_focus::clicking_the_regex_toggle_keeps_typing_in_the_input: with the bar open on query foo, a left click at (pane-find-bar bounds right - 12px, centre y) turns regex on, leaves the input focused, simulate_input extends the query, and find.close then closes the bar; it fails at the task base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::find_bar::toggle_focus::each_toggle_click_keeps_focus_where_it_was: clicks at the centres of debug bounds find-toggle:0, :1 and :2 flip match_case, whole_word and regex; after each, the input stays focused, typing extends the query, and find.next steps the editor's selection; with the editor focused and the bar open, a toggle click leaves the editor focused"
    required: true
    manual: false
  - id: "C3"
    text: "Only crates/fe-workbench/src/find_bar.rs (FindBar::render), crates/fe-workbench/src/tests/find_bar.rs (only an added mod toggle_focus; line), the new crates/fe-workbench/src/tests/find_bar/toggle_focus.rs and this task's docs/specs/\u003ckey\u003e-*.md change; the existing fe-workbench tests pass unchanged"
    required: true
    manual: false
---

# Keep focus in the find bar's input when a Match Case, Whole Word or Regex toggle is clicked

## Problem

Review of T265 (landed at d144b7b; review 01m4jnxg2rnqs4fsfr9nc0wck7, finding F1, medium,
`crates/fe-workbench/src/find_bar.rs:341`), auto-filed as T321:

> Clicking any of the three toggles steals focus from the input: ToggleButton does not suppress mouse-down
> focus transfer, so the enclosing Pane's tracked focus handle receives focus. run_find applies the toggle
> without restoring input focus. Subsequent typing no longer changes the query, and find.next, find.previous
> and find.close fail their context predicates. The tests dispatch toggle commands directly and miss this
> mouse path.

The mechanism: `Pane::render` (`crates/fe-workbench/src/pane.rs`) puts `.track_focus(&self.focus)` on the
pane's root. GPUI (`gpui-pre` 0.3.7, `src/elements/div.rs`, the "automatically transfer focus" listener)
focuses a tracked handle on a left mouse-down in its hitbox at the bubble phase unless
`window.default_prevented()`. The toggles in `FindBar::render` (`crates/fe-workbench/src/find_bar.rs`, the
`toggles` built from `fe_ui::ToggleButton`) aren't focusable and don't prevent default, so the click reaches
the pane's listener and focus leaves the input (key context `find_bar`/`text_input`) for the pane.

## Change

`crates/fe-workbench/src/find_bar.rs`, `FindBar::render` only:

- The toggles' container (`div().flex().flex_none().gap_0p5().children(toggles)`) gets
  `.on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())`. Its listener runs before the
  pane's (children's bubble listeners run first), so the pane's focus transfer is skipped and focus stays
  where it was: in the input when typing there, in the editor if that's where it was. Click listeners aren't
  gated on `default_prevented`, so each toggle's `on_click` still runs `Pane::run_find`.
- Each toggle is wrapped in `div().debug_selector(|| format!("find-toggle:{ix}"))` (0 Match Case, 1 Whole
  Word, 2 Regex) so tests can click it.

No change to `fe-ui`'s `ToggleButton`, `pane.rs`, the search panel, or `run_find`.

## Tests (new file)

`crates/fe-workbench/src/tests/find_bar/toggle_focus.rs`, declared by one line, `mod toggle_focus;`, added to
`crates/fe-workbench/src/tests/find_bar.rs` (the `toolbar_history.rs` → `toolbar_history/goto.rs` pattern).
It reuses that file's private helpers through `super::` (`opened`, `pane`, `editor`, `run`, `draw`, `query`,
`input_focused`, `editor_focused`, `open`, `select`, `selection`) and adds no item outside the new file, so
it compiles against the base API when the finish gate overlays it on the base.

- `clicking_the_regex_toggle_keeps_typing_in_the_input` (the regression test): locates the regex toggle
  without the new selectors, from `cx.debug_bounds("pane-find-bar")` (present at base): the toggle is the
  bar's last child, at least 22px wide, ending at the bar's 8px right padding, so a click at
  `(bar.right() - px(12.), bar.center().y)` hits it. Fails at base on the focus assertion.
- `each_toggle_click_keeps_focus_where_it_was`: uses `find-toggle:{0,1,2}` (fails at base only because the
  selectors are new, so it's not on the regression list).

## Out of scope

Clicks elsewhere in the bar (the count label, padding), keyboard toggles (T266), and the running-app check
(the human card).


_Moved from the Mac board (T322) to run on neon; it lands back on main through the lead._
