//! Toasts: app-wide notifications, shown at the bottom right of a window,
//! above the status bar, that never take focus.
//!
//! [`show`] sends a [`Toast`] (info, warning or error, a message, an
//! optional detail and an optional action) from anywhere to the active
//! window, else the first: each [`Workbench`](crate::Workbench) keeps a
//! [`ToastLayer`] that draws them. At most [`MAX_SHOWN`] show, newest at
//! the bottom, and more wait their turn; one identical to a toast still
//! shown or waiting counts up (`×3`) instead. Info and warnings go after
//! [`LIFETIME`], errors stay until dismissed, and a toast's timer stops
//! while the pointer is over it. Each has a close button;
//! `toast.dismiss_all` ("Dismiss Notifications") dismisses every one.
//! Clicking an action runs its command where focus is, as the command
//! palette would, and dismisses the toast.
//!
//! A toast can instead ask a question: [`Choices`], a button per title and
//! a reply that gets the index clicked, or none if the toast goes any other
//! way (closed, dismissed, dropped from a full queue, or its window
//! closing). It stays until then, whatever its level, and never merges
//! with another.
//!
//! Showing, hovering, clicking or dismissing a toast leaves focus where it
//! is, and with it the editor's vim mode: a toast has no focus handle, and
//! occludes what's under it, so a click on it focuses nothing beneath.
//!
//! The queue and its timers are [`ToastQueue`], which knows nothing of
//! GPUI: it's told the time.
//!
//! `toast.dismiss_all` is a typed command on the workbench.

use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::Rc,
    time::{Duration, Instant},
};

use fe_command::{CommandId, Invocation, Source};
use fe_ui::{ActiveTheme as _, Button, Icon, IconButton, UiTheme};
use gpui::{
    App, Context, Entity, HoverListenerMode, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Task, WeakEntity, Window, WindowId, div, px,
};

/// How many toasts show at once.
pub(crate) const MAX_SHOWN: usize = 3;

/// How many toasts wait for room, at most: past that, the oldest waiting
/// is dropped.
pub(crate) const MAX_WAITING: usize = 64;

/// How long info and warnings show, not counting time under the pointer.
pub(crate) const LIFETIME: Duration = Duration::from_secs(6);

/// A toast's width.
const WIDTH: f32 = 360.;

/// How bad it is: info and warnings go by themselves, errors stay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Level {
    Info,
    Warning,
    Error,
}

/// The question a toast asks: a button per title, and who to tell which
/// was chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Choices {
    titles: Vec<String>,
    reply: Rc<Reply>,
}

impl Choices {
    #[cfg(test)]
    pub(crate) fn titles(&self) -> &[String] {
        &self.titles
    }

    /// Answers `choice` (an index into the titles, or none), unless it was
    /// answered already.
    fn answer(&self, choice: Option<usize>) {
        self.reply.answer(choice);
    }
}

/// A reply to a question, which is answered once: with the choice, or none
/// when the last handle to it is dropped unanswered.
struct Reply(RefCell<Option<Answer>>);

type Answer = Box<dyn FnOnce(Option<usize>)>;

impl Reply {
    fn answer(&self, choice: Option<usize>) {
        let reply = self.0.borrow_mut().take();
        if let Some(reply) = reply {
            reply(choice);
        }
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        self.answer(None);
    }
}

impl PartialEq for Reply {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for Reply {}

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Reply")
    }
}

/// A notification to show: see [`show`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Toast {
    pub level: Level,
    pub message: String,
    /// A second, quieter line.
    pub detail: Option<String>,
    /// A button's label, and the command it runs.
    pub action: Option<(String, CommandId)>,
    /// A question, with a button per answer.
    pub choices: Option<Choices>,
}

impl Toast {
    pub(crate) fn new(level: Level, message: impl Into<String>) -> Self {
        Self {
            level,
            message: message.into(),
            detail: None,
            action: None,
            choices: None,
        }
    }

    pub(crate) fn info(message: impl Into<String>) -> Self {
        Self::new(Level::Info, message)
    }

    pub(crate) fn warning(message: impl Into<String>) -> Self {
        Self::new(Level::Warning, message)
    }

    pub(crate) fn error(message: impl Into<String>) -> Self {
        Self::new(Level::Error, message)
    }

    pub(crate) fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    #[cfg_attr(not(test), allow(dead_code, reason = "no caller outside tests yet"))]
    pub(crate) fn action(mut self, label: impl Into<String>, command: CommandId) -> Self {
        self.action = Some((label.into(), command));
        self
    }

