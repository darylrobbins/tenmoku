---
task_uid: "01m4vbj0fjz9a3krzm9zp2qffy"
task: "T668"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-shell tests/item_state.rs: an item implementing ItemState with a derive(Serialize, Deserialize) state, whose PaneItem methods are the three one-line forwards, round-trips through layout::save_panes and restore_panes with its state intact"
    required: true
    manual: false
  - id: "C2"
    text: "item_state.rs: a saved state that does not decode drops that item from the restored layout with a warning and no panic, and the other items restore"
    required: true
    manual: false
  - id: "C3"
    text: "item_state.rs: ItemState::apply reaches a copy made by PaneItem::split while restoring"
    required: true
    manual: false
  - id: "C4"
    text: "Value-based PaneItem implementations are unchanged and existing kiln-shell tests pass unedited"
    required: true
    manual: false
---

# Let a kiln-shell pane item keep a typed serde state instead of hand-encoded Value, beside the Value form

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 1; design record
`docs/decisions/kiln-app-layer.md`, from the docs card, which may not be on main yet).

A pane item's saved state is hand-encoded `serde_json::Value`: `PaneItem::serialize`, `restore` and
`apply_state` (`kiln/crates/kiln-shell/src/item.rs:153-166`), as in the host example's
`examples/host/conversation.rs:226-251` and `threads.rs:91-97`:

```rust
let (Some(id), Some(title)) = (thread["id"].as_u64(), thread["title"].as_str()) else { continue };
```

where a `#[derive(Serialize, Deserialize)]` type would do. Rust has no stable associated-type defaults, so
`PaneItem` can't gain a `type State = Value` without breaking every implementor.

## Change

Add typed state beside the `Value` form, which stays the default and is unchanged.

- New `kiln/crates/kiln-shell/src/item/state.rs`, declared from `item.rs` (`pub mod state;` plus
  `pub use state::ItemState;`), so `kiln-shell/src/lib.rs` isn't touched:

  ```rust
  pub trait ItemState: PaneItem {
      type State: Serialize + DeserializeOwned;
      /// `None` leaves the item out of the saved layout.
      fn state(&self, cx: &App) -> Option<Self::State>;
      fn from_state(state: Self::State, window: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>>;
      /// The per-view part (a caret) for a copy `PaneItem::split` made while restoring.
      fn apply(&mut self, _state: &Self::State, _window: &mut Window, _cx: &mut Context<Self>) {}
  }
  ```

- Three forwarding fns in the same module, which an item's `PaneItem` methods call in one line each:
  `state::serialize(self, cx)`, `state::restore::<Self>(state, window, cx)` and
  `state::apply(self, state, window, cx)`. A state that doesn't decode is logged with `log::warn!` (naming the
  item's kind and the serde error) and restores as `None`, so the layout drops that item and keeps the rest.
  Serde through `kiln_core::serde`; add `serde_json.workspace = true` to `kiln-shell/Cargo.toml` for
  `to_value`/`from_value`.
- Rustdoc on `ItemState` shows the three one-line forwards; the phase-2 `#[kiln::item]` macro will write them.

New tests go in `kiln/crates/kiln-shell/tests/item_state.rs` (its own test crate; reuse `tests/support/mod.rs`
with `mod support;` if helpful, without editing it).
