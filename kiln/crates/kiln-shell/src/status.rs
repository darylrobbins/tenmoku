// SPDX-License-Identifier: Apache-2.0

//! The status bar's items: data a host and the active pane item contribute
//! ([`StatusItem`]), the host's set of them over the item's
//! ([`StatusItems`], [`StatusItems::items_for`]), and how they draw
//! ([`status_bar`], [`status_entry`]): kiln-ui's bar, 24 px on the chrome
//! surface in the UI label type, each entry with one 8 px padding either
//! side, a filled one as a pill inside it.
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

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    SharedString, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use kiln_command::Invocation;
use kiln_l10n::verbatim;
use kiln_platform::{command_actions::dispatch_invocation, keys};
use kiln_ui::{
    ActiveTheme as _, Icon, Role, StatusBarFrame, StatusEntry, StatusFill, StatusTone, node,
    ui_theme,
};

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
    /// Drawn before the text.
    pub icon: Option<Icon>,
    /// Colours the icon ([`kiln_ui::status_entry_colors`]).
    pub tone: StatusTone,
    /// Draws the item's text and icon in a pill of this fill, as a vim
    /// mode is ([`kiln_ui::StatusEntry::fill`]).
    pub fill: Option<StatusFill>,
    /// Shown on hover.
    pub tooltip: Option<SharedString>,
    /// Draws a 6 px accent dot at the icon's top right: something there
    /// wants the user ([`StatusItem::dot`]).
    pub dot: bool,
    /// What a screen reader calls it, as a button if it has a command:
    /// for an item that is only an icon, or whose text says less than its
    /// state ([`StatusItem::name`]).
    pub name: Option<SharedString>,
}

impl StatusItem {
    /// An item that runs nothing when clicked, with no icon, fill or
    /// tooltip and the default tone: see [`StatusItem::command`] and the
    /// builders after it.
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
            icon: None,
            tone: StatusTone::Default,
            fill: None,
            tooltip: None,
            dot: false,
            name: None,
        }
    }

    /// Runs `command` when clicked.
    pub fn command(mut self, command: Invocation) -> Self {
        self.command = Some(command);
        self
    }

    /// Draws `icon` before the text.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Colours the icon by `tone`.
    pub fn tone(mut self, tone: StatusTone) -> Self {
        self.tone = tone;
        self
    }

    /// Draws the item in a pill filled with `fill`.
    pub fn fill(mut self, fill: StatusFill) -> Self {
        self.fill = Some(fill);
        self
    }

    /// Shows `tooltip` on hover.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Draws the accent dot on the icon, or not.
    pub fn dot(mut self, dot: bool) -> Self {
        self.dot = dot;
        self
    }

    /// Names the item `name` for a screen reader.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
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
/// bounds are those of the debug selector `status-item:{id}`, its icon's
/// those of `status-icon:{id}`, and its dot's, if it has one, those of
/// `status-dot:{id}`.
pub fn status_entry(item: StatusItem, cx: &App) -> impl IntoElement + use<> {
    let mut entry = StatusEntry::new(item.id.clone(), item.text, &ui_theme(cx)).tone(item.tone);
    if let Some(icon) = item.icon {
        entry = entry.icon(icon);
    }
    if let Some(fill) = item.fill {
        entry = entry.fill(fill);
    }
    if let Some(tooltip) = item.tooltip {
        entry = entry.tooltip(tooltip);
    }
    let button = item.command.is_some();
    let entry = match item.command {
        Some(command) => {
            entry.on_click(move |_, window, cx| run_item_command(command.clone(), window, cx))
        }
        None => entry,
    };
    Marked {
        id: item.id,
        entry,
        dot: item.dot,
        name: item.name,
        button,
    }
}

/// How far the dot sits from the entry's left and top: over the top right
/// of a 14 px icon after the entry's 8 px padding, in the 24 px bar.
const DOT_LEFT: f32 = 17.;
const DOT_TOP: f32 = 4.;
const DOT_SIZE: f32 = 6.;

