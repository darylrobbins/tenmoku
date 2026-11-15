//! The operation in progress, on the status bar's left, right after the vim
//! mode (`docs/design/design-system.md`, "Operation in progress"): a 12 px
//! spinner, the primary operation's label, a 72 px bar when it has a
//! fraction, a count such as `3/12` when its message carries one, and `+2`
//! for the others ([`crate::operations`]). With none the item is absent
//! and the bar doesn't shift.
//!
//! A click lists every operation in a popup above the item: a spinner or a
//! bar, the label and the source, one row each. `Esc` closes it. The item's
//! accessible name is the label and "N more".
//!
//! Nothing draws while the workbench is idle. While something runs, a timer
//! repaints the item at most [`TICK`] apart, 10 times a second, turning the
//! spinner a step each time and showing what the operations said since;
//! an operation changing doesn't repaint it by itself.

use std::{cell::Cell, rc::Rc, time::Duration};

use fe_ui::{ActiveTheme as _, Icon, Role, UI_SMALL, node};
use gpui::{
    Anchor, Bounds, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseDownEvent, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Transformation,
    WeakFocusHandle, Window, anchored, canvas, deferred, div, point, prelude::FluentBuilder as _,
    px, radians, svg,
};
use kiln_l10n::{tr, verbatim};

use crate::{
    operations::{Operation, OperationSource, Operations},
    workbench::toast::card_look,
};

/// The most often the item repaints: 10 times a second.
pub(crate) const TICK: Duration = Duration::from_millis(100);

/// Ticks to a turn of the spinner: one a second.
const TURN: u32 = 10;

/// The spinner's side, in the item and the list.
const SPINNER: f32 = 12.;

/// The bar's width and height.
pub(crate) const BAR_WIDTH: f32 = 72.;
const BAR_HEIGHT: f32 = 4.;

/// The popup's width, and the tallest it gets before it scrolls.
const LIST_WIDTH: f32 = 360.;
const LIST_MAX_HEIGHT: f32 = 280.;

/// The list's key context.
const KEY_CONTEXT: &str = "operations_status";

/// The `done` and `total` of the first `done/total` in `message`, such as
/// `Indexing 3/12 crates`.
pub(crate) fn count_in(message: &str) -> Option<(u64, u64)> {
    let bytes = message.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if !bytes[at].is_ascii_digit() || (at > 0 && bytes[at - 1].is_ascii_digit()) {
            at += 1;
            continue;
        }
        let digits = |from: usize| {
            let len = bytes[from..]
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            (len > 0).then(|| (message[from..from + len].parse::<u64>().ok(), from + len))
        };
        let (done, end) = digits(at)?;
        if bytes.get(end) == Some(&b'/')
            && let Some((total, _)) = digits(end + 1)
            && let (Some(done), Some(total)) = (done, total)
        {
            return Some((done, total));
        }
        at = end;
    }
    None
}

/// `operation`'s count, `3/12`, if its message carries one.
pub(crate) fn count_text(operation: &Operation) -> Option<SharedString> {
    let (done, total) = count_in(operation.detail.as_deref()?)?;
    Some(tr!(
        "workbench-status-operation-count",
        done = done,
        total = total
    ))
}

/// The item's accessible name: the primary's label, and how many more.
pub(crate) fn item_name(label: &str, others: usize) -> SharedString {
    tr!(
        "workbench-status-operation-name",
        label = label.to_owned(),
        count = others
    )
}

/// Where a list row says `source` comes from.
fn source_text(source: &OperationSource) -> SharedString {
    match source {
        OperationSource::LanguageServer { name, .. } => name.clone().into(),
        OperationSource::Run(_) => tr!("workbench-status-operation-source-run"),
        OperationSource::Other => tr!("workbench-status-operation-source-other"),
    }
}

/// The item and its list.
pub(crate) struct OperationsStatus {
    operations: Entity<Operations>,
    open: bool,
    focus: FocusHandle,
    /// What had focus before the list opened.
    restore: Option<WeakFocusHandle>,
    /// Where the item was drawn, so a click on it isn't a click outside the
    /// list.
    item_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// How many timer ticks have passed, which turn the spinner.
    steps: u32,
    /// Whether the operations changed since the last paint.
    stale: bool,
    ticker: Option<Task<()>>,
    /// How many times the timer has repainted.
    #[cfg(test)]
    pub(crate) repaints: usize,
    /// How many times the item rendered, and the last text it drew: the
    /// label, the count and the "+N".
    #[cfg(test)]
    pub(crate) renders: usize,
    #[cfg(test)]
    pub(crate) drawn: (SharedString, Option<SharedString>, Option<SharedString>),
    _observe: Subscription,
}

