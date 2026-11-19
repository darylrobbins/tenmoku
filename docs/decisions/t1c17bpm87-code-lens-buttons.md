# Code lens items as screen-reader buttons (T1964, T1856 F3): design calls

Card: `docs/specs/t1c17bpm87-code-lens-items-as-screen-reader-buttons.md`.

## (a) The change to the accessibility design

`docs/decisions/qenw9xxdzk-accessibility-design.md` says two things this card
reverses. The lead decided this on T1856's review. The design doc is outside this
card's scope, so the change is recorded here:

- **§4 (Editor text area)** lists "code lens … blocks (proposals, lens rows)" as
  *Hidden: no role*. Lens rows are still not text runs: they are blocks outside the
  DisplayMap, and the text area's runs don't change. Each lens **item** in view now
  has a node of its own over the rect it is painted in. Proposals' blocks stay
  hidden, as do the minimap, the git gutter, indent guides and rulers.
- **§10 (Recommended first slice)** lists "code lenses as buttons" as a non-goal.
  It is done now, for every provider: the test lens, a language server's lenses,
  merge conflicts' actions and the memory report's Restart. All of them go through
  fe-gpui's `LensItem`. No provider's file changed, and `LensItem` has no new field.

§7's keyboard row for code lenses is unchanged. Lens items have no Tab stop. The
keyboard reaches them through the palette's "Code Lens: Run…" (A11Y-16a). This
card is about what assistive technology can see and activate.

## How it is built

The breakpoint column's pattern (T1849), in `crates/fe-gpui/src/view/lens_targets.rs`:

- The editor element's prepaint records each lens item in view
  (`host_blocks::lens_targets`). For each item it keeps the row, the item's index,
  the `LensItem`, and the bounds of the pointer hitbox `insert_lens_hitboxes` put
  over it. An item clipped out of view has no hitbox and so no node. A lens row out
  of view isn't laid out, so it has no nodes either.
- A canvas layer is a child of the editor after its text area (and after the
  breakpoint layer). It lays out one absolutely placed div per item, with id
  `EditorView::lens_item_id(row, index)`, which is `("lens-item", row << 32 | index)`.
- **Names are always given**, not only while `window.is_a11y_active()`. A test window
  is never active, and the titles are already `SharedString`s, so naming costs no
  formatting.
- **Name:** the title without a leading decorative glyph and the white space after it
  (`lens::accessible_name`). A glyph is a first character that is neither ASCII nor
  alphanumeric, plus any variation selectors after it. Examples: the test lens's
  "▷ Run test" → "Run test", and rust-analyzer's "▶︎ Run Test" → "Run Test". A title
  that is only a glyph keeps it. No new catalogue string was needed: the name is the
  catalogued title.
- **The pointer is unchanged.** The layer's divs have no listeners, so they have no
  hitbox and take no mouse events. A click still reaches `press_lens` and runs once,
  with the same underline and pointing hand. `lens_tests` and `lens_bounds_tests`
  pass unchanged.

## (b) Plain-text items

A plain-text item has no invocation, for example the test lens's history
("passed in the last 8 runs", "flaky"). It is a `Role::Label` node with its whole
title. It is not a description of the button before it, for three reasons:

- It is read where it shows, after the buttons: "Run test, button", "Debug, button",
  "passed in the last 8 runs".
- The buttons' names stay short, which matters when VoiceOver lists the buttons in
  the rotor.
- A row that starts with plain text has no button before it.

The glyph is not dropped from a label: a ✓ or ✗ in a result would carry meaning.

## The Click, and its Source

A screen reader's Click (`AccessibleAction::Click`, through `fe_ui::on_action`) does
what a pointer press on the item does (`EditorView::activate_lens_item`):

1. It focuses the editor.
2. It ends a composition.
3. It runs the item through the same `EditorView::run_lens` as `press_lens`: a
   `RunCommand` dispatched from the view's focus handle, deferred until the listener
   is done.

The carets don't move.

**Source: `Source::Mouse`, for both paths.** The Click comes from the pointer's
gesture, not from a key, and commands that look at the source should treat the two
the same way (for example `lsp_links`' mouse offsets). `Source::Keyboard` would also
end vim's message, as a key press does (`announced.key()`).

**Context (a fix found on the way).** A provider's invocation names no context.
Run in the editor's frame alone, a host's typed command is not found: merge
conflicts' `git.accept_*`, and an LSP lens's `lsp.run_lens`. `run_lens` now gives
the invocation the view's context stack (`EditorView::context_stack`, the frames
around the editor and its own), as a bound click's invocation gets.

Before this, a *pointer* click on a conflict lens did nothing in the workbench too.
The existing tests dispatched the lens's invocation with a hand-made
`[workbench, editor]` context, so they never caught it.
`tests::lens_a11y::a_pointer_click_on_a_conflict_lens_runs_it` covers the pointer
path now.

## Tests

- `crates/fe-gpui/src/view/lens_a11y_tests.rs` (C1, C2): Buttons and a Label over a
  fixed provider's row, no nodes for the row out of view, one run for a Click and
  one for a pointer click, and the glyph rule.
- `crates/fe-workbench/src/tests/lens_a11y.rs` (C3 to C5):
  - The test lens's Run test, Run tests and Debug, named through the catalogue.
    Debug's Click dispatches `debug.test_at_cursor` for its row, and the debug start
    consumes `tests::adds`.
  - A resolved LSP lens, whose Click sends `workspace/executeCommand`.
  - The four conflict actions. Accept Incoming Change resolves the conflict, from a
    Click and from the pointer.

## (c) Human check: A11Y-11 (T1680)

A line for the VoiceOver pass:

- Over a Rust test with VoiceOver on, VO-Right through the lens row reads
  "Run test, button", then "Debug, button", then the history text if there is one.
  VO-Space on "Run test" runs the test, as a click does. On a merge conflict, VO
  reads "Accept Current Change, button" and the rest, and VO-Space on
  "Accept Incoming Change" resolves it.
