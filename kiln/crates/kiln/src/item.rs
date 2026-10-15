// SPDX-License-Identifier: LicenseRef-Proprietary

//! [`Item`]: a pane item as a function of its typed state, which both
//! opens it and restores it from a saved layout; `#[kiln::item]`
//! ([`crate::item`]) makes it a [`PaneItem`]. [`WorkspaceExt`] opens one
//! in the active pane, or activates its tab there if an item of its state
//! is open already.

use gpui::{App, AppContext as _, Context, Entity, FocusHandle, SharedString, Task, Window};
use kiln_core::serde::{Serialize, de::DeserializeOwned};
use kiln_shell::{ItemHandle as _, PaneItem, PaneView, StatusItem, Workspace};
use kiln_ui::Icon;

/// A tab as a function of its state: [`Item::new`] makes it from a state,
/// both to open it ([`WorkspaceExt::open`]) and to restore it from a saved
/// layout, and [`Item::state`] gives that state back, saved with the layout
/// and compared to find its tab again. `#[kiln::item]` ([`crate::item`]) on
/// the struct makes it a [`PaneItem`] through these, its kind restoring with
/// no register call.
///
/// ```
/// use kiln::{
///     Cx, Item, View,
///     gpui::{App, AppContext as _, FocusHandle, IntoElement, SharedString, Window, div,
///         prelude::*},
///     serde::{Deserialize, Serialize},
/// };
///
/// #[derive(Clone, PartialEq, Serialize, Deserialize)]
/// #[serde(crate = "kiln::serde")]
/// struct NoteState {
///     name: String,
/// }
///
/// #[kiln::item]
/// struct Note {
///     name: String,
///     focus: FocusHandle,
/// }
///
/// impl Item for Note {
///     type State = NoteState;
///
///     fn new(state: NoteState, _: &mut Window, cx: &mut App) -> Option<Self> {
///         Some(Self { name: state.name, focus: cx.focus_handle() })
///     }
///
///     fn state(&self) -> NoteState {
///         NoteState { name: self.name.clone() }
///     }
///
///     fn title(&self, _: &App) -> SharedString {
///         self.name.clone().into()
///     }
///
///     fn focus(&self, _: &App) -> FocusHandle {
///         self.focus.clone()
///     }
/// }
///
/// impl View for Note {
///     fn render(&mut self, _: &mut Cx<Self>) -> impl IntoElement {
///         div().child(self.name.clone())
///     }
/// }
/// ```
///
/// An item whose restore reads a file first, or that splits, implements
/// [`PaneItem`] by hand instead, with
/// [`ItemState`](kiln_shell::item::ItemState) for a typed state.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a kiln::Item",
    label = "#[kiln::item] needs `impl kiln::Item for {Self}`",
    note = "`impl Item for {Self} {{ type State = ..; fn new(..); fn state(..); fn title(..); fn focus(..) }}`"
)]
pub trait Item: Sized + 'static {
    /// What the item is a function of: saved with the layout, and equal
    /// for two items that would show the same thing.
    type State: Serialize + DeserializeOwned + PartialEq + Clone + 'static;

    /// The item `state` describes, to open or to restore; `None` if it
    /// can't be made now (what it shows is gone), so nothing opens, and a
    /// restored layout leaves it out. The item's entity is made from it
    /// once it returns, so it takes the window and app rather than a
    /// [`crate::Cx`] of its own: a view it watches, it watches as it
    /// renders ([`crate::Cx::watch`]).
    fn new(state: Self::State, window: &mut Window, cx: &mut App) -> Option<Self>;

    /// What [`Item::new`] would make it again from.
    fn state(&self) -> Self::State;

    /// The tab's label, and the window's title while it's the active item.
    fn title(&self, cx: &App) -> SharedString;

    /// Its focus handle, which activating its tab focuses, made in
    /// [`Item::new`] (`cx.focus_handle()`).
    fn focus(&self, cx: &App) -> FocusHandle;

    /// It has changes that closing would lose: the tab shows a dirty dot,
    /// and closing it asks first.
    fn is_dirty(&self, _cx: &App) -> bool {
        false
    }

    /// The tab's icon, if any.
    fn icon(&self, _cx: &App) -> Option<Icon> {
        None
    }

    /// Its status bar items while it's the active item.
    fn status_items(&self, _cx: &App) -> Vec<StatusItem> {
        Vec::new()
    }
}

/// The item `state` describes, as an entity, if [`Item::new`] makes one.
fn make<T: Item>(state: T::State, window: &mut Window, cx: &mut App) -> Option<Entity<T>> {
    let item = T::new(state, window, cx)?;
    Some(cx.new(|_| item))
}

/// `#[kiln::item]`'s [`ItemState::from_state`](kiln_shell::item::ItemState::from_state):
/// [`Item::new`]'s item, at once.
#[doc(hidden)]
pub fn restore_item<T: Item>(
    state: T::State,
    window: &mut Window,
    cx: &mut App,
) -> Task<Option<Entity<T>>> {
    Task::ready(make(state, window, cx))
}

/// The [`Item`]s of a workspace's panes, by state: `use kiln::prelude::*;`
/// then, in a command, `workspace.open::<Note>(state, cx.window, cx.cx)`.
pub trait WorkspaceExt {
    /// Shows the item of `state` in the active pane, focused: the tab there
    /// whose [`Item::state`] equals `state` if there is one, else a new one
    /// from [`Item::new`], nothing if that gives `None`. The item shown, if
    /// any.
    fn open<T: Item + PaneItem>(
        &mut self,
        state: T::State,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<T>>
    where
        Self: Sized;

    /// The active pane's active item's [`Item::state`], if it's a `T`.
    fn active_state<T: Item + PaneItem>(&self, cx: &App) -> Option<T::State>;
}

impl<P: PaneView> WorkspaceExt for Workspace<P> {
    fn open<T: Item + PaneItem>(
        &mut self,
        state: T::State,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<T>> {
        let items = self.active_pane().read(cx).pane().items();
        let shown = (items.iter())
            .filter_map(|item| item.downcast::<T>())
            .find(|item| item.read(cx).state() == state);
        if let Some(shown) = shown {
            let id = shown.id();
            self.activate_item_where(|item, _| item.id() == id, window, cx);
            return Some(shown);
        }
        let item = make::<T>(state, window, cx)?;
        self.add_item(Box::new(item.clone()), window, cx);
        Some(item)
    }

    fn active_state<T: Item + PaneItem>(&self, cx: &App) -> Option<T::State> {
        let item = self.active_item(cx)?.downcast_ref::<T>()?;
        Some(item.read(cx).state())
    }
}
