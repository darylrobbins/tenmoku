//! The notification inbox's popup and the bell that opens it
//! (`docs/design/design-system.md`, Notifications and agents).
//!
//! The bell is the status bar's last item on the right
//! ([`StatusItem::NOTIFICATIONS`](crate::StatusItem::NOTIFICATIONS)): a
//! `bell` icon with a 6 px accent dot while anything in the window's
//! [`NotificationInbox`](crate::workbench::toast::notification_inbox) needs
//! the user, and an accessible name that counts them. The view keeps it in
//! step with the toast layer, which notifies as the inbox changes.
//!
//! A click on it, or `notifications.show` from the palette, opens the
//! popup above it at the window's lower right, and takes focus, in the key
//! context `notification_inbox`; doing either again closes it. Two groups,
//! newest first: Needs you, each row on the `selected` surface (an agent's
//! on the agent's soft colour) with its action or its question's answers,
//! and Earlier, with a Clear earlier button. A row: the source's icon in
//! its colour (the agent's for an agent, else its level's), title, one
//! detail line, age and a dismiss, its buttons on a line below. Nothing but
//! the bell's dot and focus is drawn in the accent. Ages count on while it's
//! open, the popup redrawing as the first of them reaches its next step.
//!
//! The selected row is a notification, not a place in the list: it stays
//! on the same one as others come, merge or move between the groups, and
//! falls to the row in its place only when its own goes. Each row's and
//! button's element follows its notification the same way.
//!
//! With the popup itself focused, Up and Down move the selected row and
//! Enter acts on it: it runs its action (as the palette would, from where
//! focus was before the popup), answers its question's one answer, or with
//! more than one, moves focus to the first. Tab and Shift-Tab move focus
//! through the buttons, the selected row's first, and back to the popup
//! past either end; a focused button takes Enter and Space itself. Escape
//! closes the popup and gives focus back, and focus going elsewhere closes
//! it too. Dismissing a Needs-you row moves it to Earlier, dismissing an
//! Earlier one or Clear earlier removes them, and any of their toasts still
//! shown or waiting go with them.

use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime},
};

use fe_command::{CommandId, Source};
use fe_ui::{ActiveTheme as _, Button, Icon, IconButton, radius};
use gpui::{
    App, Context, Entity, FocusHandle, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    KeyDownEvent, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity, WeakFocusHandle,
    Window, div, prelude::FluentBuilder as _, px, svg,
};
use kiln_l10n::{Text, text, tr, verbatim};

use crate::{
    StatusBar,
    workbench::toast::{
        self, Level, ToastLayer, card_look, level_icon,
        notification_inbox::{Notification, NotificationId, NotificationSource, Resolution},
    },
};

/// The command that opens the popup, or closes it.
pub(crate) const SHOW: &str = "notifications.show";

/// The popup's key context.
pub(crate) const KEY_CONTEXT: &str = "notification_inbox";

/// The popup's width, and the most it grows before its rows scroll.
const WIDTH: f32 = 380.;
const MAX_HEIGHT: f32 = 440.;

/// How far the popup sits from the window's right edge, and above the
/// status bar.
pub(crate) const MARGIN: f32 = 8.;

/// Which group a row is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Group {
    NeedsYou,
    Earlier,
}

/// A button in the popup: whose, for its focus handle and the tab order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ButtonKey {
    /// A Needs-you row's answer, by its index among the question's.
    Answer(NotificationId, usize),
    /// A Needs-you row's action.
    Action(NotificationId),
    Dismiss(NotificationId),
    ClearEarlier,
}

impl ButtonKey {
    fn notification(self) -> Option<NotificationId> {
        match self {
            Self::Answer(id, _) | Self::Action(id) | Self::Dismiss(id) => Some(id),
            Self::ClearEarlier => None,
        }
    }
}

