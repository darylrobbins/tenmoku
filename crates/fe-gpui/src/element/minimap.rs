//! The minimap beside the text ([`fe_display::minimap`]): its width comes
//! off the text area's right before soft wrap and the frame are laid out;
//! once the frame's scroll is known, its display rows are laid out as runs
//! in the colours the text gets, over `minimap_background`, with the slider
//! over the rows the editor shows. Painted afresh each frame.

use std::ops::Range;

use fe_buffer::Bias;
use fe_display::{
    layout::{Rect, StyledSpan},
    minimap::{MinimapLayout, MinimapOptions, MinimapShowSlider, MinimapSide, MinimapViewport},
};
use fe_syntax::Syntax;
use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, Hitbox, HitboxBehavior, Hsla, MouseDownEvent,
    MouseExitEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollWheelEvent, WeakEntity, Window,
    fill,
};

use super::{FrameLayout, PointerOutside, foreground_spans, styled_highlights};
use crate::{EditorTheme, EditorView};

/// How opaque a run's colour is over the minimap's background, as VS Code's
/// blocks are faint beside the text.
const RUN_ALPHA: f32 = 0.6;

/// The most text, in bytes, between two rows the minimap draws (hidden by a
/// fold, or the rest of a long row) that their spans are queried across.
const CHUNK_GAP: usize = 16 * 1024;

/// Whether the pointer is over the minimap, and over its slider, in the
/// rendered frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Hover {
    pub(crate) over: bool,
    pub(crate) slider: bool,
}

/// The minimap one frame laid out, in the frame's coordinates.
pub(crate) struct Placed {
    pub(crate) geometry: MinimapLayout,
    /// The minimap's height, the editor's.
    height: f32,
    /// How many display rows there are.
    pub(crate) rows: u32,
    /// How many screen rows, display rows and blocks, the editor's height
    /// holds.
    pub(crate) screen_rows: f32,
    show_slider: MinimapShowSlider,
    /// Each run's rect and colour.
    runs: Vec<(Rect, Hsla)>,
    /// Over the minimap, where the pointer is an arrow.
    pub(crate) hitbox: Hitbox,
}

impl Placed {
    /// The minimap's rect.
    pub(crate) fn rect(&self) -> Rect {
        let x = &self.geometry.x;
        Rect {
            x: x.start,
            y: 0.,
            width: x.end - x.start,
            height: self.height,
        }
    }

    /// The slider's rect, across the minimap.
    pub(crate) fn slider(&self) -> Rect {
        Rect {
            y: self.geometry.slider_top,
            height: self.geometry.slider_height,
            ..self.rect()
        }
    }
}

/// The options as this card draws them: on the right, whatever `side` says.
fn drawn(options: &MinimapOptions) -> MinimapOptions {
    MinimapOptions {
        side: MinimapSide::Right,
        ..*options
    }
}

/// The width the minimap takes off the right of `width`, which it shares
/// with the text, whose columns are `em` wide: none when it's off or the
/// editor is too narrow for it.
pub(crate) fn width(options: Option<&MinimapOptions>, width: f32, em: f32) -> f32 {
    options.map_or(0., |options| drawn(options).width(width, em))
}

/// Lays out the minimap beside `layout`'s frame, `width` the editor's whole
/// width, and its hitbox: `None` when it's hidden.
#[allow(clippy::too_many_arguments)]
pub(crate) fn layout(
    options: &MinimapOptions,
    layout: &FrameLayout,
    view: &EditorView,
    syntax: Option<&Syntax>,
    unnecessary: &[Range<usize>],
    width: f32,
    em: f32,
    window: &mut Window,
) -> Option<Placed> {
    let options = drawn(options);
    let frame = &layout.frame;
    let display = view.model.display();
    let rows = display.row_count();
    // The editor scrolls by screen rows; blocks above rows aren't the
    // minimap's, so both edges of the view are taken to display rows.
    let line_height = frame.line_height.max(1.);
    let (top, screen_rows) = (frame.scroll.y / line_height, frame.height / line_height);
    let first_visible_row = frame.block_map.to_display_top(top);
    let visible_rows = frame.block_map.to_display_top(top + screen_rows) - first_visible_row;
    let geometry = MinimapLayout::new(
        &options,
        &MinimapViewport {
            width,
            height: frame.height,
            em,
            rows,
            first_visible_row,
            visible_rows,
        },
    );
    if !geometry.visible() {
        return None;
    }
    let runs = runs(&options, &geometry, view, syntax, unnecessary);
    let rect = Rect {
        x: geometry.x.start,
        y: 0.,
        width: geometry.x.end - geometry.x.start,
        height: frame.height,
    };
    let hitbox = window.insert_hitbox(layout.bounds(rect), HitboxBehavior::Normal);
    Some(Placed {
        geometry,
        height: frame.height,
        rows,
        screen_rows,
        show_slider: options.show_slider,
        runs,
        hitbox,
    })
}

