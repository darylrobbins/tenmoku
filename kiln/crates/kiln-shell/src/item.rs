// SPDX-License-Identifier: LicenseRef-Proprietary

//! What a pane shows in a tab: any view implementing [`PaneItem`], held
//! through the object-safe [`ItemHandle`], which every `Entity<T:
//! PaneItem>` is. An editor is one item; a host's chat or preview pane is
//! another, and tabs, splits, closing, the window's title, the toolbar's
//! crumbs and the status bar work the same for both.
//!
//! ```ignore
//! impl PaneItem for Notes {
//!     const KIND: &'static str = "notes.note";
//!
//!     fn tab_title(&self, _: &App) -> SharedString {
//!         self.title.clone()
//!     }
//!
//!     fn is_dirty(&self, _: &App) -> bool {
//!         self.unsaved
//!     }
//! }
//!
//! let item: Box<dyn ItemHandle> = Box::new(cx.new(|cx| Notes::new(cx)));
//! let notes = item.downcast::<Notes>().unwrap();
//! ```
//!
//! Every method but [`PaneItem::tab_title`] has a default, so an item
//! implements what applies to it.

use std::{
    any::Any,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::{
    AnyView, App, Context, Entity, EntityId, FocusHandle, Focusable, Render, SharedString,
    Subscription, Task, Window,
};
use kiln_command::Value;
use kiln_ui::{CrumbKind, Icon};

use crate::StatusItem;

/// A tab's identity: its item's entity ID. It stays with the tab as the
/// tab moves between panes. It isn't saved: a restored item is a new
/// entity with a new ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemId(pub EntityId);

/// What a pane shows in a tab. The pane observes the item, so any
/// `cx.notify()` redraws its tab, crumbs and status items and retitles the
/// window.
///
/// Focus is the [`Focusable`] supertrait: activating a tab focuses its
/// item's `focus_handle`.
pub trait PaneItem: Render + Focusable + Sized + 'static {
    /// The item's kind in saved layouts, unique among the kinds an app
    /// registers: `"notes.note"`, `"host.chat"`.
    const KIND: &'static str;

    // Title and icon.

    /// The tab's label.
    fn tab_title(&self, cx: &App) -> SharedString;

    /// The tab's icon, if any.
    fn tab_icon(&self, _cx: &App) -> Option<Icon> {
        None
    }

    /// The window's title while this is the active pane's active item.
    fn window_title(&self, cx: &App) -> SharedString {
        self.tab_title(cx)
    }

    // Dirty state, save and close veto.

    /// It has changes that closing would lose: the tab shows a dirty dot.
    fn is_dirty(&self, _cx: &App) -> bool {
        false
    }

    /// Whether `other` shows the same document, so this item's unsaved
    /// changes live on there: two views over one buffer, say. A pane
    /// closes, or moves away, a dirty item without asking while another
    /// open item shares its document.
    fn shares_document(&self, _other: &AnyView, _cx: &App) -> bool {
        false
    }

    /// Asked before anything closes the item: its tab, its pane, its
    /// window, or quitting ([`crate::close`]). It isn't asked while an
    /// item that stays open shares its document. The default keeps a dirty
    /// item open.
    fn may_close(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> CloseAnswer {
        if self.is_dirty(cx) {
            CloseAnswer::Keep
        } else {
            CloseAnswer::Close
        }
    }

    /// Its close-state revision: a number that changes whenever anything
    /// [`PaneItem::may_close`] answers by changes, such as whether it's
    /// dirty and its document's version. A close that an item agreed to
    /// asks it again if this changed before the close went ahead
    /// ([`crate::close`]). The default follows [`PaneItem::is_dirty`] only.
    fn close_revision(&self, cx: &App) -> u64 {
        u64::from(self.is_dirty(cx))
    }

    /// Saves it, for Save All and quitting with unsaved items. `None`: it
    /// has nothing to save.
    fn save(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Task<gpui::Result<()>>> {
        None
    }

    // Toolbar and status bar.

    /// The file it shows, if any: the pane's toolbar shows it as path
    /// crumbs, relative to the window's root folder.
    fn path(&self, _cx: &App) -> Option<&Path> {
        None
    }

    /// Crumbs after the path's, in the pane's toolbar.
    fn breadcrumbs(&self, _cx: &App) -> Vec<ItemCrumb> {
        Vec::new()
    }

    /// Its status bar items while it's the active item. The host's items
    /// with the same id show in their place ([`crate::StatusItems`]).
    fn status_items(&self, _cx: &App) -> Vec<StatusItem> {
        Vec::new()
    }

    // Split.

    /// A second item over the same document, for splitting the pane.
    /// `None`: the new pane opens empty.
    fn split(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Entity<Self>> {
        None
    }

    // Saved layouts.

    /// Its state in a saved layout. `None` leaves it out of the layout,
    /// as for a document never saved.
    fn serialize(&self, _cx: &App) -> Option<Value> {
        None
    }

    /// Rebuilds an item from [`PaneItem::serialize`]'s output, possibly
    /// after reading a file. `None` drops the item from the restored
    /// layout.
    fn restore(_state: Value, _window: &mut Window, _cx: &mut App) -> Task<Option<Entity<Self>>> {
        Task::ready(None)
    }

    /// Applies the per-view part of a saved state, such as a caret, to an
    /// item [`PaneItem::split`] made while restoring.
    fn apply_state(&mut self, _state: &Value, _window: &mut Window, _cx: &mut Context<Self>) {}
}

/// What [`PaneItem::may_close`] answers. It's synchronous when it can be,
/// so closing stays immediate.
pub enum CloseAnswer {
    Close,
    /// Stays open; the shell does nothing more.
    Keep,
    /// Asked the user (a save prompt, say): closes if the task resolves
    /// `true`.
    Ask(Task<bool>),
}

impl std::fmt::Debug for CloseAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Close => "Close",
            Self::Keep => "Keep",
            Self::Ask(_) => "Ask",
        })
    }
}