/// A window's inbox popup, and the bell's count.
pub(crate) struct NotificationInboxView {
    layer: Entity<ToastLayer>,
    bar: WeakEntity<StatusBar>,
    open: bool,
    /// The row Up and Down move and Enter acts on, by its notification.
    selected: Option<NotificationId>,
    /// Its place in [`NotificationInboxView::rows`] when last known: the
    /// row in its place is selected if it goes.
    selected_at: usize,
    focus: FocusHandle,
    /// Each button's focus, made as it's first drawn.
    buttons: HashMap<ButtonKey, FocusHandle>,
    /// Where focus was before the popup opened, to give it back.
    restore: Option<WeakFocusHandle>,
    scroll: ScrollHandle,
    /// The wall clock at an executor instant, taken as the popup opens, for
    /// the ages: the executor's clock moves them on, as a test's does.
    clock: (Instant, SystemTime),
    /// Redraws the popup as the next age steps on, while it's open.
    ticker: Option<Task<()>>,
    /// Each row's age as the last render drew it.
    #[cfg(test)]
    drawn_ages: Vec<(NotificationId, SharedString)>,
    /// How many times the ticker has redrawn the popup.
    #[cfg(test)]
    ticks: usize,
    _observe: Subscription,
    _focus_out: Subscription,
}

impl NotificationInboxView {
    /// The popup for `layer`'s inbox, keeping `bar`'s bell in step.
    pub(crate) fn new(
        layer: Entity<ToastLayer>,
        bar: WeakEntity<StatusBar>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let observe = cx.observe(&layer, |view, _, cx| view.changed(cx));
        let focus = cx.focus_handle();
        // A click outside, or a picker opened: focus went, and the popup
        // goes with it.
        let focus_out = cx.on_focus_out(&focus, window, |view, _, window, cx| {
            view.close(false, window, cx);
        });
        let view = Self {
            layer,
            bar,
            open: false,
            selected: None,
            selected_at: 0,
            focus,
            buttons: HashMap::new(),
            restore: None,
            scroll: ScrollHandle::new(),
            clock: (cx.background_executor().now(), SystemTime::now()),
            ticker: None,
            #[cfg(test)]
            drawn_ages: Vec::new(),
            #[cfg(test)]
            ticks: 0,
            _observe: observe,
            _focus_out: focus_out,
        };
        view.sync_bell(cx);
        view
    }

    /// Whether the popup is open.
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// The selected row's notification.
    #[cfg(test)]
    pub(crate) fn selected(&self) -> Option<NotificationId> {
        self.selected
    }

    /// Each row's age as the last render drew it, and how many times the
    /// ticker has redrawn the popup.
    #[cfg(test)]
    pub(crate) fn drawn_ages(&self) -> (&[(NotificationId, SharedString)], usize) {
        (&self.drawn_ages, self.ticks)
    }

