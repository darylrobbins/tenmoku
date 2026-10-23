//! The mouse over the minimap ([`crate::element::minimap`]): a press on
//! the slider drags it, a press elsewhere centres the row there and drags
//! from that, and the wheel scrolls as over the text. None of it moves the
//! carets or the selection, starts a text selection or changes vim's mode.

use fe_config::settings;
use fe_display::minimap::{
    MinimapLayout, MinimapOptions, MinimapShowSlider, MinimapSide, MinimapSize,
};
use fe_display::{BlockMap, ScreenRow};
use gpui::{Bounds, Context, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Window, px};

use super::{Drag, EditorView};
use crate::EditorSettings;

/// The minimap's options from `settings`: `None` with `editor.minimap.enabled`
/// off.
pub fn minimap_options(settings: &EditorSettings) -> Option<MinimapOptions> {
    let minimap = &settings.minimap;
    minimap.enabled.then_some(MinimapOptions {
        side: match minimap.side {
            settings::MinimapSide::Right => MinimapSide::Right,
            settings::MinimapSide::Left => MinimapSide::Left,
        },
        scale: minimap.scale,
        max_column: minimap.max_column,
        size: match minimap.size {
            settings::MinimapSize::Proportional => MinimapSize::Proportional,
            settings::MinimapSize::Fill => MinimapSize::Fill,
            settings::MinimapSize::Fit => MinimapSize::Fit,
        },
        show_slider: match minimap.show_slider {
            settings::MinimapShowSlider::Mouseover => MinimapShowSlider::Mouseover,
            settings::MinimapShowSlider::Always => MinimapShowSlider::Always,
        },
        render_characters: minimap.render_characters,
    })
}

/// A drag of the minimap's slider, fixed as it starts, so a pointer height
/// always scrolls to the same place however blocks come into view.
#[derive(Clone)]
pub(super) struct MinimapDrag {
    /// The minimap's geometry then, whose rate takes the pointer's travel to
    /// display rows.
    geometry: MinimapLayout,
    /// The display row and the screen row at the top then: the screen row
    /// may be partway down a block above the display row.
    start_display: f32,
    start_screen: f32,
    /// The block's screen rows above and below the start, if it was inside
    /// a block; no rows if not.
    block_above: f32,
    block_below: f32,
    /// The last display row then.
    last_row: f32,
    /// The pointer's y in the frame then.
    start_y: f32,
}

impl MinimapDrag {
    /// The screen row at the top with the pointer `dy` from where it
    /// started, the travel taken to display rows by the geometry's rate: the
    /// start's block, if it started inside one, scrolls a screen row a row
    /// of travel to its edge, so a small move is a small scroll; past it,
    /// the screen top of the display row the rest reaches, so the slider
    /// stays with the pointer. Kept between the top and the last row's
    /// screen row, or the start if that was further down, inside a block
    /// after the last row, so dragging down never scrolls up. The start,
    /// partway into a block or not, with no travel.
    fn screen_top(&self, dy: f32, blocks: &BlockMap) -> f32 {
        if dy == 0. {
            return self.start_screen;
        }
        let last = blocks.to_screen_top(self.last_row).max(self.start_screen);
        // The geometry's rate, which stops at the rows' ends: travel that
        // reaches them is past either end from anywhere.
        let rows = self.geometry.scroll_for_drag(0., dy.abs());
        if rows >= self.last_row {
            return if dy > 0. { last } else { 0. };
        }
        let in_block = if dy > 0. {
            self.block_below
        } else {
            self.block_above
        };
        let top = if rows <= in_block {
            self.start_screen + rows.copysign(dy)
        } else {
            // Out of the block, at its edge: below it, the start display
            // row's screen row; above it, the row above's bottom, or above
            // row 0 the top, where `to_screen_top` would count the blocks
            // above row 0 again.
            let display = self.start_display + (rows - in_block).copysign(dy);
            if display < 0. {
                0.
            } else {
                blocks.to_screen_top(display)
            }
        };
        top.clamp(0., last)
    }
}

