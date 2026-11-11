//! Toasts: app-wide notifications, shown at the bottom right of a window,
//! above the status bar, that take focus only when asked to.
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
//! is, and with it the editor's vim mode: a click on a toast's button
//! doesn't focus it, and the toast occludes what's under it, so a click on
//! it focuses nothing beneath.
//!
//! From the keyboard, `toast.focus` ("Toast: Focus") focuses the newest
//! toast's first button, in its key context `toast`: Tab and Shift-Tab
//! (`toast.next_button`, `toast.previous_button`) move between its buttons,
//! answers or action first and the close button last; Enter or Space
//! presses the focused one, and Escape (`toast.dismiss`) dismisses it,
//! answering a question with none. The focused button draws the focus
//! ring, and the toast's timer stops while it has focus. Once the toast
//! goes, focus goes back where it was before `toast.focus`.
//!
//! The queue and its timers are [`ToastQueue`], which knows nothing of
//! GPUI: it's told the time.
//!
//! `toast.dismiss_all` and the focus commands are typed commands on the
//! workbench.

use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::Rc,
    time::{Duration, Instant},
};

use fe_command::{CommandId, Invocation, Source};
use fe_ui::{ActiveTheme as _, Button, Icon, IconButton};
use gpui::{
    AnyWindowHandle, App, Context, Entity, FocusHandle, HoverListenerMode, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity, WeakFocusHandle,
    Window, WindowId, div, prelude::FluentBuilder as _, px,
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

    /// Answers `choice`, as clicking its button does.
    #[cfg(test)]
    pub(crate) fn choose(&self, choice: usize) {
        self.answer(Some(choice));
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
    /// Whether the pointer is over it, which stops its timer as keyboard
    /// focus does ([`ToastQueue::set_held`]).
    under_pointer: bool,
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
    /// The toast with keyboard focus, whose timer stays stopped until it
    /// loses it, whatever the pointer does.
    held: Option<ToastId>,
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
            under_pointer: false,
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
    /// its timer stops, with the time it had left, or runs on unless the
    /// toast has keyboard focus.
    pub(crate) fn set_hovered(&mut self, id: ToastId, hovered: bool, now: Instant) {
        if let Some(entry) = self.shown.iter_mut().find(|entry| entry.id == id) {
            entry.under_pointer = hovered;
        }
        self.pause(id, now);
    }

    /// Holds shown toast `id`, which has keyboard focus, at `now`: its timer
    /// stops as it does under the pointer, until it's let go (`None`) and
    /// runs on unless the pointer is over it.
    pub(crate) fn set_held(&mut self, id: Option<ToastId>, now: Instant) {
        let old = std::mem::replace(&mut self.held, id);
        for id in old.into_iter().chain(id) {
            self.pause(id, now);
        }
    }

    /// Stops toast `id`'s timer, with the time it had left, while the
    /// pointer is over it or it has keyboard focus, else runs it on.
    fn pause(&mut self, id: ToastId, now: Instant) {
        let held = self.held == Some(id);
        let Some(entry) = self.shown.iter_mut().find(|entry| entry.id == id) else {
            return;
        };
        entry.timer = match (entry.timer, entry.under_pointer || held) {
            (Timer::Runs(until), true) => Timer::Paused(until.saturating_duration_since(now)),
            (Timer::Paused(left), false) => Timer::Runs(now + left),
            (timer, _) => timer,
        };
    }

    /// The newest toast shown: the bottom one.
    pub(crate) fn newest(&self) -> Option<&Entry> {
        self.shown.last()
    }

    /// Shown toast `id`.
    fn shown_entry(&self, id: ToastId) -> Option<&Entry> {
        self.shown.iter().find(|entry| entry.id == id)
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
    /// The toast given keyboard focus, from `toast.focus` until it goes or
    /// focus is given back: focus may leave it meanwhile, for a picker
    /// opened from it, say, and come back to the same button.
    focused: Option<Focused>,
    /// The focused toast's card, in key context `toast`, which holds its
    /// buttons.
    card: FocusHandle,
    /// The focused toast's buttons' focus, in its tab order: as many as
    /// the most buttons a focused toast has had.
    buttons: Vec<FocusHandle>,
    /// Where focus was before the last `toast.focus`, kept once its toast
    /// has gone: a picker opened from the toast may give focus back to the
    /// toast's button after that, when it's no longer drawn, and then focus
    /// goes here.
    fallback: Option<WeakFocusHandle>,
    window: AnyWindowHandle,
    _focus_in: Subscription,
    _focus_out: Subscription,
    _focus_lost: Subscription,
}

/// The toast with keyboard focus, and where focus was before.
struct Focused {
    id: ToastId,
    restore: Option<WeakFocusHandle>,
}

/// A toast's button, in its tab order: its answers or its action, then
/// its close button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToastButton {
    Choice(usize),
    Action,
    Close,
}

impl Entry {
    /// Its buttons, in tab order.
    pub(crate) fn buttons(&self) -> Vec<ToastButton> {
        let answers = self.toast.choices.as_ref().map_or(0, |c| c.titles.len());
        (0..answers)
            .map(ToastButton::Choice)
            .chain(self.toast.action.is_some().then_some(ToastButton::Action))
            .chain([ToastButton::Close])
            .collect()
    }
}

impl ToastLayer {
    /// The layer for `window`, which [`show`] sends its toasts to while it's
    /// the active window, or the first one open.
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let handle = window.window_handle();
        let layer = cx.weak_entity();
        cx.default_global::<Layers>()
            .0
            .push((handle.window_id(), layer));
        let card = cx.focus_handle();
        let focus_in = cx.on_focus_in(&card, window, |layer, _, cx| {
            if let Some(id) = layer.focused.as_ref().map(|focused| focused.id) {
                layer
                    .queue
                    .set_held(Some(id), cx.background_executor().now());
                layer.changed(cx);
            }
        });
        let focus_out = cx.on_focus_out(&card, window, |layer, _, window, cx| {
            layer.focus_left(window, cx);
        });
        let focus_lost = cx.on_focus_lost(window, |layer, window, cx| {
            layer.focus_lost(window, cx);
        });
        Self {
            queue: ToastQueue::default(),
            timer: None,
            scroll: ScrollHandle::new(),
            focused: None,
            card,
            buttons: Vec::new(),
            fallback: None,
            window: handle,
            _focus_in: focus_in,
            _focus_out: focus_out,
            _focus_lost: focus_lost,
        }
    }

    /// Gives keyboard focus to the newest toast's first button, keeping
    /// where focus was to give it back once the toast goes. False, doing
    /// nothing, with no toast shown.
    pub(crate) fn focus_newest(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(newest) = self.queue.newest() else {
            return false;
        };
        let (id, count) = (newest.id, newest.buttons().len());
        let restore = match self.focused.take() {
            // Focus is in a toast already: where it was before that.
            Some(focused) if self.holds_focus(window, cx) => focused.restore,
            _ => window.focused(cx).map(|focus| focus.downgrade()),
        };
        self.fallback = restore.clone();
        self.focused = Some(Focused { id, restore });
        while self.buttons.len() < count {
            self.buttons.push(cx.focus_handle().tab_stop(true));
        }
        self.queue
            .set_held(Some(id), cx.background_executor().now());
        // The newest, at the bottom, in view.
        self.scroll.scroll_to_bottom();
        window.focus(&self.buttons[0], cx);
        self.changed(cx);
        true
    }

    /// The focused toast and its button with focus: which, and its place
    /// in the toast's tab order.
    fn focused_button(&self, window: &Window) -> Option<(ToastId, usize, Vec<ToastButton>)> {
        let id = self.focused.as_ref()?.id;
        let buttons = self.queue.shown_entry(id)?.buttons();
        let at =
            (self.buttons.iter().take(buttons.len())).position(|focus| focus.is_focused(window))?;
        Some((id, at, buttons))
    }

    /// Moves focus to the focused toast's next button, or with `back` its
    /// previous one, wrapping at the ends.
    pub(crate) fn move_focus(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, at, buttons)) = self.focused_button(window) else {
            // On the card itself: its first button, or its last going back.
            if self.holds_focus(window, cx)
                && let Some(entry) =
                    (self.focused.as_ref()).and_then(|f| self.queue.shown_entry(f.id))
            {
                let to = if back { entry.buttons().len() - 1 } else { 0 };
                window.focus(&self.buttons[to], cx);
                cx.notify();
            }
            return;
        };
        let count = buttons.len();
        let to = if back {
            (at + count - 1) % count
        } else {
            (at + 1) % count
        };
        window.focus(&self.buttons[to], cx);
        cx.notify();
    }

    /// Dismisses the focused toast, answering a question with none, and
    /// gives focus back where it was.
    pub(crate) fn dismiss_focused(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.focused.as_ref().map(|focused| focused.id) else {
            return;
        };
        self.give_back_focus(window, cx);
        self.dismiss(id, cx);
    }

    /// Lets go of the focused toast and puts focus back where it was
    /// before `toast.focus`, if that's still there.
    fn give_back_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(focused) = self.focused.take() else {
            return;
        };
        self.queue.set_held(None, cx.background_executor().now());
        if let Some(restore) = focused.restore.and_then(|restore| restore.upgrade()) {
            window.focus(&restore, cx);
        }
        self.changed(cx);
    }

    /// Focus landed on nothing drawn: if on the toast's card or one of its
    /// buttons, as a picker opened from a toast that has since gone gives
    /// it back to them, it goes where it was before `toast.focus`. Focus
    /// lost elsewhere is someone else's.
    fn focus_lost(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ours = window
            .focused(cx)
            .is_some_and(|focus| focus == self.card || self.buttons.contains(&focus));
        if !ours {
            return;
        }
        let shown = (self.focused.as_ref()).is_some_and(|f| self.queue.shown_entry(f.id).is_some());
        if shown {
            return;
        }
        self.focused = None;
        if let Some(restore) = self.fallback.as_ref().and_then(WeakFocusHandle::upgrade) {
            window.focus(&restore, cx);
        }
    }

    /// Ends the focus session for a press on the toast's action: focus
    /// goes back where it was before `toast.focus` only if it's still in
    /// the toast, as when the press is from the keyboard. Focus that has
    /// moved on, as before a click, stays, for the action to run there.
    fn end_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.holds_focus(window, cx) {
            self.give_back_focus(window, cx);
        } else if self.focused.take().is_some() {
            self.queue.set_held(None, cx.background_executor().now());
            self.changed(cx);
        }
    }

    /// Whether focus is on the focused toast's card or one of its buttons:
    /// also when the toast has just gone and they're no longer drawn.
    fn holds_focus(&self, window: &Window, cx: &App) -> bool {
        window
            .focused(cx)
            .is_some_and(|focus| focus == self.card || self.buttons.contains(&focus))
    }

    /// Focus left the focused toast: moved elsewhere, a picker say, which
    /// keeps the toast's session for focus to come back to, its timer
    /// running meanwhile; or lost as the toast went.
    fn focus_left(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(focused) = &self.focused else {
            return;
        };
        if self.queue.shown_entry(focused.id).is_none() {
            self.toast_gone(window, cx);
        } else {
            self.queue.set_held(None, cx.background_executor().now());
            self.changed(cx);
        }
    }

    /// The focused toast went: its session ends, and focus, if it was left
    /// on the toast, goes back where it was before `toast.focus`. Focus
    /// that has moved on stays where it is.
    fn toast_gone(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.focused.is_none() {
            return;
        }
        let stranded = window.focused(cx).is_none() || self.holds_focus(window, cx);
        if stranded {
            self.give_back_focus(window, cx);
        } else {
            self.focused = None;
            self.queue.set_held(None, cx.background_executor().now());
            self.changed(cx);
        }
    }

    /// The command palette's and the menu bar's actions, pressed in a
    /// focused toast: the shell beside the toasts handles them, out of the
    /// toast's reach, so focus goes back where it was first, and they run
    /// from there.
    fn forward_to_shell(
        &mut self,
        action: &dyn gpui::Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.give_back_focus(window, cx);
        window.dispatch_action(action.boxed_clone(), cx);
    }

    #[cfg(test)]
    pub(crate) fn queue(&self) -> &ToastQueue {
        &self.queue
    }

    /// Shows `toast` in this layer's window, whichever is active: for
    /// [`ToastLayer::dismiss`] once it's out of date.
    pub(crate) fn push(&mut self, toast: Toast, cx: &mut Context<Self>) -> ToastId {
        let now = cx.background_executor().now();
        let id = self.queue.push(toast, now);
        // The newest, at the bottom, shows even when they don't all fit.
        self.scroll.scroll_to_bottom();
        self.changed(cx);
        id
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
    /// If the focused toast went, focus goes back where it was.
    fn changed(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        self.queue.expire(now);
        if let Some(focused) = &self.focused
            && self.queue.shown_entry(focused.id).is_none()
        {
            // Not from inside a window update: once this one's over.
            let (window, layer) = (self.window, cx.weak_entity());
            cx.defer(move |cx| {
                window
                    .update(cx, |_, window, cx| {
                        layer.update(cx, |layer, cx| layer.toast_gone(window, cx))
                    })
                    .ok();
            });
        }
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
        // The focused toast's buttons take the layer's focus handles, in
        // its tab order.
        let focused = self
            .focused
            .as_ref()
            .is_some_and(|focused| focused.id == id);
        let order = entry.buttons();
        let focus_of = |button: ToastButton| {
            let at = order.iter().position(|b| *b == button)?;
            focused.then(|| self.buttons.get(at).cloned()).flatten()
        };
        let close = IconButton::new(
            SharedString::from(format!("toast-close-{ix}")),
            Icon::X,
            "Dismiss",
            &fe_ui::ui_theme(cx),
        )
        .when_some(focus_of(ToastButton::Close), |close, focus| {
            close.focus_handle(&focus)
        })
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
                    let focus = focus_of(ToastButton::Choice(i));
                    div()
                        .debug_selector(move || format!("toast-choice-{ix}-{i}"))
                        .child(
                            Button::new(
                                SharedString::from(format!("toast-choice-{ix}-{i}")),
                                title,
                            )
                            .small()
                            .when_some(focus, |button, focus| button.focus_handle(&focus))
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
            let focus = focus_of(ToastButton::Action);
            div()
                .debug_selector(move || format!("toast-action-{ix}"))
                .self_end()
                .flex()
                .child(
                    Button::new(("toast-action", ix), label)
                        .small()
                        .when_some(focus, |button, focus| button.focus_handle(&focus))
                        .on_click(move |event, window, cx| {
                            // Pressed from the keyboard, the command runs
                            // where focus was before the toast had it;
                            // clicked, where focus is.
                            layer
                                .update(cx, |layer, cx| layer.end_session(window, cx))
                                .ok();
                            let source = match event.is_keyboard() {
                                true => Source::Keyboard,
                                false => Source::Mouse,
                            };
                            run_action(command.clone(), source, window, cx);
                            layer.update(cx, |layer, cx| layer.dismiss(id, cx)).ok();
                        }),
                )
        });
        let hovered =
            cx.listener(move |layer, hovered: &bool, _, cx| layer.set_hovered(id, *hovered, cx));
        div()
            .id(("toast", ix))
            .debug_selector(move || format!("toast-{ix}"))
            .when(focused, |card| {
                card.key_context("toast")
                    .track_focus(&self.card)
                    // A click doesn't focus the card, as one on its
                    // buttons doesn't focus them.
                    .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                        window.prevent_default()
                    })
                    .on_action(cx.listener(
                        |layer, action: &kiln_shell::command_palette::Toggle, window, cx| {
                            layer.forward_to_shell(action, window, cx)
                        },
                    ))
                    .on_action(cx.listener(
                        |layer, action: &kiln_shell::menu_bar::Open, window, cx| {
                            layer.forward_to_shell(action, window, cx)
                        },
                    ))
            })
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
fn run_action(command: CommandId, source: Source, window: &mut Window, cx: &mut App) {
    let stack = kiln_platform::keys::context_stack(&window.context_stack(), cx);
    let invocation = Invocation::new(command).source(source).context(stack);
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

/// A toast from the keyboard, beyond what `tests/a11y_reach_chrome.rs`
/// reaches through the crate's API: an action toast's buttons, and the
/// timer held by keyboard focus and by the pointer apart.
#[cfg(test)]
mod keyboard_tests {
    use std::time::{Duration, Instant};

    use fe_buffer::Buffer;
    use fe_command::{CommandId, ContextFrame, ContextStack, Invocation};
    use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
    use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

    use super::{LIFETIME, Toast, ToastQueue, show};
    use crate::{Launch, Workbench};

    mod timer {
        use super::*;

        const SECOND: Duration = Duration::from_secs(1);

        /// Shown toast at `t0`, its id, and whether it's gone at `at`.
        fn shown_at(queue: &mut ToastQueue, at: Instant) -> bool {
            queue.expire(at);
            !queue.shown().is_empty()
        }

        #[test]
        fn focus_let_go_under_the_pointer_keeps_it() {
            let (mut queue, t0) = (ToastQueue::default(), Instant::now());
            let id = queue.push(Toast::info("saved"), t0);
            queue.set_held(Some(id), t0 + SECOND);
            queue.set_hovered(id, true, t0 + 2 * SECOND);
            // Focus goes; the pointer is still over it.
            queue.set_held(None, t0 + 3 * SECOND);
            assert!(
                shown_at(&mut queue, t0 + LIFETIME * 10),
                "stays under the pointer"
            );
            // The pointer leaves: the time it had left runs.
            let left = t0 + LIFETIME * 10;
            queue.set_hovered(id, false, left);
            assert!(shown_at(&mut queue, left + LIFETIME - 2 * SECOND));
            assert!(!shown_at(&mut queue, left + LIFETIME));
        }

        #[test]
        fn the_pointer_leaving_a_focused_toast_keeps_it() {
            let (mut queue, t0) = (ToastQueue::default(), Instant::now());
            let id = queue.push(Toast::info("saved"), t0);
            queue.set_hovered(id, true, t0 + SECOND);
            queue.set_held(Some(id), t0 + 2 * SECOND);
            // The pointer goes; focus is still in it.
            queue.set_hovered(id, false, t0 + 3 * SECOND);
            assert!(shown_at(&mut queue, t0 + LIFETIME * 10), "stays with focus");
            let left = t0 + LIFETIME * 10;
            queue.set_held(None, left);
            assert!(shown_at(&mut queue, left + LIFETIME - 2 * SECOND));
            assert!(!shown_at(&mut queue, left + LIFETIME));
        }
    }

    /// A workbench showing `one\ntwo\n` in an editor that has focus, in an
    /// active window, with an action toast whose action is Select All.
    fn opened(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
        cx.update(|cx| {
            fe_gpui::init(cx);
            crate::init(cx);
        });
        let dir = tempfile::tempdir().unwrap();
        let launch = Launch {
            root: Some(dir.path().to_owned()),
            file: Some((
                dir.path().join("a.txt"),
                Some(Buffer::new("one\ntwo\n").into()),
            )),
        };
        let (workbench, cx) = cx.add_window_view(move |window, cx| {
            let theme = EditorTheme::default_dark(cx);
            Workbench::new(launch, theme, EditorSettings::default(), window, cx)
        });
        cx.update(|window, cx| {
            window.activate_window();
            window.focus(&workbench.focus_handle(cx), cx);
        });
        draw(cx);
        let select_all = CommandId::parse("editor.select_all").unwrap();
        cx.update(|_, cx| show(cx, Toast::info("Copied").action("Select All", select_all)));
        draw(cx);
        assert!(editor_focused(&workbench, cx));
        (dir, workbench, cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }

    fn run(command: &str, cx: &mut VisualTestContext) {
        let stack = ContextStack::new().with(ContextFrame::new("workbench"));
        let invocation = Invocation::of(command).unwrap().context(stack);
        cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
        draw(cx);
    }

    /// Presses and releases `keys`, a key at a time.
    fn press(keys: &str, cx: &mut VisualTestContext) {
        for key in keys.split_whitespace() {
            cx.simulate_keystrokes(key);
            cx.simulate_event(gpui::KeyUpEvent {
                keystroke: gpui::Keystroke::parse(key).unwrap(),
            });
            draw(cx);
        }
    }

    fn editor_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
        let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
        let editor = editor.expect("an editor");
        cx.update(|window, cx| editor.focus_handle(cx).contains_focused(window, cx))
    }

    fn selection(
        workbench: &Entity<Workbench>,
        cx: &mut VisualTestContext,
    ) -> std::ops::Range<usize> {
        let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
        editor
            .unwrap()
            .read_with(cx, |editor, _| editor.model().primary_selection().range)
    }

    const ACTION: &str = "toast-action-0";
    const CLOSE: &str = "icon-button:toast-close-0";

    /// The button holding the focus ring.
    fn ringed(cx: &mut VisualTestContext) -> Option<&'static str> {
        let ring = cx.debug_bounds("focus-ring")?;
        [ACTION, CLOSE].into_iter().find(|button| {
            cx.debug_bounds(button)
                .is_some_and(|bounds| ring.contains(&bounds.center()))
        })
    }

    #[gpui::test]
    fn its_action_and_close_buttons_are_in_its_tab_order(cx: &mut TestAppContext) {
        let (_dir, _workbench, cx) = opened(cx);
        run("toast.focus", cx);
        let mut seen = Vec::new();
        for key in ["tab", "tab", "shift-tab", "shift-tab"] {
            press(key, cx);
            seen.push(ringed(cx));
        }
        assert_eq!(seen, [Some(CLOSE), Some(ACTION), Some(CLOSE), Some(ACTION)]);
        // Both are labelled buttons to a screen reader.
        let nodes = cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            fe_ui::nodes(window, cx)
        });
        let labels: Vec<String> = (nodes.iter())
            .filter(|node| node.role == fe_ui::Role::Button)
            .filter(|node| node.id.to_string().contains("toast"))
            .map(|node| node.label.to_string())
            .collect();
        assert_eq!(labels.len(), 2, "{labels:?}");
        assert!(labels.contains(&"Select All".to_owned()), "{labels:?}");
        assert!(labels.contains(&"Dismiss".to_owned()), "{labels:?}");
    }

    /// Enter on the action runs its command where focus was before the
    /// toast had it, the editor, which selects all, and dismisses the
    /// toast.
    #[gpui::test]
    fn enter_on_the_action_runs_it_in_the_editor(cx: &mut TestAppContext) {
        let (_dir, workbench, cx) = opened(cx);
        assert_eq!(selection(&workbench, cx), 0..0);
        run("toast.focus", cx);
        press("enter", cx);
        assert_eq!(selection(&workbench, cx), 0.."one\ntwo\n".len());
        assert!(editor_focused(&workbench, cx));
        let layer = workbench.read_with(cx, |workbench, _| workbench.toasts.clone());
        assert!(layer.read_with(cx, |layer, _| layer.queue().shown().is_empty()));
    }

    /// Focus moved on from a focused toast, to the project panel, stays
    /// where it went when the toast's action is clicked: the action runs
    /// there, not in the editor focus was in before `toast.focus`.
    #[gpui::test]
    fn a_click_on_the_action_after_focus_moved_on_runs_it_there(cx: &mut TestAppContext) {
        let (_dir, workbench, cx) = opened(cx);
        run("toast.focus", cx);
        assert!(!editor_focused(&workbench, cx));
        let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
        cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
        draw(cx);
        let action = cx.debug_bounds(ACTION).expect("the action shows");
        cx.simulate_click(action.center(), gpui::Modifiers::none());
        draw(cx);
        let panel_focused =
            cx.update(|window, cx| panel.focus_handle(cx).contains_focused(window, cx));
        assert!(panel_focused, "focus stays in the panel");
        assert!(!editor_focused(&workbench, cx));
        assert_eq!(
            selection(&workbench, cx),
            0..0,
            "the editor selected nothing"
        );
        let layer = workbench.read_with(cx, |workbench, _| workbench.toasts.clone());
        assert!(layer.read_with(cx, |layer, _| layer.queue().shown().is_empty()));
    }

    #[gpui::test]
    fn space_on_the_close_button_runs_nothing(cx: &mut TestAppContext) {
        let (_dir, workbench, cx) = opened(cx);
        run("toast.focus", cx);
        press("tab space", cx);
        assert_eq!(selection(&workbench, cx), 0..0);
        assert!(editor_focused(&workbench, cx));
        let layer = workbench.read_with(cx, |workbench, _| workbench.toasts.clone());
        assert!(layer.read_with(cx, |layer, _| layer.queue().shown().is_empty()));
    }
}