    /// Asks a question: a button per title, in the place of the action's.
    /// `reply` gets the index of the one clicked, or none if the toast goes
    /// without one. The toast stays until then.
    pub(crate) fn choices(
        mut self,
        titles: Vec<String>,
        reply: impl FnOnce(Option<usize>) + 'static,
    ) -> Self {
        self.choices = Some(Choices {
            titles,
            reply: Rc::new(Reply(RefCell::new(Some(Box::new(reply))))),
        });
        self
    }
}

/// A toast in a queue, for as long as it's there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ToastId(u64);

/// When a shown toast goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Timer {
    /// Not by itself: an error.
    Stays,
    /// At this time.
    Runs(Instant),
    /// This long after the pointer leaves it.
    Paused(Duration),
}

/// A toast in a [`ToastQueue`]: shown or waiting.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub id: ToastId,
    pub toast: Toast,
    /// How many times it was shown: 1, or more for an identical toast
    /// shown again while this one was there.
    pub count: usize,
    timer: Timer,
}

impl Entry {
    /// Whether it's paused under the pointer.
    #[cfg(test)]
    pub(crate) fn hovered(&self) -> bool {
        matches!(self.timer, Timer::Paused(_))
    }

    /// Its timer from `now`, or with its time in full while it's under the
    /// pointer.
    fn start(&mut self, now: Instant) {
        self.timer = match (self.toast.level, self.timer) {
            _ if self.toast.choices.is_some() => Timer::Stays,
            (Level::Error, _) => Timer::Stays,
            (_, Timer::Paused(_)) => Timer::Paused(LIFETIME),
            _ => Timer::Runs(now + LIFETIME),
        };
    }
}

/// The toasts of a window, with no GPUI: those shown, oldest first, and
/// those waiting for room, each told the time when it matters.
#[derive(Debug, Default)]
pub(crate) struct ToastQueue {
    shown: Vec<Entry>,
    waiting: VecDeque<Entry>,
    next_id: u64,
}

impl ToastQueue {
    /// Shows `toast` at `now`, at the bottom, or has it wait if
    /// [`MAX_SHOWN`] show, dropping the oldest waiting if [`MAX_WAITING`]
    /// already do. One identical to a toast shown or waiting counts that one
    /// up instead, a shown one's timer starting again.
    pub(crate) fn push(&mut self, toast: Toast, now: Instant) -> ToastId {
        let merges = toast.choices.is_none();
        if let Some(entry) = (self.shown.iter_mut()).find(|entry| merges && entry.toast == toast) {
            entry.count += 1;
            entry.start(now);
            return entry.id;
        }
        if let Some(entry) = (self.waiting.iter_mut()).find(|entry| merges && entry.toast == toast)
        {
            entry.count += 1;
            return entry.id;
        }
        let id = ToastId(self.next_id);
        self.next_id += 1;
        let entry = Entry {
            id,
            toast,
            count: 1,
            timer: Timer::Stays,
        };
        self.waiting.push_back(entry);
        if self.waiting.len() > MAX_WAITING
            && let Some(dropped) = self.waiting.pop_front()
        {
            log::info!("too many notifications, dropped: {}", dropped.toast.message);
        }
        self.fill(now);
        id
    }

    /// The toasts shown, oldest (top) first.
    #[cfg(test)]
    pub(crate) fn shown(&self) -> &[Entry] {
        &self.shown
    }

    /// How many wait for room.
    #[cfg(test)]
    pub(crate) fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Dismisses toast `id`, shown or waiting, the next waiting taking a
    /// shown one's place. False if it's not there.
    pub(crate) fn dismiss(&mut self, id: ToastId, now: Instant) -> bool {
        let before = self.shown.len() + self.waiting.len();
        self.shown.retain(|entry| entry.id != id);
        self.waiting.retain(|entry| entry.id != id);
        self.fill(now);
        self.shown.len() + self.waiting.len() != before
    }

    /// Dismisses every toast, waiting ones too. False if there were none.
    pub(crate) fn dismiss_all(&mut self) -> bool {
        let any = !self.shown.is_empty() || !self.waiting.is_empty();
        self.shown.clear();
        self.waiting.clear();
        any
    }

    /// The pointer came onto (`true`) or left shown toast `id` at `now`:
    /// its timer stops, with the time it had left, or runs on.
    pub(crate) fn set_hovered(&mut self, id: ToastId, hovered: bool, now: Instant) {
        let Some(entry) = self.shown.iter_mut().find(|entry| entry.id == id) else {
            return;
        };
        entry.timer = match (entry.timer, hovered) {
            (Timer::Runs(until), true) => Timer::Paused(until.saturating_duration_since(now)),
            (Timer::Paused(left), false) => Timer::Runs(now + left),
            (timer, _) => timer,
        };
    }