/// The screen rows above and below `screen_top` of the block it's inside,
/// with all the blocks above that display row; no rows outside a block.
fn block_around(screen_top: f32, blocks: &BlockMap) -> (f32, f32) {
    let ScreenRow::Block { .. } = blocks.at_screen_row(screen_top.max(0.) as u32) else {
        return (0., 0.);
    };
    let below = blocks.to_display_top(screen_top) as u32;
    let start = match below.checked_sub(1) {
        Some(above) => blocks.screen_row(above) + 1,
        None => 0,
    };
    let end = blocks.screen_row(below);
    (screen_top - start as f32, end as f32 - screen_top)
}

impl EditorView {
    /// Shows the minimap beside the text with `options`, or hides it with
    /// `None`; the text area narrows by its width. Off by default.
    pub fn set_minimap(&mut self, options: Option<MinimapOptions>, cx: &mut Context<Self>) {
        if self.minimap != options {
            self.minimap = options;
            if options.is_none() && self.minimap_dragging() {
                self.drag = None;
            }
            cx.notify();
        }
    }

    /// The minimap's options, if it's on: see [`EditorView::set_minimap`].
    pub fn minimap(&self) -> Option<MinimapOptions> {
        self.minimap
    }

    /// Where the last frame drew the minimap, in window coordinates: `None`
    /// with it off or too narrow to show.
    pub fn minimap_bounds(&self) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        Some(layout.bounds(layout.minimap.as_ref()?.rect()))
    }

    /// Whether the minimap's slider is being dragged.
    pub(crate) fn minimap_dragging(&self) -> bool {
        matches!(self.drag, Some(Drag::Minimap(_)))
    }

    /// A left press on the last frame's minimap: on the slider, starts
    /// dragging it; elsewhere, scrolls the row there to the editor's centre,
    /// as VS Code does, and drags from there. Returns whether it was on the
    /// minimap.
    pub(super) fn press_minimap(
        &mut self,
        event: &MouseDownEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        let Some(minimap) = layout.minimap.as_ref() else {
            return false;
        };
        let at = layout.to_frame(event.position);
        // Not where a popover or menu covers it.
        let over = minimap.hitbox.is_hovered_at(event.position, window);
        if event.button != MouseButton::Left || !over {
            return false;
        }
        let frame = &layout.frame;
        let blocks = &frame.block_map;
        let start_screen = if minimap.slider().contains(at) {
            frame.scroll.y / frame.line_height
        } else {
            // Centred on the screen, blocks and all.
            let row = minimap.geometry.scroll_for_click(at.y) as f32;
            let last = blocks.to_screen_top(minimap.rows.saturating_sub(1) as f32);
            let centre = blocks.to_screen_top(row + 0.5);
            (centre - minimap.screen_rows / 2.).clamp(0., last)
        };
        let (block_above, block_below) = block_around(start_screen, blocks);
        let drag = MinimapDrag {
            geometry: minimap.geometry.clone(),
            start_display: blocks.to_display_top(start_screen),
            start_screen,
            block_above,
            block_below,
            last_row: minimap.rows.saturating_sub(1) as f32,
            start_y: at.y,
        };
        self.scroll_to_screen_top(start_screen, cx);
        self.drag = Some(Drag::Minimap(Box::new(drag)));
        cx.notify();
        true
    }

    /// A move while the slider is dragged, wherever the pointer is: the
    /// slider follows it. A move with the button up ends the drag.
    pub(crate) fn drag_minimap(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(Drag::Minimap(drag)) = &self.drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            cx.notify();
            return;
        }
        let Some(layout) = &self.layout else {
            return;
        };
        let dy = layout.to_frame(event.position).y - drag.start_y;
        let top = drag.screen_top(dy, &layout.frame.block_map);
        self.scroll_to_screen_top(top, cx);
    }

    /// Scrolls so the screen row `top`, fractional, is at the top from the
    /// next frame; the carets stay.
    fn scroll_to_screen_top(&mut self, top: f32, cx: &mut Context<Self>) {
        let Some(layout) = &self.layout else {
            return;
        };
        let y = top * layout.frame.line_height;
        self.set_hover(None, cx);
        self.hide_popover(cx);
        self.scroll.y = px(y.max(0.));
        self.autoscroll = false;
        cx.notify();
    }
}

#[cfg(test)]
#[path = "minimap_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "minimap_left_tests.rs"]
mod left_tests;