/// An entry with its dot and accessible name, if it has either: in a
/// wrapper then, the entry as it is otherwise.
#[derive(IntoElement)]
struct Marked {
    id: SharedString,
    entry: StatusEntry,
    dot: bool,
    name: Option<SharedString>,
    button: bool,
}

impl RenderOnce for Marked {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        if !self.dot && self.name.is_none() {
            return self.entry.into_any_element();
        }
        let id = self.id;
        let prefix = verbatim!("status-marked:");
        let wrapper = div()
            .id(ElementId::Name(format!("{prefix}{id}").into()))
            .relative()
            .flex_none()
            .h_full()
            .child(self.entry)
            .when(self.dot, |wrapper| {
                let accent = cx.theme().colors.accent.base;
                let id = id.clone();
                wrapper.child(
                    div()
                        .debug_selector(move || {
                            let prefix = verbatim!("status-dot:");
                            format!("{prefix}{id}")
                        })
                        .absolute()
                        .left(px(DOT_LEFT))
                        .top(px(DOT_TOP))
                        .size(px(DOT_SIZE))
                        .rounded_full()
                        .bg(accent),
                )
            });
        match self.name {
            Some(name) => {
                let role = match self.button {
                    true => Role::Button,
                    false => Role::Label,
                };
                node(wrapper, window, cx, role, name).into_any_element()
            }
            None => wrapper.into_any_element(),
        }
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

#[cfg(test)]
mod tests {
    use gpui::{
        Context, IntoElement, ParentElement as _, Render, Styled as _, TestAppContext, Window, div,
        px,
    };

    use super::{Icon, StatusItem, StatusSide, status_bar};

    struct Bench(Vec<StatusItem>);

    impl Render for Bench {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(div().flex_1())
                .child(status_bar(self.0.clone(), cx))
        }
    }

    #[test]
    fn the_dot_and_name_builders() {
        let plain = StatusItem::new("bell", StatusSide::Right, 0, "");
        assert!(!plain.dot);
        assert_eq!(plain.name, None);
        let marked = plain.dot(true).name("2 notifications");
        assert!(marked.dot);
        assert_eq!(marked.name.as_deref(), Some("2 notifications"));
        assert!(!marked.dot(false).dot);
    }

    #[gpui::test]
    fn a_dotted_item_draws_its_dot_on_the_icon(cx: &mut TestAppContext) {
        let items = vec![
            StatusItem::new("bell", StatusSide::Right, 0, "")
                .icon(Icon::Bell)
                .dot(true)
                .name("1 notification needs you"),
            StatusItem::new("quiet", StatusSide::Right, 1, "")
                .icon(Icon::Bell)
                .name("No notifications"),
            StatusItem::new("plain", StatusSide::Left, 0, "Plain"),
        ];
        let (_, cx) = cx.add_window_view(|_, _| Bench(items));
        cx.run_until_parked();

        let item = cx.debug_bounds("status-item:bell").expect("drawn");
        let icon = cx.debug_bounds("status-icon:bell").expect("icon drawn");
        let dot = cx.debug_bounds("status-dot:bell").expect("dot drawn");
        assert_eq!(dot.size, gpui::size(px(6.), px(6.)));
        // Over the icon's top right, inside the entry.
        assert!(item.contains(&dot.origin) && item.contains(&dot.bottom_right()));
        assert!(
            dot.left() > icon.center().x && dot.left() < icon.right(),
            "{dot:?} {icon:?}"
        );
        assert!(dot.top() < icon.top() + px(3.), "{dot:?} {icon:?}");

        assert_eq!(cx.debug_bounds("status-dot:quiet"), None);
        assert_eq!(cx.debug_bounds("status-dot:plain"), None);
        assert!(cx.debug_bounds("status-item:quiet").is_some());
        assert!(cx.debug_bounds("status-item:plain").is_some());
    }
}