    /// Dismisses the toasts whose time is up at `now`, waiting ones taking
    /// their place. Whether any went.
    pub(crate) fn expire(&mut self, now: Instant) -> bool {
        let before = self.shown.len();
        self.shown
            .retain(|entry| !matches!(entry.timer, Timer::Runs(until) if until <= now));
        let expired = self.shown.len() != before;
        self.fill(now);
        expired
    }

    /// When the next toast's time is up, if one's is running.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        (self.shown.iter())
            .filter_map(|entry| match entry.timer {
                Timer::Runs(until) => Some(until),
                Timer::Stays | Timer::Paused(_) => None,
            })
            .min()
    }

    /// Shows waiting toasts while there's room, their timers from `now`.
    fn fill(&mut self, now: Instant) {
        while self.shown.len() < MAX_SHOWN
            && let Some(mut entry) = self.waiting.pop_front()
        {
            entry.start(now);
            self.shown.push(entry);
        }
    }
}

/// Shows `toast` in the active window, else the first one open, from
/// anywhere. With no workbench window open, it's only logged.
pub(crate) fn show(cx: &mut App, toast: Toast) {
    let Some(layer) = layer_for(cx.active_window().map(|window| window.window_id()), cx) else {
        log::info!("no window to show a toast in: {}", toast.message);
        return;
    };
    layer.update(cx, |layer, cx| layer.push(toast, cx));
}

/// The layer of window `active` if it has one, else the first one open.
fn layer_for(active: Option<WindowId>, cx: &mut App) -> Option<Entity<ToastLayer>> {
    if !cx.has_global::<Layers>() {
        return None;
    }
    let open: Vec<WindowId> = (cx.windows().iter())
        .map(|window| window.window_id())
        .collect();
    let layers = &mut cx.global_mut::<Layers>().0;
    // A layer can outlive its window for a moment as it closes.
    layers.retain(|(window, layer)| open.contains(window) && layer.upgrade().is_some());
    let live = |(_, layer): &(WindowId, WeakEntity<ToastLayer>)| layer.upgrade();
    (layers.iter())
        .find(|(window, _)| Some(*window) == active)
        .and_then(live)
        .or_else(|| layers.iter().find_map(live))
}

/// Each window's layer, oldest window first.
#[derive(Default)]
struct Layers(Vec<(WindowId, WeakEntity<ToastLayer>)>);

impl gpui::Global for Layers {}

/// A window's toasts, drawn as a column the workbench places at the
/// bottom right, with the timer that dismisses them.
pub(crate) struct ToastLayer {
    queue: ToastQueue,
    /// Wakes the layer when the next toast's time is up.
    timer: Option<Task<()>>,
    /// The column's scroll, when the toasts are taller than the window.
    scroll: ScrollHandle,
    ui: UiTheme,
}

impl ToastLayer {
    /// The layer for `window`, which [`show`] sends its toasts to while it's
    /// the active window, or the first one open.
    pub(crate) fn new(ui: UiTheme, window: &Window, cx: &mut Context<Self>) -> Self {
        let id = window.window_handle().window_id();
        let layer = cx.weak_entity();
        cx.default_global::<Layers>().0.push((id, layer));
        Self {
            queue: ToastQueue::default(),
            timer: None,
            scroll: ScrollHandle::new(),
            ui,
        }
    }

    #[cfg(test)]
    pub(crate) fn queue(&self) -> &ToastQueue {
        &self.queue
    }

    fn push(&mut self, toast: Toast, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        self.queue.push(toast, now);
        // The newest, at the bottom, shows even when they don't all fit.
        self.scroll.scroll_to_bottom();
        self.changed(cx);
    }

    pub(crate) fn dismiss(&mut self, id: ToastId, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        if self.queue.dismiss(id, now) {
            self.changed(cx);
        }
    }

    pub(crate) fn dismiss_all(&mut self, cx: &mut Context<Self>) {
        if self.queue.dismiss_all() {
            self.changed(cx);
        }
    }

    fn set_hovered(&mut self, id: ToastId, hovered: bool, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        self.queue.set_hovered(id, hovered, now);
        self.changed(cx);
    }

    /// Dismisses what's due, sets the timer for what's next, and redraws.
    fn changed(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        self.queue.expire(now);
        self.timer = self.queue.next_deadline().map(|due| {
            let wait = due.saturating_duration_since(now);
            cx.spawn(async move |layer, cx| {
                cx.background_executor().timer(wait).await;
                layer.update(cx, |layer, cx| layer.changed(cx)).ok();
            })
        });
        cx.notify();
    }

