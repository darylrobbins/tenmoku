// SPDX-License-Identifier: Apache-2.0

//! [`SplitPane`]: two views side by side or one above the other, sharing
//! the space by a ratio, or one side a fixed size and the other the rest,
//! which dragging the border between them changes.

use gpui::{
    AnyView, App, AppContext as _, Axis, Bounds, Context, CursorStyle, DragMoveEvent, EntityId,
    EventEmitter, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point, Render,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::ActiveTheme as _;

/// The border's grab area, centred on it: wider than the 1 px it draws.
const HANDLE: Pixels = px(6.);

/// Two views, `first` left of (or above) `second`, with a 1 px border
/// between them. `first` takes [`SplitPane::ratio`] of the space, so both
/// grow with the pane; or, given [`SplitPane::first_size`] or
/// [`SplitPane::second_size`], that side keeps its size and the other
/// takes the rest. Dragging the border changes the ratio or the size, each
/// side kept at least its minimum, and emits [`SplitPaneEvent::Resized`].
/// Either view can be swapped ([`SplitPane::set_first`]).
///
/// It holds views, not elements: each side keeps its own state and
/// redraws alone, and an element would have to be rebuilt by the pane's
/// owner every frame, which a view already is.
pub struct SplitPane {
    axis: Axis,
    first: AnyView,
    second: AnyView,
    ratio: f32,
    /// A side that keeps its size, if one does.
    fixed: Option<Fixed>,
    min_first: Pixels,
    min_second: Pixels,
}

/// The side of a [`SplitPane`] that keeps its size.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Fixed {
    First(Pixels),
    Second(Pixels),
}

/// What a [`SplitPane`] tells its subscribers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SplitPaneEvent {
    /// Dragging the border put it at this share of the space: the new
    /// ratio, or the fixed side's new size over the pane's.
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
            fixed: None,
            min_first: px(0.),
            min_second: px(0.),
        }
    }

    /// Starts with `first` taking `ratio` (0 to 1) of the space.
    pub fn with_ratio(mut self, ratio: f32) -> Self {
        self.ratio = ratio.clamp(0., 1.);
        self
    }

    /// `first` keeps `size` (its width, or its height in a vertical
    /// split), and `second` takes the rest.
    pub fn first_size(mut self, size: Pixels) -> Self {
        self.fixed = Some(Fixed::First(size));
        self
    }

    /// `second` keeps `size`, and `first` takes the rest.
    pub fn second_size(mut self, size: Pixels) -> Self {
        self.fixed = Some(Fixed::Second(size));
        self
    }

    /// The size of the side that keeps one, if one does.
    pub fn fixed_size(&self) -> Option<Pixels> {
        self.fixed
            .map(|(Fixed::First(size) | Fixed::Second(size))| size)
    }

    /// Shows `view` as the first side, in place of the one before.
    pub fn set_first(&mut self, view: impl Into<AnyView>, cx: &mut Context<Self>) {
        self.first = view.into();
        cx.notify();
    }

    /// Shows `view` as the second side, in place of the one before.
    pub fn set_second(&mut self, view: impl Into<AnyView>, cx: &mut Context<Self>) {
        self.second = view.into();
        cx.notify();
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

    /// Where the border goes for a drag at `position` in a pane laid out
    /// at `bounds`, each side kept to its minimum where there's room for
    /// both, and the pane's length; `None` if it has none.
    fn border_at(
        &self,
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
    ) -> Option<(Pixels, Pixels)> {
        let (at, total) = match self.axis {
            Axis::Horizontal => (position.x - bounds.left(), bounds.size.width),
            Axis::Vertical => (position.y - bounds.top(), bounds.size.height),
        };
        if total <= px(0.) {
            return None;
        }
        let low = self.min_first.min(total);
        let high = (total - self.min_second).max(low);
        Some((at.clamp(low, high), total))
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
        let Some((at, total)) = self.border_at(event.event.position, event.bounds) else {
            return;
        };
        let changed = match &mut self.fixed {
            None => std::mem::replace(&mut self.ratio, at / total) != at / total,
            Some(Fixed::First(size)) => std::mem::replace(size, at) != at,
            // The border's pixel is the first side's.
            Some(Fixed::Second(size)) => {
                let second = (total - at - px(1.)).max(px(0.));
                std::mem::replace(size, second) != second
            }
        };
        if changed {
            cx.emit(SplitPaneEvent::Resized(at / total));
            cx.notify();
        }
    }
}

impl Render for SplitPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().colors.border.default;
        let horizontal = self.axis == Axis::Horizontal;
        // A side with a fixed size keeps it; the others share the rest by
        // their grow factors.
        let cell = |grow: f32, fixed: Option<Pixels>, view: AnyView| {
            div()
                .flex()
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .map(|cell| match fixed {
                    Some(size) if horizontal => cell.flex_none().w(size),
                    Some(size) => cell.flex_none().h(size),
                    // Opposite a fixed side, the rest: grow factors below
                    // 1 would take only their share of it.
                    None if self.fixed.is_some() => cell.flex_basis(px(0.)).flex_grow(1.),
                    None => cell.flex_basis(px(0.)).flex_grow(grow),
                })
                .child(view)
        };
        let (first_fixed, second_fixed) = match self.fixed {
            Some(Fixed::First(size)) => (Some(size), None),
            Some(Fixed::Second(size)) => (None, Some(size)),
            None => (None, None),
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
            .bg(border)
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
            .child(cell(self.ratio, first_fixed, self.first.clone()))
            .child(divider)
            .child(cell(1. - self.ratio, second_fixed, self.second.clone()))
    }
}
