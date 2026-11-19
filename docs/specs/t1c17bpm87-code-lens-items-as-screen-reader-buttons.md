---
task_uid: "01m7rcy2hcq99dgdt1c17bpm87"
task: "T1964"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test view::lens_a11y_tests::a_lens_item_is_a_named_button passes (cargo nextest run -p fe-gpui lens_a11y): every item of a drawn lens row that has an invocation is recorded by fe_ui::nodes as Role::Button, named for its title with a leading decorative glyph such as ▷ dropped, and has a Click in fe_ui::actions. Plain-text items (no invocation) are not buttons. Lens rows out of view have no node"
    required: true
    manual: false
  - id: "C2"
    text: "Test view::lens_a11y_tests::a_screen_readers_click_runs_the_lens_command passes: fe_ui::perform with Click on an item's node dispatches that item's invocation exactly once, from the view, as press_lens does (commands::observe_invocations). A pointer click on the item still runs it exactly once, and lens_tests and lens_bounds_tests pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "Test tests::lens_a11y::the_test_lens_run_and_debug_are_buttons passes (cargo nextest run -p fe-workbench lens_a11y): over a Rust test, the ▷ Run test and Debug items are Buttons named through the catalogue (workbench-lens-run-test, and T1856's Debug key), and a Click on Debug runs T1856's debug invocation for that row"
    required: true
    manual: false
  - id: "C4"
    text: "Test tests::lens_a11y::an_lsp_lens_is_a_button_that_runs_its_command passes: with the fake server (crate::tests::lsp), a resolved code lens's node is a Button named for its title, and a Click on it sends workspace/executeCommand with the lens's command"
    required: true
    manual: false
  - id: "C5"
    text: "Test tests::lens_a11y::merge_conflict_lenses_are_buttons passes: the four conflict items (Accept Current Change, Accept Incoming Change, Accept Both Changes, Compare Changes) are Buttons with those names, and a Click on Accept Incoming Change runs git.accept_incoming for the conflict's row and changes the text"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p fe-gpui --test l10n and cargo nextest run -p fe-workbench --test l10n pass, and cargo clippy -p fe-gpui -p fe-workbench --all-targets -- -D warnings is clean"
    required: true
    manual: false
  - id: "C7"
    text: "The decision file records: (a) the change to docs/decisions/qenw9xxdzk-accessibility-design.md, whose section 4 lists lens rows as Hidden and whose section 10 lists code lenses as buttons as a non-goal; (b) how plain-text items are exposed; (c) a line for the A11Y-11 human pass (T1680): VoiceOver reads a test lens as \"Run test, button\", and VO-Space runs it"
    required: true
    manual: false
---

# Code lens items as screen-reader buttons: each drawn lens item is an AccessKit Button named for its title, whose Click runs its command, for every provider (T1856 F3)

# Code lens items as screen-reader buttons (T1856 F3 follow-up)

## Problem

Review F3 on T1856 (medium) found that the test lens's new Debug item has no
accessibility node. No lens item has one. The ▷ Run test lens (`test_lens.rs`),
LSP code lenses (`lsp_lenses.rs`), merge-conflict lenses (`git_conflicts.rs`) and
the memory report's restart lens (`memory_report.rs`) are all `LensItem` text.
fe-gpui's editor element lays that text out and paints it
(`element/host_blocks.rs`: `lens_layout`, `insert_lens_hitboxes`, `paint_lenses`),
with only a mouse hitbox. `EditorView::press_lens` (`view.rs`) runs it on a click.
A screen reader never sees the items. The only way to them without a mouse is the
palette's "Code Lens: Run…" picker (A11Y-16a). T1856 records F3 as a known limit
that names this card.

## What to build

The fix goes in fe-gpui and covers every provider. No provider's file changes, and
`LensItem` gets no new field: its constructors are in other cards' scopes
(T1856's `test_lens.rs`, T1739's `lsp_lenses.rs`).

- **Follow the breakpoint gutter's pattern** (T1849:
  `view/breakpoints.rs::layout_breakpoint_targets` and `view.rs::breakpoint_targets_layer`):
  - The editor element's prepaint records each lens item's shown rect, the same
    rect `insert_lens_hitboxes` uses, together with its `LensItem`.
  - A canvas layer, a child of the editor after the text area, lays out one
    absolutely placed div per item. Each div has a stable id such as
    `("lens-item", row, index)`.
  - Build the layer in a new `view/lens_targets.rs` (declared from `view.rs`), or
    inline in `view.rs`.
- **The node.** An item with an invocation goes through
  `fe_ui::node(…, Role::Button, name)` and `fe_ui::on_action(…, Action::Click, …)`:
  - The Click runs `RunCommand(invocation)` from the view, deferred as `press_lens`
    does. Pick the `Source` and record the choice.
  - The name is the item's title with a leading decorative glyph (▷) and its space
    dropped.
  - Always name the items. The titles are already `SharedString`s. The breakpoint
    layer names only while `window.is_a11y_active()`, which a test window never
    is, so a test could not read the names.
  - Plain-text items (for example the test history) are not buttons. They are
    either static text nodes or the description of the button before them. Your
    call: record it in the decision file.
- **The pointer is unchanged.** The layer must not take mouse events, so a click
  still reaches `press_lens` and runs once. The underline and the pointing hand
  are unchanged.
- **Keyboard.** Lens items get no Tab stop. Keyboard reach stays the palette's
  "Code Lens: Run…" picker. This card is about what assistive technology can see
  and activate.

This reverses docs/decisions/qenw9xxdzk-accessibility-design.md section 4 (lens rows Hidden) and
section 10 (code lenses as buttons a non-goal): the lead decided it on T1856's review. Record the
change in your decision file; the design doc itself is outside scope.

## Tests (new files)

- `crates/fe-gpui/src/view/lens_a11y_tests.rs`, declared `#[cfg(test)] mod lens_a11y_tests;`
  from `view.rs` (test ids `view::lens_a11y_tests::<test>`).
  - Use a fixed `LensProvider`, as in `lens_tests.rs`.
  - Use kiln-ui's recorder (`fe_ui::nodes`, `fe_ui::actions`, `fe_ui::perform`),
    as `view/breakpoints_tests.rs::a_screen_readers_click_on_a_row_toggles_it`
    does.
- `crates/fe-workbench/src/tests/lens_a11y.rs`, declared from `src/tests/mod.rs`
  (test ids `tests::lens_a11y::<test>`). It covers three providers:
  - the test lens with T1856's Debug item;
  - an LSP lens through `crate::tests::lsp`'s fake server (as `lsp_lenses_tests.rs`);
  - a merge conflict (as `git_conflicts_lens_tests.rs`).
  - Build expected names through the catalogue.

## Not in this card

- Proposal buttons (`ProposalButton`, the same painted-hitbox pattern).
- Inlay hints.
- Lens items as Tab stops.

## Conventions

- Strings go through the catalogue. If a description (for example "line N") needs
  a new string, it goes in `crates/fe-gpui/l10n/en-US/fe-gpui.ftl`.
- Design calls go in your decision file, `docs/decisions/<lane>-*.md`.
- Platform: Linux-OK (headless tests only; no screenshot goldens move).
