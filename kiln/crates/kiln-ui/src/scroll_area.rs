// SPDX-License-Identifier: LicenseRef-Proprietary

//! [`ScrollArea`]: a vertically scrolling container with a scrollbar thumb
//! drawn in the theme's colours, which can be dragged.

use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, App, Bounds, Div, ElementId, HitboxBehavior, InteractiveElement, Interactivity,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    RenderOnce, ScrollHandle, Stateful, StatefulInteractiveElement, StyleRefinement, Styled,
    Window, canvas, div, fill, point, px,
};

use crate::{ActiveTheme as _, controls::styled};

/// The scrollbar's strip, along the area's right edge, over its content.
pub const SCROLLBAR_WIDTH: Pixels = px(8.);
/// The shortest the thumb gets, however long the content.
const MIN_THUMB: Pixels = px(20.);
/// The thumb's inset from the strip's sides.
const THUMB_INSET: Pixels = px(1.);

/// A container that scrolls its children vertically, with a thumb over its
/// right edge showing where it's scrolled to; dragging the thumb scrolls
/// it. Styled like a `div`: the caller's sizes and styles go on the area,
/// which clips its content.
///
/// ```ignore
/// ScrollArea::new("settings").h(px(300.)).children(sections)
/// ```
#[derive(IntoElement)]
pub struct ScrollArea {
    id: ElementId,
    base: Stateful<Div>,
    children: Vec<AnyElement>,
    handle: Option<ScrollHandle>,
}

impl ScrollArea {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self {
            base: div().id(id.clone()),
            id,
            children: Vec::new(),
            handle: None,
        }
    }

    /// Tracks the content's scroll offset in `handle`, to read or set it.
    pub fn track_scroll(mut self, handle: &ScrollHandle) -> Self {
        self.handle = Some(handle.clone());
        self
    }
}

impl ParentElement for ScrollArea {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for ScrollArea {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for ScrollArea {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for ScrollArea {}

/// What a scroll area keeps between frames.
struct ScrollState {
    handle: ScrollHandle,
    /// While the thumb is dragged: how far below the thumb's top it was
    /// pressed.
    grab: Rc<Cell<Option<Pixels>>>,
}

impl RenderOnce for ScrollArea {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let scrollbar = cx.theme().colors.scrollbar;
        let given = self.handle;
        let state = window.use_keyed_state(self.id, cx, |_, _| ScrollState {
            handle: given.clone().unwrap_or_default(),
            grab: Rc::default(),
        });
        let (handle, grab) = {
            let state = state.read(cx);
            (state.handle.clone(), state.grab.clone())
        };
        let (thumb, thumb_hover) = (scrollbar.thumb, scrollbar.thumb_hover);
        let (painted, dragged) = (handle.clone(), handle.clone());
        let bar = canvas(
            move |track, window, _| {
                let thumb = thumb_bounds(track, -painted.offset().y, painted.max_offset().y)?;
                Some((
                    thumb,
                    window.insert_hitbox(thumb, HitboxBehavior::BlockMouseExceptScroll),
                ))
            },
            move |track, thumb_hit, window, _| {
                let Some((bounds, hitbox)) = thumb_hit else {
                    return;
                };
                let hot = grab.get().is_some() || hitbox.is_hovered(window);
                window.paint_quad(
                    fill(bounds, if hot { thumb_hover } else { thumb }).corner_radii(px(3.)),
                );
                let hover_was = hitbox.is_hovered(window);
                let (pressed, moved, released) = (grab.clone(), grab.clone(), grab);
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if phase.bubble()
                        && event.button == MouseButton::Left
                        && hitbox.is_hovered(window)
                    {
                        pressed.set(Some(event.position.y - bounds.top()));
                        cx.stop_propagation();
                        window.refresh();
                    }
                });
                let handle = dragged;
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
                    if !phase.bubble() {
                        return;
                    }
                    match moved.get() {
                        Some(grab) if event.dragging() => {
                            let top = event.position.y - grab;
                            let max = handle.max_offset().y;
                            let scroll = scroll_for_thumb(track, bounds.size.height, top, max);
                            handle.set_offset(point(handle.offset().x, -scroll));
                            window.refresh();
                        }
                        Some(_) => moved.set(None),
                        None if bounds.contains(&event.position) != hover_was => window.refresh(),
                        None => {}
                    }
                });
                window.on_mouse_event(move |_: &MouseUpEvent, phase, window, _| {
                    if phase.bubble() && released.take().is_some() {
                        window.refresh();
                    }
                });
            },
        )
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .w(SCROLLBAR_WIDTH);
        styled(self.base, |area| area.relative().overflow_hidden())
            .child(
                div()
                    .id("scroll-area-content")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&handle)
                    .children(self.children),
            )
            .child(bar)
    }
}

/// The thumb in a scrollbar `track` the content's viewport tall, scrolled
/// `scroll` of `max`; `None` if the content fits.
pub(crate) fn thumb_bounds(
    track: Bounds<Pixels>,
    scroll: Pixels,
    max: Pixels,
) -> Option<Bounds<Pixels>> {
    if max <= px(0.) || track.size.height <= px(0.) {
        return None;
    }
    let viewport = track.size.height;
    let height = (viewport * (viewport / (viewport + max)))
        .max(MIN_THUMB)
        .min(viewport);
    let top = track.top() + (viewport - height) * (scroll / max).clamp(0., 1.);
    Some(Bounds::new(
        point(track.left() + THUMB_INSET, top),
        gpui::size(track.size.width - THUMB_INSET * 2., height),
    ))
}

/// The scroll offset that puts a thumb `height` tall at `top` in `track`,
/// clamped to `0..=max`.
pub(crate) fn scroll_for_thumb(
    track: Bounds<Pixels>,
    height: Pixels,
    top: Pixels,
    max: Pixels,
) -> Pixels {
    let room = track.size.height - height;
    if room <= px(0.) {
        return px(0.);
    }
    (max * ((top - track.top()) / room)).clamp(px(0.), max)
}
