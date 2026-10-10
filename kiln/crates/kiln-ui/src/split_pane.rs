// SPDX-License-Identifier: LicenseRef-Proprietary

//! [`SplitPane`]: two views side by side or one above the other, sharing
//! the space by a ratio that dragging the border between them changes.

use gpui::{
    AnyView, App, AppContext as _, Axis, Bounds, Context, CursorStyle, DragMoveEvent, EntityId,
    EventEmitter, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point, Render,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::ui_theme;

/// The border's grab area, centred on it: wider than the 1 px it draws.
const HANDLE: Pixels = px(6.);

/// Two views, `first` left of (or above) `second`, with a 1 px border
/// between them. `first` takes [`SplitPane::ratio`] of the space; dragging
/// the border changes it, each side kept at least its minimum size, and
/// emits [`SplitPaneEvent::Resized`]. The ratio, not either side's size,
/// is what's kept, so both grow with the pane.
pub struct SplitPane {
    axis: Axis,
    first: AnyView,
    second: AnyView,
    ratio: f32,
    min_first: Pixels,
    min_second: Pixels,
}

/// What a [`SplitPane`] tells its subscribers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SplitPaneEvent {
    /// Dragging the border set the ratio to this.
    Resized(f32),
}

impl EventEmitter<SplitPaneEvent> for SplitPane {}

/// A split pane's border, as dragged: the pane's entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DividerDrag(EntityId);

/// A border's drag draws nothing at the pointer.
struct NoGhost;

impl Render for NoGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

impl SplitPane {
    /// `first` and `second` split along `axis` (`Horizontal` puts them side
    /// by side), half each, with no minimum sizes.
    pub fn new(axis: Axis, first: impl Into<AnyView>, second: impl Into<AnyView>) -> Self {
        Self {
            axis,
            first: first.into(),
            second: second.into(),
            ratio: 0.5,
            min_first: px(0.),
            min_second: px(0.),
        }
    }

    /// Starts with `first` taking `ratio` (0 to 1) of the space.
    pub fn with_ratio(mut self, ratio: f32) -> Self {
        self.ratio = ratio.clamp(0., 1.);
        self
    }

    /// The least each side gets by dragging the border.
    pub fn with_min_sizes(mut self, first: Pixels, second: Pixels) -> Self {
        self.min_first = first;
        self.min_second = second;
        self
    }

    pub fn axis(&self) -> Axis {
        self.axis
    }

    /// The share of the space `first` takes, from 0 to 1.
    pub fn ratio(&self) -> f32 {
        self.ratio
    }

    /// Sets the ratio without emitting an event: for the pane's owner.
    pub fn set_ratio(&mut self, ratio: f32, cx: &mut Context<Self>) {
        self.ratio = ratio.clamp(0., 1.);
        cx.notify();
    }

    /// The ratio that puts the border at `position` in a pane laid out at
    /// `bounds`, each side kept to its minimum where there's room for both.
    pub(crate) fn ratio_at(&self, position: Point<Pixels>, bounds: Bounds<Pixels>) -> f32 {
        let (at, total) = match self.axis {
            Axis::Horizontal => (position.x - bounds.left(), bounds.size.width),
            Axis::Vertical => (position.y - bounds.top(), bounds.size.height),
        };
        if total <= px(0.) {
            return self.ratio;
        }
        let low = self.min_first.min(total);
        let high = (total - self.min_second).max(low);
        at.clamp(low, high) / total
    }

    fn on_drag_move(
        &mut self,
        event: &DragMoveEvent<DividerDrag>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.drag(cx).0 != cx.entity_id() {
            return;
        }
        let ratio = self.ratio_at(event.event.position, event.bounds);
        if ratio != self.ratio {
            self.ratio = ratio;
            cx.emit(SplitPaneEvent::Resized(ratio));
            cx.notify();
        }
    }
}

impl Render for SplitPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ui_theme(cx);
        let horizontal = self.axis == Axis::Horizontal;
        let cell = |grow: f32, view: AnyView| {
            div()
                .flex()
                .flex_basis(px(0.))
                .flex_grow(grow)
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .child(view)
        };
        let drag = DividerDrag(cx.entity_id());
        let handle = div()
            .id("split-divider")
            .absolute()
            // Over the views either side, for presses, but not during a
            // drag, so drops still reach them.
            .when(!cx.has_active_drag(), |handle| handle.occlude())
            .map(|handle| {
                let offset = -(HANDLE - px(1.)) / 2.;
                if horizontal {
                    handle
                        .top_0()
                        .bottom_0()
                        .left(offset)
                        .w(HANDLE)
                        .cursor(CursorStyle::ResizeLeftRight)
                } else {
                    handle
                        .left_0()
                        .right_0()
                        .top(offset)
                        .h(HANDLE)
                        .cursor(CursorStyle::ResizeUpDown)
                }
            })
            .on_drag(drag, |_, _, _, cx: &mut App| cx.new(|_| NoGhost));
        let divider = div()
            .relative()
            .flex_none()
            .bg(theme.border)
            .map(|divider| {
                if horizontal {
                    divider.w(px(1.)).h_full()
                } else {
                    divider.h(px(1.)).w_full()
                }
            })
            .child(handle);
        div()
            .id("split-pane")
            .size_full()
            .flex()
            .when(!horizontal, |pane| pane.flex_col())
            .on_drag_move(cx.listener(Self::on_drag_move))
            .child(cell(self.ratio, self.first.clone()))
            .child(divider)
            .child(cell(1. - self.ratio, self.second.clone()))
    }
}
