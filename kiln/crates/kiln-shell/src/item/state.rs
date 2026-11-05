// SPDX-License-Identifier: Apache-2.0

//! A typed saved state for a pane item, beside the [`Value`] form of
//! [`PaneItem::serialize`], [`PaneItem::restore`] and
//! [`PaneItem::apply_state`], which stays the default.
//!
//! An item implements [`ItemState`] for a `#[derive(Serialize,
//! Deserialize)]` state type, and forwards its three `PaneItem` methods in
//! one line each:
//!
//! ```ignore
//! impl ItemState for Notes {
//!     type State = NoteState;
//!
//!     fn state(&self, _: &App) -> Option<NoteState> {
//!         Some(NoteState { title: self.title.to_string(), caret: self.caret })
//!     }
//!
//!     fn from_state(state: NoteState, _: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
//!         Task::ready(Some(cx.new(|cx| Notes::at(state, cx))))
//!     }
//!
//!     fn apply(&mut self, state: &NoteState, _: &mut Window, _: &mut Context<Self>) {
//!         self.caret = state.caret;
//!     }
//! }
//!
//! impl PaneItem for Notes {
//!     const KIND: &'static str = "notes.note";
//!
//!     fn tab_title(&self, _: &App) -> SharedString {
//!         self.title.clone()
//!     }
//!
//!     fn serialize(&self, cx: &App) -> Option<Value> {
//!         state::serialize(self, cx)
//!     }
//!
//!     fn restore(state: Value, window: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
//!         state::restore::<Self>(state, window, cx)
//!     }
//!
//!     fn apply_state(&mut self, state: &Value, window: &mut Window, cx: &mut Context<Self>) {
//!         state::apply(self, state, window, cx)
//!     }
//! }
//! ```

use gpui::{App, Context, Entity, Task, Window};
use kiln_command::Value;
use kiln_core::serde::{Serialize, de::DeserializeOwned};

use super::PaneItem;

/// A pane item whose saved state is a serde type.
pub trait ItemState: PaneItem {
    type State: Serialize + DeserializeOwned;

    /// Its state in a saved layout. `None` leaves it out of the layout.
    fn state(&self, cx: &App) -> Option<Self::State>;

    /// Rebuilds an item from its saved state, possibly after reading a
    /// file. `None` drops the item from the restored layout.
    fn from_state(
        state: Self::State,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<Option<Entity<Self>>>;

    /// The per-view part of a saved state, such as a caret, for a copy
    /// [`PaneItem::split`] made while restoring.
    fn apply(&mut self, _state: &Self::State, _window: &mut Window, _cx: &mut Context<Self>) {}
}

/// [`PaneItem::serialize`] for an [`ItemState`] item.
pub fn serialize<T: ItemState>(item: &T, cx: &App) -> Option<Value> {
    let state = item.state(cx)?;
    match serde_json::to_value(state) {
        Ok(value) => Some(value),
        Err(error) => {
            log::warn!("not saving a {} item: {error}", T::KIND);
            None
        }
    }
}

/// [`PaneItem::restore`] for an [`ItemState`] item. A state that doesn't
/// decode is logged and restores as `None`, so the layout drops the item.
pub fn restore<T: ItemState>(
    state: Value,
    window: &mut Window,
    cx: &mut App,
) -> Task<Option<Entity<T>>> {
    match decode::<T>(state) {
        Some(state) => T::from_state(state, window, cx),
        None => Task::ready(None),
    }
}

/// [`PaneItem::apply_state`] for an [`ItemState`] item. A state that
/// doesn't decode is logged and leaves the item as it is.
pub fn apply<T: ItemState>(item: &mut T, state: &Value, window: &mut Window, cx: &mut Context<T>) {
    if let Some(state) = decode::<T>(state.clone()) {
        item.apply(&state, window, cx);
    }
}

fn decode<T: ItemState>(state: Value) -> Option<T::State> {
    match serde_json::from_value(state) {
        Ok(state) => Some(state),
        Err(error) => {
            log::warn!("dropping a saved {} item: {error}", T::KIND);
            None
        }
    }
}
