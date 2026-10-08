---
task_uid: "01m48zkzrn8bmkyy5h0c65ajd2"
task: "T114"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-keymap test tests::click::click_parses_only_with_modifiers: cmd-click and alt-shift-click parse to Key::Click with their modifiers and print back the same; a keymap layer binding click alone, cmd-k click or click a reports one issue per binding and skips it; Keymap::lookup(cmd-click, stack) returns the winning invocation by the matcher's context precedence and None when unbound"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::mouse_binding_tests::bound_click_dispatches_a_command: with cmd-click bound to a test command in the editor context, a cmd single click on a word moves the caret to the clicked offset, starts no drag, and dispatches the command once with source Mouse; a following mouse move with the button held does not extend a selection"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test view::mouse_binding_tests::unbound_clicks_are_unchanged: with no click binding, cmd-click places the caret as a plain click does, shift-click extends the selection, double click selects the word, and clicking a fold toggle folds; no command is dispatched"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test view::mouse_binding_tests::hover_underlines_the_word_under_cmd: moving the pointer over helper with cmd held sets the hover range to helper's word range and the pointing-hand cursor; pressing cmd without moving sets it, releasing cmd clears it; moving onto whitespace, leaving the view, editing or scrolling clears it; with cmd-click unbound in the context no hover range is set. A cmd press while a non-editor (such as a panel) has focus and the pointer is over an editor sets the hover, through the workbench root's modifiers hook (closes F4), with a test. Known limit, accepted by the user 2026-10-08: a view that has never been focused shows no hover, because gpui-pre 0.3.7 keeps Window::context_stack_for_focus_handle private (F6); record it in the hand-in"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::nav_click::cmd_click_goes_to_definition: with the default keymap and src/main.rs open, cmd-click on helper in helper() opens src/util.rs with the selection equal to the name range of fn helper and the editor focused; ctrl-minus returns to main.rs at the clicked offset"
    required: true
    manual: false
  - id: "C6"
    text: "cmd-click is bound to nav.goto_definition in the workbench context in default-macos.toml; no KeyBinding::new or actions! is added; the binding counts in fe-gpui command_tests.rs and keymap_tests.rs are updated and pass; the only fe-workbench source change outside tests is wiring the root element's modifiers-changed hook in workbench.rs (a few lines, user-approved 2026-10-08); fe-keymap label.rs may gain the Click arm; Cargo.toml and Cargo.lock are unchanged"
    required: true
    manual: false
---

# Cmd-click to go to definition: modifier-aware mouse bindings, with a hover underline

# Cmd-click to go to definition: modifier-aware mouse bindings, with a hover underline

Milestone M4 "Navigate". The user asked (2026-10-06) for cmd-click go-to-definition. T107 runs
`nav.goto_definition` from F12 at the newest selection's head. `fe-gpui`'s `EditorView` handles the
mouse directly (`on_mouse_down`: click, double and triple click, shift-click extends) and knows no
modifier bindings. `docs/decisions/command-model.md` keeps mouse input as direct model calls for
now and lists "Mouse input as commands" as not done, with `Source::Mouse` already in
`fe_command::Invocation` for it. This card adds the first piece: a **modified click can be bound
to a command in the keymap**, like a keystroke, and dispatched through the same `RunCommand` path;
the default binds `cmd-click` to `nav.goto_definition`. While cmd is held, the word under the
pointer is underlined.

## What exists (checked 2026-10-06)

- `crates/fe-keymap/src/keystroke.rs`: `Keystroke { modifiers, key }`, `Key` (chars, arrows,
  named keys, `F(n)`), parsed from strings like `cmd-shift-f`.
- `crates/fe-keymap/src/keymap.rs`: `Keymap`, `Binding::{depth, invocation}` against a
  `ContextStack`, `bindings()`, `bindings_for()`; layers from `default-macos.toml` then the user's.
- `crates/fe-gpui/src/keymap.rs`: the keystroke interceptor that feeds one `Matcher` with the
  focused element's context and dispatches matches as `RunCommand(Invocation)`, which propagate from
  the view to the workbench and app registries (`crates/fe-gpui/src/commands.rs`).
- `crates/fe-gpui/src/view.rs`: `on_mouse_down` / `on_mouse_move` / `on_mouse_up`,
  `offset_for_position`, `model.word_range_at`; `CursorStyle::IBeam` on the text area.
