//! Dragging a tab: what's dragged, the ghost following the pointer, and
//! where in a pane a drop lands, which a pane shows while hovered.

use fe_ui::{TAB_HEIGHT, UiTheme};
use gpui::{
    Bounds, Context, IntoElement, ParentElement as _, Pixels, Point, Render, SharedString,
    Styled as _, WeakEntity, Window, div, point, px, size,
};

use crate::{Direction, Pane, pane::TabId, pane_group::Destination};

/// A tab being dragged: its pane, and the tab, by ID, as indices can shift
/// while the drag goes on (a file landing, say).
#[derive(Clone, Debug)]
pub(crate) struct DraggedTab {
    pub pane: WeakEntity<Pane>,
    pub tab: TabId,
}

/// A tab dropped on a pane, for the workbench to move.
pub(crate) struct TabDropped {
    pub dragged: DraggedTab,
    pub destination: Destination,
}

/// The dragged tab's file name, in tab style, at the pointer.
pub(crate) struct TabGhost(pub SharedString, pub UiTheme);

impl Render for TabGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let tab = fe_ui::Tab::new("ghost", self.0.clone(), &self.1).active(true);
        div().h(TAB_HEIGHT).text_size(px(12.)).child(tab)
    }
}

/// Where in a pane's content a drop goes: the outer quarter on each side
/// splits that way, the rest takes the tab in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DropZone {
    Left,
    Right,
    Up,
    Down,
    Centre,
}

impl DropZone {
    /// The zone of content laid out at `bounds` that `point` is in. In a
    /// corner, the nearer edge wins, by distance relative to the size.
    pub(crate) fn at(bounds: Bounds<Pixels>, point: Point<Pixels>) -> Self {
        let x = f32::from(point.x - bounds.left()) / f32::from(bounds.size.width);
        let y = f32::from(point.y - bounds.top()) / f32::from(bounds.size.height);
        let (dx, dy) = (x.min(1. - x), y.min(1. - y));
        match (dx.min(dy) < 0.25, dx <= dy) {
            (false, _) => DropZone::Centre,
            (true, true) if x < 0.5 => DropZone::Left,
            (true, true) => DropZone::Right,
            (true, false) if y < 0.5 => DropZone::Up,
            (true, false) => DropZone::Down,
        }
    }

    /// The part of a pane laid out at `pane` a drop here fills: the half on
    /// that side, or for the centre all of its content, laid out at
    /// `content`.
    pub(crate) fn overlay(self, pane: Bounds<Pixels>, content: Bounds<Pixels>) -> Bounds<Pixels> {
        let (mut overlay, half) = (pane, pane.size / 2.);
        match self {
            DropZone::Left => overlay.size.width = half.width,
            DropZone::Up => overlay.size.height = half.height,
            DropZone::Right => {
                overlay.origin.x += half.width;
                overlay.size.width -= half.width;
            }
            DropZone::Down => {
                overlay.origin.y += half.height;
                overlay.size.height -= half.height;
            }
            DropZone::Centre => overlay = content,
        }
        overlay
    }
}

/// Where in a pane a hovering tab would drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DropTarget {
    /// Into the tab bar, at this index.
    TabBar(usize),
    /// Onto the content area.
    Content(DropZone),
}

impl DropTarget {
    /// Where the tab goes, dropped here in `pane`.
    pub(crate) fn destination(self, pane: gpui::Entity<Pane>) -> Destination {
        match self {
            DropTarget::TabBar(index) => Destination::Pane {
                pane,
                index: Some(index),
            },
            DropTarget::Content(zone) => {
                let direction = match zone {
                    DropZone::Left => Direction::Left,
                    DropZone::Right => Direction::Right,
                    DropZone::Up => Direction::Up,
                    DropZone::Down => Direction::Down,
                    DropZone::Centre => return Destination::Pane { pane, index: None },
                };
                Destination::Split { pane, direction }
            }
        }
    }
}

/// The insertion index for a pointer at `x` over tabs laid out at `tabs`:
/// before the first tab whose right half or beyond it's not in, else the
/// end.
pub(crate) fn insertion(tabs: &[Bounds<Pixels>], x: Pixels) -> usize {
    tabs.iter()
        .position(|tab| x < tab.center().x)
        .unwrap_or(tabs.len())
}

/// The 2 px insertion marker for `index` among tabs laid out at `tabs`, in
/// a bar laid out at `bar`.
pub(crate) fn marker(tabs: &[Bounds<Pixels>], index: usize, bar: Bounds<Pixels>) -> Bounds<Pixels> {
    let x = match (tabs.get(index), tabs.last()) {
        (Some(tab), _) => tab.left(),
        (None, Some(last)) => last.right(),
        (None, None) => bar.left() + px(1.),
    };
    // Tabs can reach past the bar, scrolled or long.
    let x = (x - px(1.)).clamp(bar.left(), (bar.right() - px(2.)).max(bar.left()));
    Bounds {
        origin: point(x, bar.top()),
        size: size(px(2.), TAB_HEIGHT),
    }
}

#[cfg(test)]
#[path = "pane_drag_followup_tests.rs"]
mod followup_tests;