impl OperationsStatus {
    pub(crate) fn new(
        operations: Entity<Operations>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let observe = cx.observe_in(&operations, window, |status, _, window, cx| {
            status.changed(window, cx)
        });
        let mut status = Self {
            operations,
            open: false,
            focus: cx.focus_handle(),
            restore: None,
            item_bounds: Rc::default(),
            steps: 0,
            stale: false,
            ticker: None,
            #[cfg(test)]
            repaints: 0,
            #[cfg(test)]
            renders: 0,
            #[cfg(test)]
            drawn: Default::default(),
            _observe: observe,
        };
        status.ensure_ticker(cx);
        status
    }

    /// Whether the list is open.
    #[cfg(test)]
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// The spinner's angle at the current step, in radians.
    pub(crate) fn angle(&self, cx: &gpui::App) -> f32 {
        if cx.reduce_motion() {
            return 0.;
        }
        (self.steps % TURN) as f32 / TURN as f32 * std::f32::consts::TAU
    }

    /// The operations changed: the next tick shows it. When none are left
    /// the list closes and the timer stops.
    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operations.read(cx).list().is_empty() {
            // Focus goes back to where it was, if the list had it.
            self.close(true, window, cx);
            self.restore = None;
            self.ticker = None;
            self.stale = false;
            return;
        }
        self.stale = true;
        self.ensure_ticker(cx);
    }

    fn ensure_ticker(&mut self, cx: &mut Context<Self>) {
        if self.ticker.is_some() || self.operations.read(cx).list().is_empty() {
            return;
        }
        self.ticker = Some(cx.spawn(async move |status, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let more = status.update(cx, |status, cx| {
                    if status.operations.read(cx).list().is_empty() {
                        status.ticker = None;
                        return false;
                    }
                    status.steps = status.steps.wrapping_add(1);
                    #[cfg(test)]
                    {
                        status.repaints += 1;
                    }
                    status.stale = false;
                    cx.notify();
                    true
                });
                if !matches!(more, Ok(true)) {
                    break;
                }
            }
        }));
    }

    fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.close(true, window, cx);
        } else {
            self.restore = window.focused(cx).map(|focus| focus.downgrade());
            self.open = true;
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn close(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        let back = self.restore.take().and_then(|focus| focus.upgrade());
        if restore
            && self.focus.contains_focused(window, cx)
            && let Some(back) = back
        {
            window.focus(&back, cx);
        }
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" && !event.keystroke.modifiers.modified() {
            self.close(true, window, cx);
            cx.stop_propagation();
        }
    }

    fn on_mouse_down_out(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let on_item =
            (self.item_bounds.get()).is_some_and(|bounds| bounds.contains(&event.position));
        if !on_item {
            self.close(false, window, cx);
        }
    }

    /// A spinner turned to `angle`, or still when motion is reduced.
    fn spinner(&self, angle: f32, colour: gpui::Hsla) -> gpui::Svg {
        svg()
            .path(Icon::LoaderCircle.path())
            .size(px(SPINNER))
            .flex_none()
            .text_color(colour)
            .with_transformation(Transformation::rotate(radians(angle)))
    }

    /// The bar, `fraction` filled.
    fn bar(&self, fraction: f32, cx: &gpui::App) -> impl IntoElement {
        let colors = cx.theme().colors;
        div()
            .debug_selector(|| verbatim!("status-operation-bar").to_owned())
            .flex_none()
            .w(px(BAR_WIDTH))
            .h(px(BAR_HEIGHT))
            .rounded_full()
            .overflow_hidden()
            .bg(colors.border.default)
            .child(
                div()
                    .debug_selector(|| verbatim!("status-operation-fill").to_owned())
                    .h_full()
                    .w(gpui::relative(fraction.clamp(0., 1.)))
                    .bg(colors.accent.base),
            )
    }

    fn list(&self, angle: f32, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let look = card_look(&colors);
        let rows = self
            .operations
            .read(cx)
            .list()
            .iter()
            .enumerate()
            .map(|(at, operation)| {
                let lead = match operation.fraction {
                    Some(fraction) => self.bar(fraction, cx).into_any_element(),
                    None => self.spinner(angle, colors.text.muted).into_any_element(),
                };
                div()
                    .debug_selector(move || format!("{}{at}", verbatim!("status-operations-row:")))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .h(px(28.))
                    .text_size(UI_SMALL.size)
                    .line_height(UI_SMALL.line_height)
                    .child(div().flex().flex_none().w(px(BAR_WIDTH)).child(lead))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text.primary)
                            .child(SharedString::from(operation.label.clone())),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(colors.text.muted)
                            .child(source_text(&operation.source)),
                    )
            })
            .collect::<Vec<_>>();
        let list = div().id(verbatim!("status-operations"));
        node(
            list,
            window,
            cx,
            Role::Dialog,
            tr!("workbench-status-operations-title"),
        )
        .debug_selector(|| verbatim!("status-operations-list").to_owned())
        .key_context(KEY_CONTEXT)
        .track_focus(&self.focus)
        .on_key_down(cx.listener(Self::on_key_down))
        .on_mouse_down_out(cx.listener(Self::on_mouse_down_out))
        .occlude()
        .w(px(LIST_WIDTH))
        .max_h(px(LIST_MAX_HEIGHT))
        .overflow_y_scroll()
        .py_1()
        .rounded(look.radius)
        .border_1()
        .border_color(colors.border.default)
        .bg(look.fill)
        .shadow(vec![look.shadow])
        .children(rows)
    }
}