- `crates/fe-gpui/src/element.rs`: rows painted from `DecorationRun`s (with an unused `underline`
  field) and selection quads.

## Change

### `crates/fe-keymap`: a click is a bindable key

- `keystroke.rs`: `Key::Click`, spelled `click`, so `cmd-click` parses and prints. Only a primary
  (left) single click.
- `keymap.rs`: a binding whose keys contain `click` is valid only as a single keystroke with at least
  one modifier; anything else (`click` alone, `cmd-k click`, `click a`) is a load issue reported like
  other invalid bindings, and the binding is skipped. Keyboard input never produces `Key::Click`, so
  the `Matcher` needs no change. Add `Keymap::lookup(&self, keystroke: &Keystroke, stack:
  &ContextStack) -> Option<Invocation>`: the binding that would win for that one keystroke in that
  context (same precedence as the matcher), without touching any pending chord.

### `crates/fe-gpui`: dispatch a bound click; underline the word while the modifier is held

- `keymap.rs`: a `pub(crate)` helper that resolves a `Keystroke` against the active keymap and a
  view's context stack, through `Keymap::lookup`.
- `view.rs`:
  - **Click.** On a left mouse-down with `click_count == 1` and modifiers whose `<mods>-click`
    keystroke resolves to a command in the view's context: set the caret at the clicked offset (no
    drag starts, no selection extends), then dispatch `RunCommand` with that `Invocation`,
    `source: Mouse`, from the view so it propagates like a keystroke's. When nothing is bound, the
    click behaves exactly as today (shift-click still extends; the fold gutter still toggles).
  - **Hover.** While the pointer is over text and the held modifiers make `<mods>-click` resolve
    to a command, the word under the pointer (`word_range_at`, empty ranges ignored) is the hover
    range: drawn underlined and with `CursorStyle::PointingHand`. It updates on mouse move and on
    GPUI's modifiers-changed event (pressing or releasing cmd without moving), and clears on mouse
    exit, focus loss, an edit, or a scroll. No navigation query runs on hover.
- `element.rs`: paint the hover range's underline (a 1 px line in the text colour under each row
  piece of the range, or the `DecorationRun` underline; implementer's choice), only for visible
  rows.

### Default binding

`crates/fe-keymap/keymaps/default-macos.toml`: `cmd-click` → `nav.goto_definition` in the
`workbench` context (T107's F12 command), so a bare `EditorView` outside the workbench shows no
underline. Update the binding counts pinned in `crates/fe-gpui/src/command_tests.rs` and
`crates/fe-gpui/src/keymap_tests.rs`.

No `fe-workbench` source changes: `nav.goto_definition` already reads the caret, which the click
has just moved. As a consequence, back (`ctrl-minus`) after a cmd-click returns to the clicked
word, not to where the caret was before the click (VS Code behaves the same).

### Tests (new files)

- `crates/fe-keymap/src/tests/click.rs`, declared in `crates/fe-keymap/src/tests/mod.rs`.
- `crates/fe-gpui/src/view/mouse_binding_tests.rs`, declared in `view.rs` like `reveal_tests`:
  headless GPUI with a test keymap layer binding `cmd-click` to a test command in the `editor`
  context, mouse events simulated at laid-out positions.
- `crates/fe-workbench/src/tests/nav_click.rs`, declared in `crates/fe-workbench/src/tests/mod.rs`:
  end to end with the default keymap.

## Out of scope

- Other mouse buttons, double-click bindings, drag bindings, middle-click, alt-click multi-cursor.
- Peek-on-hover, hover cards, underlining only words that have a definition.
- Moving existing mouse handling (selection, folds) onto commands.
- Linux/Windows keymaps (`ctrl-click`), which arrive with those keymaps.

## Decided (lead defaults; the user may revisit)

- **A click is a `Key` in `fe-keymap`** (`cmd-click`), so it is rebindable in the user's
  `keymap.toml`, contexts and overrides apply, and the binding report covers it, rather than a
  separate mouse-binding table.
- **Underline the word, not a resolved definition**: hover never queries `fe-nav`, so it costs no
  background work; a click with no definition shows T107's "No definition found" picker.
- `cmd-click` sets the caret and then runs the command; it never extends a selection.