    /// Every row, as drawn: Needs you, then Earlier, each newest first.
    pub(crate) fn rows<'a>(&self, cx: &'a App) -> Vec<(Group, &'a Notification)> {
        let inbox = self.layer.read(cx).inbox();
        let needs_you: Vec<_> = inbox.needs_you().collect();
        let earlier: Vec<_> = inbox.earlier().collect();
        let needs_you = (needs_you.into_iter().rev()).map(|item| (Group::NeedsYou, item));
        let earlier = (earlier.into_iter().rev()).map(|item| (Group::Earlier, item));
        needs_you.chain(earlier).collect()
    }

    /// The selected row's place among `rows`.
    fn selected_index(&self, rows: &[(Group, &Notification)]) -> usize {
        (rows.iter())
            .position(|(_, item)| Some(item.id) == self.selected)
            .unwrap_or_else(|| self.selected_at.min(rows.len().saturating_sub(1)))
    }

    /// The wall clock now, by the executor's.
    fn now(&self, cx: &App) -> SystemTime {
        let (instant, system) = self.clock;
        system
            + cx.background_executor()
                .now()
                .saturating_duration_since(instant)
    }

    /// Opens the popup if it's closed, else closes it.
    pub(crate) fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.close(true, window, cx);
        } else {
            self.open(window, cx);
        }
    }

    /// Opens the popup on its first row, giving it focus.
    pub(crate) fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            return;
        }
        self.restore = window.focused(cx).map(|focus| focus.downgrade());
        self.open = true;
        self.clock = (cx.background_executor().now(), SystemTime::now());
        self.selected = self.rows(cx).first().map(|(_, item)| item.id);
        self.selected_at = 0;
        self.scroll.scroll_to_item(0);
        window.focus(&self.focus, cx);
        self.tick_later(cx);
        cx.notify();
    }

    /// Closes the popup; with `restore`, focus goes back where it was
    /// before it opened, if the popup still has it.
    pub(crate) fn close(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.ticker = None;
        let back = self.restore.take().and_then(|focus| focus.upgrade());
        if restore
            && self.focus.contains_focused(window, cx)
            && let Some(back) = back
        {
            window.focus(&back, cx);
        }
        cx.notify();
    }

    /// The inbox changed: the bell follows, the selection stays on its
    /// notification or falls to the row in its place, and the buttons of
    /// notifications gone are forgotten.
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.sync_bell(cx);
        let rows = self.rows(cx);
        let at = self.selected_index(&rows);
        let selected = rows.get(at).map(|(_, item)| item.id);
        let ids: Vec<NotificationId> = rows.iter().map(|(_, item)| item.id).collect();
        self.selected = selected;
        self.selected_at = at;
        (self.buttons).retain(|key, _| key.notification().is_none_or(|id| ids.contains(&id)));
        self.tick_later(cx);
        cx.notify();
    }

    fn sync_bell(&self, cx: &mut App) {
        let count = self.layer.read(cx).inbox().needs_you_count();
        self.bar.update(cx, |bar, cx| bar.set_bell(count, cx)).ok();
    }

    /// While the popup is open, redraws it when the first of its ages
    /// steps on: past a minute, an hour or a day.
    fn tick_later(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            self.ticker = None;
            return;
        }
        let now = self.now(cx);
        let wait = (self.layer.read(cx).inbox().all().iter())
            .map(|item| until_next_age(now.duration_since(item.at).unwrap_or_default()))
            .min();
        self.ticker = wait.map(|wait| {
            cx.spawn(async move |view, cx| {
                cx.background_executor().timer(wait).await;
                view.update(cx, |view, cx| {
                    #[cfg(test)]
                    {
                        view.ticks += 1;
                    }
                    view.tick_later(cx);
                    cx.notify();
                })
                .ok();
            })
        });
    }

    /// Moves the selection `by` rows, staying on the list.
    fn move_selection(&mut self, by: isize, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        if rows.is_empty() {
            return;
        }
        let at = (self.selected_index(&rows))
            .saturating_add_signed(by)
            .min(rows.len() - 1);
        let child = child_index(&rows, at);
        self.selected = Some(rows[at].1.id);
        self.selected_at = at;
        self.scroll.scroll_to_item(child);
        cx.notify();
    }

    /// Selects notification `id`'s row, if it's there.
    fn select(&mut self, id: NotificationId, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        if let Some(at) = rows.iter().position(|(_, item)| item.id == id) {
            let child = child_index(&rows, at);
            self.selected = Some(id);
            self.selected_at = at;
            self.scroll.scroll_to_item(child);
            cx.notify();
        }
    }

    /// Acts on the selected row, if it needs you: runs its action, answers
    /// its question's one answer, or moves focus to its first button when
    /// it has more than one.
    fn activate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        let Some((Group::NeedsYou, item)) = rows.get(self.selected_index(&rows)) else {
            return;
        };
        let (id, action, answers) = (item.id, item.action.clone(), item.answers.len());
        match (action, answers) {
            (Some((_, command)), 0) => self.run(id, command, Source::Keyboard, window, cx),
            (None, 1) => self.answer(id, 0, window, cx),
            (None, 0) => {}
            (_, _) => {
                let first = self.handle(ButtonKey::Answer(id, 0), cx);
                window.focus(&first, cx);
            }
        }
    }

    /// The buttons in their tab order, as drawn: each Needs-you row's
    /// answers and action, then Clear earlier, then each row's dismiss in
    /// its place.
    fn tab_order(rows: &[(Group, &Notification)]) -> Vec<ButtonKey> {
        let mut order = Vec::new();
        let mut last = None;
        for (group, item) in rows {
            if *group == Group::Earlier && last != Some(Group::Earlier) {
                order.push(ButtonKey::ClearEarlier);
            }
            last = Some(*group);
            if *group == Group::NeedsYou {
                order.extend((0..item.answers.len()).map(|i| ButtonKey::Answer(item.id, i)));
                if item.action.is_some() {
                    order.push(ButtonKey::Action(item.id));
                }
            }
            order.push(ButtonKey::Dismiss(item.id));
        }
        order
    }

    /// Moves focus to the next button (`back`: the previous), from the
    /// popup to the selected row's first, and from either end back to the
    /// popup. The row of the button focused is selected.
    fn tab(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        let order = Self::tab_order(&rows);
        let selected = rows
            .get(self.selected_index(&rows))
            .map(|(_, item)| item.id);
        if order.is_empty() {
            return;
        }
        let current = (order.iter())
            .position(|key| (self.buttons.get(key)).is_some_and(|focus| focus.is_focused(window)));
        let next = match (current, back) {
            (None, false) => (order.iter())
                .position(|key| key.notification() == selected && selected.is_some())
                .or(Some(0)),
            (None, true) => Some(order.len() - 1),
            (Some(at), false) => (at + 1 < order.len()).then_some(at + 1),
            (Some(at), true) => at.checked_sub(1),
        };
        match next.map(|at| order[at]) {
            Some(key) => {
                let focus = self.handle(key, cx);
                window.focus(&focus, cx);
                if let Some(id) = key.notification() {
                    self.select(id, cx);
                }
            }
            None => window.focus(&self.focus, cx),
        }
        cx.notify();
    }

    /// Button `key`'s focus handle.
    fn handle(&mut self, key: ButtonKey, cx: &mut Context<Self>) -> FocusHandle {
        (self.buttons.entry(key))
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone()
    }

    /// Focus goes back to the popup from a button of its, which a change
    /// may take away: with nothing drawn focused, the popup would close.
    fn refocus(&self, window: &mut Window, cx: &mut App) {
        if self.focus.contains_focused(window, cx) && !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
    }

    /// Runs notification `id`'s action, `command`, from where focus was
    /// before the popup, which closes; the notification moves to Earlier.
    fn run(
        &mut self,
        id: NotificationId,
        command: CommandId,
        source: Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close(true, window, cx);
        toast::run_action(command, source, window, cx);
        (self.layer).update(cx, |layer, cx| {
            layer.resolve_notification(id, Resolution::Acted, cx)
        });
    }

    /// Answers notification `id`'s question with its answer `choice`, from
    /// where focus was before the popup, which closes; its toast goes and
    /// the notification moves to Earlier. If its toast is gone, so that
    /// there's nothing to answer, it's dismissed instead.
    fn answer(
        &mut self,
        id: NotificationId,
        choice: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close(true, window, cx);
        (self.layer).update(cx, |layer, cx| {
            if !layer.answer_notification(id, choice, cx) {
                layer.resolve_notification(id, Resolution::Dismissed, cx);
            }
        });
    }

    /// Dismisses row `id` of `group`: a Needs-you one moves to Earlier, an
    /// Earlier one goes.
    fn dismiss(
        &mut self,
        id: NotificationId,
        group: Group,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refocus(window, cx);
        self.layer.update(cx, |layer, cx| match group {
            Group::NeedsYou => layer.resolve_notification(id, Resolution::Dismissed, cx),
            Group::Earlier => layer.remove_notification(id, cx),
        });
    }

    fn clear_earlier(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refocus(window, cx);
        self.layer.update(cx, |layer, cx| layer.clear_earlier(cx));
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        if key == "tab" && !(modifiers.control || modifiers.alt || modifiers.platform) {
            self.tab(modifiers.shift, window, cx);
            cx.stop_propagation();
            return;
        }
        if modifiers.modified() {
            return;
        }
        // A focused button takes its own Enter.
        let on_popup = self.focus.is_focused(window);
        match key {
            "escape" => self.close(true, window, cx),
            "up" if on_popup => self.move_selection(-1, cx),
            "down" if on_popup => self.move_selection(1, cx),
            "enter" if on_popup => self.activate_selected(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// The palette's and the menu bar's keys, pressed in the popup: the
    /// shell beside it handles them, out of its reach, so the popup closes,
    /// focus goes back, and they run from there.
    fn forward_to_shell(
        &mut self,
        action: &dyn gpui::Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close(true, window, cx);
        window.dispatch_action(action.boxed_clone(), cx);
    }

    /// Group `group`'s header: its name, and for Earlier the Clear earlier
    /// button.
    fn header(&mut self, group: Group, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let colors = cx.theme().colors;
        let (name, prefix) = match group {
            Group::NeedsYou => (
                tr!("workbench-inbox-needs-you"),
                verbatim!("inbox-header-needs-you"),
            ),
            Group::Earlier => (
                tr!("workbench-inbox-earlier"),
                verbatim!("inbox-header-earlier"),
            ),
        };
        let clear = (group == Group::Earlier).then(|| {
            let view = cx.weak_entity();
            let focus = self.handle(ButtonKey::ClearEarlier, cx);
            let id = verbatim!("inbox-clear-earlier");
            div().debug_selector(move || id.to_owned()).child(
                Button::new(id, text!("workbench-inbox-clear-earlier"))
                    .small()
                    .ghost()
                    .focus_handle(&focus)
                    .on_click(move |_, window, cx| {
                        view.update(cx, |view, cx| view.clear_earlier(window, cx))
                            .ok();
                    }),
            )
        });
        div()
            .debug_selector(move || prefix.to_owned())
            .flex()
            .items_center()
            .justify_between()
            .h(px(28.))
            .px_3()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(colors.text.muted)
            .child(name)
            .children(clear)
    }

    /// The row for `item`, of `group`, selected or not. Its elements are
    /// named by the notification, `n` (`inbox-row-{n}` and the like).
    fn row(
        &mut self,
        group: Group,
        item: &Notification,
        selected: bool,
        now: SystemTime,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let colors = cx.theme().colors;
        let (icon, tint) = source_icon(item.source, item.level, &colors);
        let view = cx.weak_entity();
        let id = item.id;
        let n = id.value();
        let ground = match (group, item.source) {
            (Group::Earlier, _) => None,
            (Group::NeedsYou, NotificationSource::Agent) => Some(colors.agent.soft),
            (Group::NeedsYou, _) => Some(colors.state.selected),
        };
        let mut buttons = Vec::new();
        if group == Group::NeedsYou {
            for (i, title) in item.answers.iter().enumerate() {
                let focus = self.handle(ButtonKey::Answer(id, i), cx);
                let view = view.clone();
                let prefix = verbatim!("inbox-answer-");
                let name = SharedString::from(format!("{prefix}{n}-{i}"));
                let selector = name.to_string();
                buttons.push(
                    div().flex_none().debug_selector(move || selector).child(
                        Button::new(name, Text::verbatim(title.clone()))
                            .small()
                            .focus_handle(&focus)
                            .on_click(move |_, window, cx| {
                                view.update(cx, |view, cx| view.answer(id, i, window, cx))
                                    .ok();
                            }),
                    ),
                );
            }
            if let Some((label, command)) = item.action.clone() {
                let focus = self.handle(ButtonKey::Action(id), cx);
                let view = view.clone();
                let prefix = verbatim!("inbox-action-");
                let name = SharedString::from(format!("{prefix}{n}"));
                let selector = name.to_string();
                buttons.push(
                    div().flex_none().debug_selector(move || selector).child(
                        Button::new(name, Text::verbatim(label))
                            .small()
                            .focus_handle(&focus)
                            .on_click(move |event, window, cx| {
                                let source = match event.is_keyboard() {
                                    true => Source::Keyboard,
                                    false => Source::Mouse,
                                };
                                let command = command.clone();
                                view.update(cx, |view, cx| {
                                    view.run(id, command, source, window, cx)
                                })
                                .ok();
                            }),
                    ),
                );
            }
        }
        let focus = self.handle(ButtonKey::Dismiss(id), cx);
        let prefix = verbatim!("inbox-dismiss-");
        let dismiss = IconButton::new(
            SharedString::from(format!("{prefix}{n}")),
            Icon::X,
            tr!("workbench-inbox-dismiss"),
            &fe_ui::ui_theme(cx),
        )
        .focus_handle(&focus)
        .on_click(move |_, window, cx| {
            view.update(cx, |view, cx| view.dismiss(id, group, window, cx))
                .ok();
        });
        let count = (item.count > 1).then(|| {
            div()
                .flex_none()
                .text_color(colors.text.muted)
                .child(tr!("workbench-toast-count", count = item.count))
        });
        let detail = item.detail.clone().map(|detail| {
            (div().text_xs().line_clamp(1).text_ellipsis())
                .text_color(colors.text.muted)
                .child(detail)
        });
        let age = age(item.at, now);
        #[cfg(test)]
        self.drawn_ages.push((id, age.clone()));
        let states = fe_ui::States {
            selected: Some(selected),
            description: item.detail.clone().map(SharedString::from),
            ..fe_ui::States::default()
        };
        let prefix = verbatim!("inbox-row-");
        let row = div().id(SharedString::from(format!("{prefix}{n}")));
        let title = SharedString::from(item.title.clone());
        let line = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                svg()
                    .debug_selector(move || {
                        let prefix = verbatim!("inbox-icon-");
                        format!("{prefix}{n}")
                    })
                    .path(icon.path())
                    .size(px(16.))
                    .flex_none()
                    .text_color(tint),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(div().flex_1().min_w_0().truncate().child(title.clone()))
                            .children(count),
                    )
                    .children(detail),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.text.faint)
                    .child(age),
            )
            .child(dismiss);
        // Under the text, from its left edge: the icon's 16 px and the gap.
        let buttons = (!buttons.is_empty()).then(|| {
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .pl(px(24.))
                .children(buttons)
        });
        fe_ui::node_with(row, window, cx, fe_ui::Role::ListItem, title, states)
            .debug_selector(move || format!("{prefix}{n}"))
            .flex()
            .flex_col()
            .gap_1()
            .mx_1()
            .px_2()
            .py_1()
            .rounded(radius::CONTROL)
            .border_1()
            .border_color(match selected {
                true => colors.state.focus_ring,
                false => gpui::transparent_black(),
            })
            .when_some(ground, |row, ground| row.bg(ground))
            .child(line)
            .children(buttons)
    }
}