/// What a click on an [`ItemCrumb`] runs.
pub type CrumbClick = Rc<dyn Fn(&mut Window, &mut App)>;

/// A crumb an item contributes, for kiln-ui's [`kiln_ui::Crumb`].
#[derive(Clone)]
pub struct ItemCrumb {
    pub label: SharedString,
    pub kind: CrumbKind,
    pub icon: Option<Icon>,
    /// Run on a click, after the pane focuses the item, so focus returns
    /// there once a picker this opens is dismissed.
    pub on_click: Option<CrumbClick>,
}

impl ItemCrumb {
    pub fn new(label: impl Into<SharedString>, kind: CrumbKind) -> Self {
        Self {
            label: label.into(),
            kind,
            icon: None,
            on_click: None,
        }
    }
}

/// What [`ItemHandle::observe_in`] runs when the item notifies.
pub type OnChangeIn = Box<dyn FnMut(&mut Window, &mut App)>;

/// A [`PaneItem`] of any type: what a pane holds in a tab. Each method
/// forwards to the item's, reading or updating its entity.
pub trait ItemHandle: 'static {
    fn id(&self) -> ItemId;
    /// [`PaneItem::KIND`].
    fn kind(&self) -> &'static str;
    fn to_any_view(&self) -> AnyView;
    /// The `Entity<T>` itself, for [`ItemHandle::downcast_ref`].
    fn as_any(&self) -> &dyn Any;
    fn boxed_clone(&self) -> Box<dyn ItemHandle>;
    /// The item's focus handle, which activating its tab focuses. (Not
    /// `focus_handle`, which `Entity<T>` has already through `Focusable`.)
    fn item_focus_handle(&self, cx: &App) -> FocusHandle;
    /// Runs `changed` whenever the item notifies, while the subscription
    /// is held.
    fn observe(&self, changed: Box<dyn FnMut(&mut App)>, cx: &mut App) -> Subscription;
    /// Runs `changed` in `window` whenever the item notifies, while the
    /// subscription is held and the window is open.
    fn observe_in(&self, window: &mut Window, changed: OnChangeIn, cx: &mut App) -> Subscription;

    fn tab_title(&self, cx: &App) -> SharedString;
    fn tab_icon(&self, cx: &App) -> Option<Icon>;
    fn window_title(&self, cx: &App) -> SharedString;
    fn is_dirty(&self, cx: &App) -> bool;
    /// Whether `other` shows this item's document: see
    /// [`PaneItem::shares_document`].
    fn shares_document(&self, other: &dyn ItemHandle, cx: &App) -> bool;
    fn may_close(&self, window: &mut Window, cx: &mut App) -> CloseAnswer;
    fn close_revision(&self, cx: &App) -> u64;
    fn save(&self, window: &mut Window, cx: &mut App) -> Option<Task<gpui::Result<()>>>;
    fn path(&self, cx: &App) -> Option<PathBuf>;
    fn breadcrumbs(&self, cx: &App) -> Vec<ItemCrumb>;
    fn status_items(&self, cx: &App) -> Vec<StatusItem>;
    fn split(&self, window: &mut Window, cx: &mut App) -> Option<Box<dyn ItemHandle>>;
    fn serialize(&self, cx: &App) -> Option<Value>;
    fn apply_state(&self, state: &Value, window: &mut Window, cx: &mut App);
}

