---
task_uid: "01m50ewh5h0v1r3fxydzr5g6ed"
task: "T94"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln tests/macro_item.rs: open::\u003cT\u003e(state) opens a tab, and opening an equal state again activates that tab instead of adding one"
    required: true
    manual: false
  - id: "C2"
    text: "macro_item.rs: the tab's title comes from Item::title and follows a model change on the next draw"
    required: true
    manual: false
  - id: "C3"
    text: "macro_item.rs: the item restores from a saved layout through its typed state with no register call; the kind defaults to CRATE.SNAKE_NAME and kind = .. keeps an existing one"
    required: true
    manual: false
  - id: "C4"
    text: "macro_item.rs: active_state::\u003cT\u003e() gives the active item's state, None when the active item is another type, and a #[kiln::command(context = ..)] impl on the item is attached"
    required: true
    manual: false
---

# Add kiln::Item and #[kiln::item]: a tab as a function of its typed state, restored by kind, with workspace open and active_state (Mac T684)

Filed on argon from Mac T684 (card ~/coordination/argon-cards/T684.json rev 1; note zh). Mac task uid 01m4vbjd82f3nq77yrbwqpb73f. Base: argon trunk bb63a81 = mac/main 6ac346f synced. Design record: docs/decisions/kiln-app-layer.md. Argon deps: T93 (Mac T683). Mac T675 rewrites kiln/crates/kiln/examples/host and tests/host.rs and re-lands soon; don't touch those, the spec cites them only as background. Title the host commit "... (Mac T684)".

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 2; design record
`docs/decisions/kiln-app-layer.md`). A pane item today implements `PaneItem` (title, serialize, restore,
apply_state over `Value`), is registered for restore by hand (`layout::register_item::<Conversation>`,
`examples/host/main.rs:31`), and "open this or activate its tab" is written per app (`examples/host/app.rs`
`open`, ~187-211). The approved design: *a tab is a function of its saved state*: `new` both opens and
restores it, and `open` activates an existing tab with an equal state instead.

## Change

In `kiln/crates/kiln-macros` (new `src/item.rs`) and a new `kiln/crates/kiln/src/item.rs`:

- **`kiln::Item`**: `type State: Serialize + DeserializeOwned + PartialEq + Clone`; `fn new(state, cx: &mut
  Cx<Self>) -> Option<Self>`; `fn state(&self) -> Self::State`; `fn title(&self, cx: &App) -> SharedString`;
  `fn focus(&self, cx: &App) -> FocusHandle`; with `is_dirty`, `icon` and `status_items` defaulted. Async restore
  stays a handwritten `PaneItem`.
- **`#[kiln::item(kind = "..")]`** on a struct that implements `Item` and `View`: implements `Render`
  (through `View` and `ViewCommands::attach`), `PaneItem` (title, the typed state through
  `kiln_shell::item::ItemState` and its three forwards) and contributes the item kind for restore. Kind
  `<crate>.<snake name>` unless `kind` (to keep an existing one, such as `chat.conversation`).
- **On the workspace** (an extension trait in `item.rs`): `open::<T: Item>(state, window, cx)` activates the tab
  in the active pane whose state equals `state`, else makes one with `T::new` (nothing if it returns `None`);
  `active_state::<T>() -> Option<T::State>`.
- Rustdoc shows the expansion and the handwritten form (`PaneItem` plus `ItemState` plus `register_item`).

New tests go in `kiln/crates/kiln/tests/macro_item.rs`.

## Implementation notes

- `Item::new` is `fn new(state, window: &mut Window, cx: &mut App) -> Option<Self>`, not `cx: &mut Cx<Self>`.
  GPUI hands out a `Context<T>` only inside `cx.new`/`insert_entity`, whose closure must return a `T`, so a
  constructor given a `Cx<Self>` can't decline. kiln makes the entity from the value `new` returns. An item
  watches models as it renders (`Cx::watch`), which is what makes its tab title follow a model (C2).
- `WorkspaceExt` (also in `kiln::prelude`) is implemented for `Workspace<P>` of any pane; `open` returns the
  item shown, `Option<Entity<T>>`. Both calls bound `T: Item + PaneItem`, which `#[kiln::item]` gives.
- `#[kiln::item]` adds no hidden field: the item keeps its own focus handle, as `Item::focus` requires.
- The kind's record is `ItemRecord` in `contributions.rs`, beside `PanelRecord` and `StatusRecord`, added by
  `Contributions::add_recorded` and `kiln::module!` through `Contributions::add_item_kind`, and warned of
  when unclaimed. `View`'s "not implemented" label in `view.rs` now names `#[kiln::item]` too. Both files are
  outside the listed scope.
