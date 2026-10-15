---
task_uid: "01m50ewh3cqh69yjs0scxtgtsm"
task: "T93"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln tests/macro_panel.rs: a #[kiln::panel(title = Notes, dock = Left, toggle_key = secondary-b)] derive(Default) unit struct implementing View shows in a kiln::testing app's left dock with no other wiring, and secondary-b, the palette's Toggle Notes and View \u003e Toggle Notes toggle it, focus leaving it when hidden"
    required: true
    manual: false
  - id: "C2"
    text: "macro_panel.rs: dock = Right and order = n override, and the rail follows order, then file and line"
    required: true
    manual: false
  - id: "C3"
    text: "macro_panel.rs: a panel rendering with cx.watch::\u003cM\u003e() redraws after update_model with no stored Subscription"
    required: true
    manual: false
  - id: "C4"
    text: "macro_panel.rs: #[kiln::status(side = Left)] fn count(cx: \u0026App) -\u003e String shows in the status bar and shows the new text when the bar next draws"
    required: true
    manual: false
---

# Add kiln::View with #[kiln::panel] and #[kiln::status]: a dock panel or status item from one attribute, with its toggle command, key, menu item and rail button (Mac T683)

Filed on argon from Mac T683 (card ~/coordination/argon-cards/T683.json rev 1; note zh). Mac task uid 01m4vbjcjzdhzjp4t66jwf4yhb. Base: argon trunk bb63a81 = mac/main 6ac346f synced. Design record: docs/decisions/kiln-app-layer.md. Argon deps: T92 (Mac T682). Mac T675 rewrites kiln/crates/kiln/examples/host and tests/host.rs and re-lands soon; don't touch those, the spec cites them only as background. Title the host commit "... (Mac T683)".

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 2; design record
`docs/decisions/kiln-app-layer.md`). A dock panel today is a view with `Render`, `Focusable` (a stored
`FocusHandle`), `EventEmitter`s and `DockItem`, a `Dock` it's added to, a toggle command in the keymap and menus,
and focus handling when it hides (`examples/host/panel.rs`, `app.rs:79-108, 264-276`). A status item is pushed by
the host and relayed with `cx.notify()` (`app.rs:405-413`).

## Change

In `kiln/crates/kiln-macros` (new `src/panel.rs`, `src/status.rs`) and `kiln/crates/kiln/src/view.rs`:

- **`kiln::View`**: `fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoElement`, the `Render` of a view kiln
  hosts.
- **`#[kiln::panel(title = "Notes", icon = Icon::Notebook, dock = Left, toggle_key = "secondary-b", order = 0,
  id = "..")]`** on a struct that implements `View`: the macro adds a hidden `__kiln: kiln::__private::ViewState`
  field (it holds the focus handle; `Default`, filled when kiln builds the panel) and implements `Render` (through
  `View`, with `ViewCommands::attach`), `Focusable`, `DockItem` and the events GPUI needs. The struct is built
  from `Default`, or from `new = path` (`fn(&mut Cx<Self>) -> Self`). It contributes the panel (`add_panel`), so
  it shows in its dock with its toggle command, key, View-menu item and rail button, and its open state is in the
  saved layout. `dock = Right` and `order = n` override; ID `<crate>.<snake name>` unless `id`.
- **`#[kiln::status(side = Left, order = 0)]`** on `fn(cx: &App) -> String` (or `Option<String>`, or
  `StatusItem`): a status source pulled when the bar draws. The rustdoc says to keep it O(1).
- Rustdoc on both shows the expansion and the handwritten form (`impl DockItem` plus `Contributions::add_panel`;
  `Workspace::add_status_source`).

New tests go in `kiln/crates/kiln/tests/macro_panel.rs`.

## Implementation notes

- `kiln::View` and the hidden `kiln::__private::ViewState` (a `OnceCell<FocusHandle>`, filled by
  `make_panel` as kiln builds the panel) are in `view.rs`. `ViewState` implements `Clone` (a fresh handle),
  `Debug`, `PartialEq` and `Eq`, so the user's own derives on the struct keep working. A unit struct becomes one
  with the named field `__kiln`, a tuple struct gets it as its last field; a `new` fn fills it with
  `Default::default()`. `#[kiln::panel]` must sit above the struct's derives: a derive above it expands on
  the struct without the field, and the attribute can't see it to say so; the rustdoc says where to put it.
- The generated `Render` wraps `View::render` in a full-size `div` that tracks the focus handle, then attaches
  the view's `ViewCommands` through `kiln::__private::attach!` (nothing if it has none). `DockItem` needs no GPUI
  events, so the macro implements none: "the events GPUI needs" are `Render` and `Focusable`.
- `icon`, `dock` and `side` take a bare variant (`icon = FileText`, `dock = Right`) or any expression of the
  type. Kiln has no `Icon::Notebook` yet, so the tests use existing icons; `icon` defaults to `Icon::Files`,
  `dock` and `side` to `Left`, `order` to `0`. The title falls back to the struct's first doc line.
- The macros submit `PanelRecord`s and `StatusRecord`s with `inventory`, beside `CommandRecord`s:
  `Contributions::add_recorded` adds the app target's and `kiln::module!` a library crate's, by file and line,
  so the stable sort by `order` in `Contributions::panels` puts ties by file and line. The debug-build warning
  of unclaimed library records now names panels and status sources too.
- A status source returns `String`, `&'static str`, `SharedString`, `StatusItem` or an `Option` of any of
  them (`IntoStatus`). It's pulled when the bar draws, and the bar is a cached view that redraws as the
  workspace changes; the rustdoc says so, and C4's test notifies the workspace to make the bar draw again.
- C3's test watches the panel's notifications: GPUI re-renders the dock's active panel on every frame, so a
  render count alone wouldn't show that `cx.watch` asks for a redraw.
- Carried T90 F2: `dispatch_invocation` (kiln-platform `command_actions.rs`, outside the listed scope, since
  only it knows) now records, beside the current invocation, whether it tells the observers itself
  (`dispatch_notifies`). `workspace_handler` reads that during the dispatch instead of checking
  `is_action_available` again once deferred. Regression test: `macro_panel.rs`
  `observers_hear_of_a_command_once_whatever_focus_does_before_its_handler_runs`, with a listener that blurs
  then propagates (notified twice before) and a focus change before the deferred run (not notified before).
- Carried T91 F1: `table_menu_bar` reports a repeated menu name, or a command repeated in a menu, as an error
  worded as the TOML parser words it, and leaves the table out (an empty bar), as a TOML table with errors is;
  `fail_fast` panics on them. Regression tests are in `typed_tables.rs`, and `menus!`'s rustdoc
  (`tables.rs`) says so.
- Review round 1 (F1): `Contributions` keeps each recorded panel's and status source's declaration site beside
  its spec (`PanelSpec` and `StatusSpec` are unchanged), and `panels()`/`statuses()` sort by order, then site,
  those added by hand first in their order, so ties go by file and line whichever module added them. A recorded
  panel's toggle gets `MenuSpec::declared`, so it sits among the View menu's attributed commands by file and line.
  Tests: the unit test `recorded_panels_and_statuses_go_by_file_and_line_whatever_module_adds_them` (two modules
  added in reverse source order) and `Zoom Notes` in C2's test, a View menu command declared between two panels.
