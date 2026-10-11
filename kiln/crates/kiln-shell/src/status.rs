// SPDX-License-Identifier: LicenseRef-Proprietary

//! The status bar's items: data a host and the active pane item contribute
//! ([`StatusItem`]), the host's set of them over the item's
//! ([`StatusItems`], [`StatusItems::items_for`]), and how they draw
//! ([`status_bar`], [`status_entry`]).
//!
//! Items sort by side, then by priority, lower first, so left to right;
//! ties keep the order they were given in, the active item's before the
//! host's. A host item with an id the active item also gives shows in its
//! place, so a host can replace one of the item's (its own language
//! picker, say) and take it away again.
//!
//! A clicked item's command is dispatched as a key would dispatch it: to
//! the focused element, stamped, if it names no context, with the focused
//! element's ([`kiln_platform::keys::context_stack`]).

use gpui::{App, IntoElement, SharedString, Window};
use kiln_command::Invocation;
use kiln_platform::{command_actions::dispatch_invocation, keys};
use kiln_ui::{StatusBarFrame, StatusEntry, ui_theme};

use crate::ItemHandle;

/// Which end of the bar an item sits at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StatusSide {
    Left,
    Right,
}

/// One item on the bar.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusItem {
    /// Distinct among the bar's items.
    pub id: SharedString,
    pub side: StatusSide,
    /// Lower sits further left on its side.
    pub priority: i32,
    pub text: SharedString,
    /// Run when the item is clicked, as a key would run it. An invocation
    /// with no context gets the focused element's then.
    pub command: Option<Invocation>,
}

impl StatusItem {
    /// An item that runs nothing when clicked: see [`StatusItem::command`].
    pub fn new(
        id: impl Into<SharedString>,
        side: StatusSide,
        priority: i32,
        text: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            side,
            priority,
            text: text.into(),
            command: None,
        }
    }

    /// Runs `command` when clicked.
    pub fn command(mut self, command: Invocation) -> Self {
        self.command = Some(command);
        self
    }
}

/// The host's items, set by id, shown over those of the active item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatusItems {
    /// In the order first set.
    host: Vec<StatusItem>,
}

impl StatusItems {
    /// Adds `item`, or replaces the item with its id where it was.
    pub fn set(&mut self, item: StatusItem) {
        match self.host.iter_mut().find(|old| old.id == item.id) {
            Some(old) => *old = item,
            None => self.host.push(item),
        }
    }

    /// Removes the host's item `id`: an item of the active item's that it
    /// replaced shows again. Whether there was one.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.host.len();
        self.host.retain(|item| item.id != id);
        self.host.len() != before
    }

    /// The host's items, in the order first set.
    pub fn host(&self) -> &[StatusItem] {
        &self.host
    }

    /// Every item to draw, sorted: `own` (the active item's), less those
    /// the host replaces, then the host's.
    pub fn merged(&self, own: impl IntoIterator<Item = StatusItem>) -> Vec<StatusItem> {
        let mut items: Vec<_> = own
            .into_iter()
            .filter(|item| !self.host.iter().any(|host| host.id == item.id))
            .chain(self.host.iter().cloned())
            .collect();
        // Stable, so ties keep their order.
        items.sort_by_key(|item| (item.side, item.priority));
        items
    }
}

impl StatusItems {
    /// Every item to draw while `active` is the active pane's active item:
    /// its [`PaneItem::status_items`](crate::PaneItem::status_items) with
    /// the host's over them ([`StatusItems::merged`]); the host's alone
    /// with no item.
    pub fn items_for(&self, active: Option<&dyn ItemHandle>, cx: &App) -> Vec<StatusItem> {
        self.merged(active.map(|item| item.status_items(cx)).unwrap_or_default())
    }
}

/// The bar for `items`, sorted ([`StatusItems::merged`]): each side's
/// items in order.
pub fn status_bar(items: Vec<StatusItem>, cx: &App) -> StatusBarFrame {
    let ui = ui_theme(cx);
    let (left, right): (Vec<_>, Vec<_>) = items
        .into_iter()
        .partition(|item| item.side == StatusSide::Left);
    StatusBarFrame::new(&ui)
        .left(left.into_iter().map(|item| status_entry(item, cx)))
        .right(right.into_iter().map(|item| status_entry(item, cx)))
}

/// `item`'s entry, which runs its command when clicked. In tests its
/// bounds are those of the debug selector `status-item:{id}`.
pub fn status_entry(item: StatusItem, cx: &App) -> impl IntoElement + use<> {
    let entry = StatusEntry::new(item.id, item.text, &ui_theme(cx));
    match item.command {
        Some(command) => {
            entry.on_click(move |_, window, cx| run_item_command(command.clone(), window, cx))
        }
        None => entry,
    }
}

/// Runs a clicked item's `command` as a key would.
fn run_item_command(mut command: Invocation, window: &mut Window, cx: &mut App) {
    if command.context.is_empty() {
        command.context = keys::context_stack(&window.context_stack(), cx);
    }
    if let Err(error) = dispatch_invocation(command, window, cx) {
        log::warn!("{error}");
    }
}
