---
task_uid: "01m55cv0xfbxj1bnf22d1r4fhq"
task: "T858"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui unit test view::change_bar_click_tests::lens::a_lens_row_sits_above_its_row passes: rows above a lens draw where they did without it, its row one line height lower, and its items' recorded bounds lie on the row above, starting at the row's indentation; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui unit test view::change_bar_click_tests::lens::clicking_an_item_runs_its_command passes: a click inside the second item's bounds dispatches its Invocation with Source::Mouse and leaves the caret; a click between items runs nothing; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui unit test view::change_bar_click_tests::lens::lenses_follow_the_text_and_folds passes: a row inserted above moves the lens with its row; folding the rows around it hides it; it fails at the base"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-gpui passes with no existing test edited (change_bar_click_tests.rs only gains the #[path] mod line; screenshot goldens unchanged); cargo clippy -p fe-gpui --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Clickable lens rows above editor rows, as VS Code's CodeLens: a provider's items drawn on a row of their own above a line, following edits and folds, a click running the item's command

## Problem

VS Code puts clickable action rows above lines in two day-one places: a language server's CodeLens ("3 references",
"Run") and the merge-conflict actions above each conflict ("Accept Current Change | Accept Incoming Change | Accept
Both Changes | Compare Changes"). fastedit's editor can place rows between rows (`HostBlock`,
`crates/fe-gpui/src/view/host.rs:30`, laid out by fe-display's block layer and painted by
`crates/fe-gpui/src/element/host_blocks.rs`), but only as filler or tinted text a host sets wholesale
(`EditorView::set_blocks`), not clickable and not following edits by themselves; the only clickable in-text
buttons are the AI proposal's Accept / Reject (`view/proposal.rs`, `view.rs:142` `proposal_buttons`). Gap audit
2026-10-17 (user-approved): merge conflicts and LSP completeness first; this is the editor surface both need.

## Change

1. New module `crates/fe-gpui/src/lens.rs` (declared and re-exported in `crates/fe-gpui/src/lib.rs`):
   - `pub struct LensItem { pub title: SharedString, pub invocation: fe_command::Invocation }`,
     `pub struct Lens { pub row: u32, pub items: Vec<LensItem> }` (`row`: the buffer row it sits above);
   - `pub trait LensProvider { fn lenses(&mut self, snapshot: &BufferSnapshot) -> Vec<Lens>; }`, asked each frame for
     the snapshot drawn, so a provider computes from the text (caching by `snapshot.version()`) or returns what a
     host stored, as `DecorationProvider`s do.
2. `crates/fe-gpui/src/view.rs`: `EditorView::add_lens_provider(&mut self, Rc<RefCell<dyn LensProvider>>)` and
   `remove_lens_provider` (by `Rc::ptr_eq`), and `lens_providers()`. Each frame the providers' lenses for one row are
   joined in provider order into one lens row; a row past the end, or inside a fold, has none.
3. Layout and paint (`crates/fe-gpui/src/element.rs`, `crates/fe-gpui/src/element/host_blocks.rs`): each lens row
   is a one-row block above its row's first display row, placed with the host's blocks (a lens above a row that
   also has a host block comes first), so the text below moves down and motions never land on it (blocks are screen
   rows). Its items are drawn from the row's first non-blank column, in the editor font at 0.9 of its size, in the
   muted text colour, separated by ` | `; the item under the pointer is underlined with a pointing-hand cursor.
   The element records each drawn item's bounds on the view, as `proposal_buttons` are recorded.
4. Clicks (`view.rs` `on_mouse_down`, beside `press_proposal_button`): a left click on an item's bounds runs its
   invocation from the view, with `Source::Mouse`, deferred until the listener returns, exactly as a bound click's
   command is dispatched (`view.rs:666-684`); the carets and selections don't move and no drag starts. A click
   elsewhere on a lens row does nothing.
5. Tests: new unit-test file `crates/fe-gpui/src/view/lens_tests.rs`, declared by
   `#[path = "lens_tests.rs"] mod lens;` at the end of the existing `crates/fe-gpui/src/view/change_bar_click_tests.rs`,
   in a headless window as that file's tests are, with a test provider:
   - `a_lens_row_sits_above_its_row`: a lens over row 2 of a five-row buffer: rows 0 and 1 draw where they did
     without it, row 2 one line height lower, and the two items' recorded bounds are on the row above row 2,
     starting at row 2's indentation;
   - `clicking_an_item_runs_its_command`: a click inside the second item's bounds dispatches its invocation (seen
     with `observe_invocations`) from the mouse, and leaves the caret where it was; a click between items runs
     nothing;
   - `lenses_follow_the_text_and_folds`: inserting a row above moves the lens down with its row; folding the rows
     around it hides it.

## Implementation notes

- Lens rows are blocks with ids from `1 << 40` (`host_blocks::LENS_IDS`), chained before the host's blocks in
  `element::block_map`. fe-display's sort by row is stable, so a lens comes first above a row that also has a host
  block, a proposal hunk's block or the ghost text's lines.
- The providers are asked once per frame, in prepaint after the display map has caught up with edits and folds
  (`host_blocks::lenses`); `lens::join` merges their lenses by row in provider order and drops empty rows.
- The indentation an item row starts at is the row's leading whitespace, tabs expanded, as the editor font shapes
  it, so it matches the row's own first non-blank column. Items are in `theme.gutter_foreground`, the editor's muted
  text colour.
- The view keeps the last painted lens rows (`EditorView::lens_rows`, `lens::PaintedRow`): the part of each row in
  view and of each item. `on_mouse_down` checks them beside the proposals' buttons; an item's click is dispatched as
  `RunCommand` from the view's focus handle in `window.defer`, as a bound click's.
- The underline follows `Hitbox::is_hovered` at paint; a mouse-move listener redraws when the pointer moves onto or
  off an item.
- `add_lens_provider` / `remove_lens_provider` take no context, as the spec's signature has it: the host notifies
  the view to show the change.