/// Row `at`'s place among the list's children, which hold a header before
/// each group's rows.
fn child_index(rows: &[(Group, &Notification)], at: usize) -> usize {
    let headers = (rows.iter().take(at + 1))
        .zip(std::iter::once(None).chain(rows.iter().map(|(group, _)| Some(*group))))
        .filter(|((group, _), before)| *before != Some(*group))
        .count();
    at + headers
}

/// How long until an age of `age` shows the next step: the next whole
/// minute under an hour, hour under a day, else day.
fn until_next_age(age: Duration) -> Duration {
    let seconds = age.as_secs();
    let unit = match seconds {
        0..3_600 => 60,
        3_600..86_400 => 3_600,
        _ => 86_400,
    };
    Duration::from_secs(seconds - seconds % unit + unit).saturating_sub(age)
}

impl Render for NotificationInboxView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        self.drawn_ages.clear();
        if !self.open {
            return div().into_any_element();
        }
        let colors = cx.theme().colors;
        let look = card_look(&colors);
        let now = self.now(cx);
        let rows: Vec<(Group, Notification)> = (self.rows(cx).into_iter())
            .map(|(group, item)| (group, item.clone()))
            .collect();
        let selected = {
            let refs: Vec<(Group, &Notification)> =
                rows.iter().map(|(group, item)| (*group, item)).collect();
            self.selected_index(&refs)
        };
        let mut list = Vec::new();
        let mut last = None;
        for (at, (group, item)) in rows.iter().enumerate() {
            if last != Some(*group) {
                list.push(self.header(*group, cx).into_any_element());
                last = Some(*group);
            }
            list.push(
                self.row(*group, item, at == selected, now, window, cx)
                    .into_any_element(),
            );
        }
        let empty = rows.is_empty().then(|| {
            div()
                .debug_selector(|| verbatim!("inbox-empty").to_owned())
                .px_3()
                .py_4()
                .text_color(colors.text.muted)
                .child(tr!("workbench-inbox-empty"))
        });
        let popup = div().id(verbatim!("notification-inbox"));
        fe_ui::node(
            popup,
            window,
            cx,
            fe_ui::Role::Dialog,
            tr!("workbench-inbox-title"),
        )
        .debug_selector(|| verbatim!("notification-inbox").to_owned())
        .key_context(KEY_CONTEXT)
        .track_focus(&self.focus)
        .on_key_down(cx.listener(Self::on_key_down))
        .on_action(cx.listener(
            |view, action: &kiln_shell::command_palette::Toggle, window, cx| {
                view.forward_to_shell(action, window, cx)
            },
        ))
        .on_action(
            cx.listener(|view, action: &kiln_shell::menu_bar::Open, window, cx| {
                view.forward_to_shell(action, window, cx)
            }),
        )
        .occlude()
        .w(px(WIDTH))
        .max_h(px(MAX_HEIGHT))
        .flex()
        .flex_col()
        .rounded(look.radius)
        .border_1()
        .border_color(colors.border.default)
        .bg(look.fill)
        .shadow(vec![look.shadow])
        .text_sm()
        .text_color(colors.text.primary)
        .child(
            div()
                .id(verbatim!("notification-inbox-rows"))
                .track_scroll(&self.scroll)
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_0p5()
                .pb_1()
                .children(list)
                .children(empty),
        )
        .into_any_element()
    }
}