impl Render for OperationsStatus {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(primary) = self.operations.read(cx).primary().cloned() else {
            return div().into_any_element();
        };
        let others = self.operations.read(cx).others();
        let colors = cx.theme().colors;
        let angle = self.angle(cx);
        let name = item_name(&primary.label, others);
        let count = count_text(&primary);
        #[cfg(test)]
        {
            self.renders += 1;
            self.drawn = (
                primary.label.clone().into(),
                count.clone(),
                (others > 0).then(|| tr!("workbench-status-operation-more", count = others)),
            );
        }
        let bounds = self.item_bounds.clone();
        let item = div()
            .id(verbatim!("status-operation"))
            .debug_selector(|| verbatim!("status-operation").to_owned())
            .flex()
            .flex_none()
            .items_center()
            .gap_1p5()
            .h_full()
            .px_2()
            .whitespace_nowrap()
            .cursor_pointer()
            .hover(|style| style.bg(colors.list.row_hover))
            .on_click(cx.listener(|status, _, window, cx| status.toggle(window, cx)))
            .child(
                self.spinner(angle, colors.text.muted)
                    .debug_selector(|| verbatim!("status-operation-spinner").to_owned()),
            )
            .child(
                div()
                    .debug_selector(|| verbatim!("status-operation-label").to_owned())
                    .child(SharedString::from(primary.label.clone())),
            )
            .when_some(primary.fraction, |item, fraction| {
                item.child(self.bar(fraction, cx))
            })
            .when_some(count, |item, count| {
                item.child(
                    div()
                        .debug_selector(|| verbatim!("status-operation-count").to_owned())
                        .text_color(colors.text.muted)
                        .child(count),
                )
            })
            .when(others > 0, |item| {
                item.child(
                    div()
                        .debug_selector(|| verbatim!("status-operation-more").to_owned())
                        .text_color(colors.text.muted)
                        .child(tr!("workbench-status-operation-more", count = others)),
                )
            })
            .child(
                canvas(move |drawn, _, _| bounds.set(Some(drawn)), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            );
        let item = node(item, window, cx, Role::Button, name);
        let list = self.open.then(|| {
            // Above the item's top left, over the editor.
            div().absolute().top_0().left_0().child(
                deferred(
                    anchored()
                        .anchor(Anchor::BottomLeft)
                        .offset(point(px(0.), px(-4.)))
                        .snap_to_window()
                        .child(self.list(angle, window, cx)),
                )
                .with_priority(1),
            )
        });
        div()
            .relative()
            .flex_none()
            .h_full()
            .child(item)
            .children(list)
            .into_any_element()
    }
}

impl crate::Workbench {
    /// Opens the list of operations above the status bar's item, if any run.
    pub fn show_operations(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let item = self.status_bar().read(cx).operations_item();
        item.update(cx, |item, cx| {
            if !item.open && !item.operations.read(cx).list().is_empty() {
                item.toggle(window, cx);
            }
        });
    }
}