    /// The card for `entry`.
    fn render_toast(
        &self,
        entry: &Entry,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let colors = cx.theme().colors;
        let level = match entry.toast.level {
            Level::Info => colors.status.info,
            Level::Warning => colors.status.warning,
            Level::Error => colors.status.error,
        };
        let id = entry.id;
        let ix = id.0;
        let layer = cx.weak_entity();
        let close = IconButton::new(
            SharedString::from(format!("toast-close-{ix}")),
            Icon::X,
            &self.ui,
        )
        .on_click({
            let layer = layer.clone();
            move |_, _, cx| {
                layer.update(cx, |layer, cx| layer.dismiss(id, cx)).ok();
            }
        });
        let count = (entry.count > 1).then(|| {
            div()
                .flex_none()
                .text_color(colors.text.muted)
                .child(format!("×{}", entry.count))
        });
        let detail = entry.toast.detail.clone().map(|detail| {
            (div().text_xs().line_clamp(4))
                .text_color(colors.text.muted)
                .child(detail)
        });
        let choices = entry.toast.choices.clone().map(|choices| {
            let buttons = choices
                .titles
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, title)| {
                    let (layer, choices) = (layer.clone(), choices.clone());
                    div()
                        .debug_selector(move || format!("toast-choice-{ix}-{i}"))
                        .child(
                            Button::new(
                                SharedString::from(format!("toast-choice-{ix}-{i}")),
                                title,
                            )
                            .small()
                            .on_click(move |_, _, cx| {
                                choices.answer(Some(i));
                                layer.update(cx, |layer, cx| layer.dismiss(id, cx)).ok();
                            }),
                        )
                });
            div()
                .self_end()
                .flex()
                .flex_wrap()
                .justify_end()
                .gap_1()
                .children(buttons)
        });
        let action = entry.toast.action.clone().map(|(label, command)| {
            let layer = layer.clone();
            div()
                .debug_selector(move || format!("toast-action-{ix}"))
                .self_end()
                .flex()
                .child(Button::new(("toast-action", ix), label).small().on_click(
                    move |_, window, cx| {
                        run_action(command.clone(), window, cx);
                        layer.update(cx, |layer, cx| layer.dismiss(id, cx)).ok();
                    },
                ))
        });
        let hovered =
            cx.listener(move |layer, hovered: &bool, _, cx| layer.set_hovered(id, *hovered, cx));
        div()
            .id(("toast", ix))
            .debug_selector(move || format!("toast-{ix}"))
            .on_hover(hovered)
            .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
            .w(px(WIDTH))
            .max_w(window.viewport_size().width - px(24.))
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .pl_3()
            .rounded_md()
            .border_1()
            .border_l_4()
            .border_color(colors.border.default)
            .bg(colors.surface.overlay)
            .shadow_md()
            .text_sm()
            .text_color(colors.text.primary)
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(
                        div()
                            .mt(px(5.))
                            .flex_none()
                            .size(px(8.))
                            .rounded_full()
                            .bg(level),
                    )
                    .child(
                        (div().flex_1().min_w_0().line_clamp(8)).child(entry.toast.message.clone()),
                    )
                    .children(count)
                    .child(close),
            )
            .children(detail)
            .children(action)
            .children(choices)
    }
}

/// Runs `command` from where `window`'s focus is, in its context, as the
/// command palette does once it's given focus back.
fn run_action(command: CommandId, window: &mut Window, cx: &mut App) {
    let stack = kiln_platform::keys::context_stack(&window.context_stack(), cx);
    let invocation = Invocation::new(command)
        .source(Source::Mouse)
        .context(stack);
    window.defer(cx, move |window, cx| {
        if let Err(error) =
            kiln_platform::command_actions::dispatch_invocation(invocation, window, cx)
        {
            log::warn!("{error}");
        }
    });
}

impl Render for ToastLayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toasts: Vec<_> = (self.queue.shown.iter())
            .map(|entry| self.render_toast(entry, window, cx))
            .collect();
        div()
            .id("toasts")
            .debug_selector(|| "toasts".into())
            // So a click on it reaches nothing under it, which could take
            // focus: not hitboxes beneath, nor window-wide listeners such
            // as the menu bar's, which closes on any press.
            .occlude()
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            // No taller than the room the workbench gives it, scrolling
            // past that, so each toast's buttons can be reached.
            .max_h_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .items_end()
            .gap_2()
            .children(toasts)
    }
}

#[cfg(test)]
#[path = "toast_tests.rs"]
mod toast_tests;

#[cfg(test)]
#[path = "toast_routing_tests.rs"]
mod toast_routing_tests;