/// A notification's icon and its colour: the agent's robot in the agent's
/// colour; Git's branch, a task's run and a language server's server in
/// the level's colour; the app's own, the level's icon.
pub(crate) fn source_icon(
    source: NotificationSource,
    level: Level,
    colors: &fe_ui::Tokens<Hsla>,
) -> (Icon, Hsla) {
    let (level_glyph, level_colour) = level_icon(level, colors);
    match source {
        NotificationSource::Agent => (Icon::Bot, colors.agent.base),
        NotificationSource::Git => (Icon::GitBranch, level_colour),
        NotificationSource::Task => (Icon::ListVideo, level_colour),
        NotificationSource::Lsp => (Icon::Server, level_colour),
        NotificationSource::App => (level_glyph, level_colour),
    }
}

/// How long ago `at` was at `now`: now, then minutes, hours and days.
pub(crate) fn age(at: SystemTime, now: SystemTime) -> SharedString {
    let seconds = now.duration_since(at).unwrap_or_default().as_secs();
    match seconds {
        0..60 => tr!("workbench-inbox-age-now"),
        60..3_600 => tr!("workbench-inbox-age-minutes", count = seconds / 60),
        3_600..86_400 => tr!("workbench-inbox-age-hours", count = seconds / 3_600),
        _ => tr!("workbench-inbox-age-days", count = seconds / 86_400),
    }
}