impl<T: PaneItem> ItemHandle for Entity<T> {
    fn id(&self) -> ItemId {
        ItemId(self.entity_id())
    }

    fn kind(&self) -> &'static str {
        T::KIND
    }

    fn to_any_view(&self) -> AnyView {
        self.clone().into()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn boxed_clone(&self) -> Box<dyn ItemHandle> {
        Box::new(self.clone())
    }

    fn item_focus_handle(&self, cx: &App) -> FocusHandle {
        self.read(cx).focus_handle(cx)
    }

    fn observe(&self, mut changed: Box<dyn FnMut(&mut App)>, cx: &mut App) -> Subscription {
        cx.observe(self, move |_, cx| changed(cx))
    }

    fn observe_in(
        &self,
        window: &mut Window,
        mut changed: OnChangeIn,
        cx: &mut App,
    ) -> Subscription {
        window.observe(self, cx, move |_, window, cx| changed(window, cx))
    }

    fn tab_title(&self, cx: &App) -> SharedString {
        self.read(cx).tab_title(cx)
    }

    fn tab_icon(&self, cx: &App) -> Option<Icon> {
        self.read(cx).tab_icon(cx)
    }

    fn window_title(&self, cx: &App) -> SharedString {
        self.read(cx).window_title(cx)
    }

    fn is_dirty(&self, cx: &App) -> bool {
        self.read(cx).is_dirty(cx)
    }

    fn shares_document(&self, other: &dyn ItemHandle, cx: &App) -> bool {
        other.id() != self.id() && self.read(cx).shares_document(&other.to_any_view(), cx)
    }

    fn may_close(&self, window: &mut Window, cx: &mut App) -> CloseAnswer {
        self.update(cx, |item, cx| item.may_close(window, cx))
    }

    fn close_revision(&self, cx: &App) -> u64 {
        self.read(cx).close_revision(cx)
    }

    fn save(&self, window: &mut Window, cx: &mut App) -> Option<Task<gpui::Result<()>>> {
        self.update(cx, |item, cx| item.save(window, cx))
    }

    fn path(&self, cx: &App) -> Option<PathBuf> {
        self.read(cx).path(cx).map(Path::to_owned)
    }

    fn breadcrumbs(&self, cx: &App) -> Vec<ItemCrumb> {
        self.read(cx).breadcrumbs(cx)
    }

    fn status_items(&self, cx: &App) -> Vec<StatusItem> {
        self.read(cx).status_items(cx)
    }

    fn split(&self, window: &mut Window, cx: &mut App) -> Option<Box<dyn ItemHandle>> {
        let split = self.update(cx, |item, cx| item.split(window, cx))?;
        Some(Box::new(split))
    }

    fn serialize(&self, cx: &App) -> Option<Value> {
        self.read(cx).serialize(cx)
    }

    fn apply_state(&self, state: &Value, window: &mut Window, cx: &mut App) {
        self.update(cx, |item, cx| item.apply_state(state, window, cx));
    }
}

impl dyn ItemHandle {
    /// The item as a `T`, if it is one.
    pub fn downcast<T: PaneItem>(&self) -> Option<Entity<T>> {
        self.to_any_view().downcast().ok()
    }

    /// The item as a `T`, borrowed, if it is one.
    pub fn downcast_ref<T: PaneItem>(&self) -> Option<&Entity<T>> {
        self.as_any().downcast_ref()
    }
}

impl Clone for Box<dyn ItemHandle> {
    fn clone(&self) -> Self {
        self.boxed_clone()
    }
}

impl std::fmt::Debug for dyn ItemHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ItemHandle")
            .field("kind", &self.kind())
            .field("id", &self.id())
            .finish()
    }
}