/// The runs of the display rows `geometry` draws, each the row's height,
/// in the colours the text's spans give them, or the text colour.
fn runs(
    options: &MinimapOptions,
    geometry: &MinimapLayout,
    view: &EditorView,
    syntax: Option<&Syntax>,
    unnecessary: &[Range<usize>],
) -> Vec<(Rect, Hsla)> {
    let theme = &view.theme;
    let (display, text) = (view.model.display(), view.model.text());
    let max_column = options.max_column.max(1);
    // A char is a column at least and 4 bytes at most: a long row is read
    // no further than its drawn columns can reach.
    let reach = 4 * max_column as usize;
    let rows: Vec<(u32, Range<usize>)> = (geometry.first_row..geometry.end_row)
        .map(|row| {
            let range = display.row(row).range;
            let end = text.clip_offset(range.end.min(range.start + reach), Bias::Left);
            (row, range.start..end)
        })
        .collect();
    let column = options.column_width();
    let mut out = Vec::new();
    // Rows near one another take their spans in one go, which the syntax
    // caches as one range: what a fold hides or a long row's rest is
    // queried with them, unless that's far more than a row.
    for chunk in rows.chunk_by(|(_, above), (_, below)| below.start <= above.end + CHUNK_GAP) {
        let range = chunk[0].1.start..chunk[chunk.len() - 1].1.end;
        let foreground = foreground_spans(&view.model.decorations(range.clone()), theme);
        let spans = styled_highlights(syntax, theme, unnecessary, &foreground, range);
        let mut next = 0;
        for (row, range) in chunk {
            next += spans[next..].partition_point(|span| span.range.end <= range.start);
            let row_spans = row_spans(&spans[next..], range);
            let row_text = text.slice_to_string(range.clone());
            let runs = fe_display::minimap::row_runs(
                &row_text,
                view.model.tab_size(),
                &row_spans,
                theme.foreground,
                max_column,
            );
            let y = geometry.row_y(*row);
            out.extend(runs.into_iter().map(|run| {
                let rect = Rect {
                    x: geometry.x.start + run.columns.start as f32 * column,
                    y,
                    width: run.columns.len() as f32 * column,
                    height: geometry.row_height,
                };
                (rect, run.color.opacity(RUN_ALPHA))
            }));
        }
    }
    out
}

/// The spans over `range`, which start at or after `spans`' first, as byte
/// ranges within it.
fn row_spans(spans: &[StyledSpan<Hsla>], range: &Range<usize>) -> Vec<(Range<usize>, Hsla)> {
    (spans.iter())
        .take_while(|span| span.range.start < range.end)
        .filter(|span| span.range.end > range.start)
        .map(|span| {
            let start = span.range.start.max(range.start) - range.start;
            let end = span.range.end.min(range.end) - range.start;
            (start..end, span.style)
        })
        .collect()
}

/// What [`paint`] painted, in window coordinates.
#[cfg(test)]
#[derive(Clone, Debug, Default)]
pub(crate) struct Painted {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) background: Hsla,
    pub(crate) runs: Vec<(Bounds<Pixels>, Hsla)>,
    pub(crate) slider: Option<(Bounds<Pixels>, Hsla)>,
}

/// Paints the minimap `layout` placed, the slider with the pointer `hover`
/// and the slider `dragged` or not, with an arrow over it.
pub(crate) fn paint(
    layout: &FrameLayout,
    theme: &EditorTheme,
    hover: Hover,
    dragged: bool,
    window: &mut Window,
) {
    let Some(placed) = &layout.minimap else {
        return;
    };
    let bounds = layout.bounds(placed.rect());
    window.paint_quad(fill(bounds, theme.minimap_background));
    for &(run, color) in &placed.runs {
        window.paint_quad(fill(layout.bounds(run), color));
    }
    if let Some((slider, color)) = slider(layout, theme, hover, dragged) {
        window.paint_quad(fill(slider, color));
    }
    window.set_cursor_style(CursorStyle::Arrow, &placed.hitbox);
}