/// Where the workbench draws the popup: at the window's lower right, above
/// the status bar's top, `bottom` from the window's bottom.
pub(crate) fn overlay(
    view: &Entity<NotificationInboxView>,
    bottom: gpui::Pixels,
    cx: &App,
) -> Option<impl IntoElement + use<>> {
    view.read(cx).is_open().then(|| {
        div()
            .absolute()
            .right(px(MARGIN))
            .bottom(bottom)
            .child(view.clone())
    })
}

impl crate::Workbench {
    /// For the screenshot audit (`examples/screenshots/audit/inbox.rs`):
    /// shows a toast as [`crate::Workbench::notify`] does, raised by
    /// `source` (`agent`, `task`, `git`, `lsp`, else the app), with
    /// `action`'s button running its command if given.
    #[doc(hidden)]
    pub fn notify_from(
        cx: &mut App,
        level: log::Level,
        source: &str,
        message: String,
        detail: Option<String>,
        action: Option<(String, CommandId)>,
    ) {
        let level = match level {
            log::Level::Error => Level::Error,
            log::Level::Warn => Level::Warning,
            _ => Level::Info,
        };
        let source = match source {
            "agent" => NotificationSource::Agent,
            "task" => NotificationSource::Task,
            "git" => NotificationSource::Git,
            "lsp" => NotificationSource::Lsp,
            _ => NotificationSource::App,
        };
        let mut shown = toast::Toast::new(level, message).source(source);
        shown.detail = detail;
        shown.action = action;
        toast::show(cx, shown);
    }
}