/// The slider's bounds and colour, if it shows: always, or while the pointer
/// is over the minimap or drags it.
fn slider(
    layout: &FrameLayout,
    theme: &EditorTheme,
    hover: Hover,
    dragged: bool,
) -> Option<(Bounds<Pixels>, Hsla)> {
    let placed = layout.minimap.as_ref()?;
    let shown = placed.show_slider == MinimapShowSlider::Always || hover.over || dragged;
    let color = if dragged {
        theme.minimap_slider_active
    } else if hover.slider {
        theme.minimap_slider_hover
    } else {
        theme.minimap_slider
    };
    shown.then(|| (layout.bounds(placed.slider()), color))
}

/// What [`paint`] paints for `layout`, for tests.
#[cfg(test)]
pub(crate) fn painted(
    layout: &FrameLayout,
    theme: &EditorTheme,
    hover: Hover,
    dragged: bool,
) -> Option<Painted> {
    let placed = layout.minimap.as_ref()?;
    Some(Painted {
        bounds: layout.bounds(placed.rect()),
        background: theme.minimap_background,
        runs: (placed.runs.iter())
            .map(|&(run, color)| (layout.bounds(run), color))
            .collect(),
        slider: slider(layout, theme, hover, dragged),
    })
}

/// Window-wide: a drag that started on the minimap scrolls wherever the
/// pointer goes, and every event that places the pointer reconciles the
/// hover with it, as does each frame once it's the rendered one.
pub(crate) fn listen(
    view: &WeakEntity<EditorView>,
    layout: &FrameLayout,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(placed) = &layout.minimap else {
        return;
    };
    let (hitbox, slider) = (placed.hitbox.clone(), layout.bounds(placed.slider()));
    {
        let view = view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                view.update(cx, |view, cx| view.drag_minimap(event, cx))
                    .ok();
            }
        });
    }
    on_pointer_event::<MouseMoveEvent>(view, &hitbox, slider, window);
    on_pointer_event::<MouseDownEvent>(view, &hitbox, slider, window);
    on_pointer_event::<MouseUpEvent>(view, &hitbox, slider, window);
    on_pointer_event::<ScrollWheelEvent>(view, &hitbox, slider, window);
    {
        let view = view.clone();
        window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                set_hover(&view, Hover::default(), cx);
            }
        });
    }
    let view = view.clone();
    window.defer(cx, move |window, cx| {
        reconcile(&view, &hitbox, slider, window, cx)
    });
}

fn on_pointer_event<E: gpui::MouseEvent>(
    view: &WeakEntity<EditorView>,
    hitbox: &Hitbox,
    slider: Bounds<Pixels>,
    window: &mut Window,
) {
    let (view, hitbox) = (view.clone(), hitbox.clone());
    window.on_mouse_event(move |_: &E, phase, window, cx| {
        if phase == DispatchPhase::Bubble {
            // The pointer is in the window, whichever listener hears it first.
            let id = window.window_handle().window_id();
            cx.default_global::<PointerOutside>().0.remove(&id);
            reconcile(&view, &hitbox, slider, window, cx);
        }
    });
}

/// Sets the hover from the pointer's position in the rendered frame, as
/// [`super::reconcile_fold_column`] does the fold column's.
fn reconcile(
    view: &WeakEntity<EditorView>,
    hitbox: &Hitbox,
    slider: Bounds<Pixels>,
    window: &Window,
    cx: &mut App,
) {
    let id = window.window_handle().window_id();
    let outside = cx
        .try_global::<PointerOutside>()
        .is_some_and(|outside| outside.0.contains(&id));
    let position = window.mouse_position();
    let over = !outside && hitbox.is_hovered_at(position, window);
    let hover = Hover {
        over,
        slider: over && slider.contains(&position),
    };
    set_hover(view, hover, cx);
}

fn set_hover(view: &WeakEntity<EditorView>, hover: Hover, cx: &mut App) {
    view.update(cx, |view, cx| {
        if view.minimap_hover != hover {
            view.minimap_hover = hover;
            cx.notify();
        }
    })
    .ok();
}

#[cfg(test)]
#[path = "minimap_tests.rs"]
mod tests;
