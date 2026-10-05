use std::{
    collections::HashMap,
    mem,
    ops::Range,
    sync::Arc,
    time::{Duration, Instant},
};

use fe_buffer::Text;
use fe_display::{
    CharWidth, LongRows,
    layout::{
        self, Frame, FrameInput, FrameStyles, MeasuredLine, Rect, StyleRun, StyledSpan,
        TextMeasure, Vec2,
    },
};
use fe_syntax::{Syntax, SyntaxDriver};
use gpui::{
    App, Bounds, ContentMask, CursorStyle, DecorationRun, Element, ElementId, ElementInputHandler,
    Entity, Font, FontId, GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId,
    IntoElement, LayoutId, LineLayout, Pixels, Point, Style, TextAlign, TextRun, TextSystem,
    Window, WindowTextSystem, fill, point, px, relative, size,
};

use crate::{EditorTheme, EditorView, FrameTiming};

/// The gutter's chevron on a folded row, which unfolds it.
const FOLDED_CHEVRON: &str = "›";
/// The gutter's chevron on a row that can fold.
const UNFOLDED_CHEVRON: &str = "⌄";

/// What one frame laid out: kept by the view for hit testing and IME.
pub(crate) struct FrameLayout {
    /// Window position of the editor's top-left corner, which the frame's
    /// coordinates are relative to.
    pub(crate) origin: Point<Pixels>,
    pub(crate) frame: Frame<Line>,
    /// The indexes of long rows, kept from frame to frame, and the char
    /// widths they're measured with, for the tab size they're laid out
    /// with.
    long_rows: LongRows,
    glyph_widths: GlyphWidths,
    tab_size: usize,
    /// The fold toggles' chevrons: folded, then unfolded.
    chevrons: [Line; 2],
    /// Over the fold toggles and markers, which show a pointing hand.
    clickable: Vec<Hitbox>,
    /// When prepaint started and how long it took, for [`FrameTiming`].
    prepaint_started: Instant,
    prepaint_took: Duration,
}

impl FrameLayout {
    pub(crate) fn to_frame(&self, position: Point<Pixels>) -> Vec2 {
        Vec2 {
            x: (position.x - self.origin.x).into(),
            y: (position.y - self.origin.y).into(),
        }
    }

    pub(crate) fn to_window(&self, point: Vec2) -> Point<Pixels> {
        self.origin + gpui::point(px(point.x), px(point.y))
    }

    /// The buffer offset under a window position.
    pub(crate) fn offset_at(&mut self, text: &Text, position: Point<Pixels>) -> usize {
        let position = self.to_frame(position);
        self.frame
            .offset_at(text, position, &mut self.long_rows, &mut self.glyph_widths)
    }

    /// The window bounds of a caret at `offset`, if its row is on screen.
    pub(crate) fn caret_bounds(&mut self, offset: usize) -> Option<Bounds<Pixels>> {
        let caret = self
            .frame
            .caret_bounds(offset, &mut self.long_rows, &mut self.glyph_widths)?;
        Some(self.bounds(caret))
    }

    pub(crate) fn bounds(&self, rect: Rect) -> Bounds<Pixels> {
        Bounds::new(
            self.to_window(Vec2 {
                x: rect.x,
                y: rect.y,
            }),
            size(px(rect.width), px(rect.height)),
        )
    }
}

/// A row or line number shaped by GPUI in one font run, with its colours
/// applied at paint time.
#[derive(Clone)]
pub(crate) struct Line {
    layout: Arc<LineLayout>,
    decorations: Vec<DecorationRun>,
}

impl Line {
    /// The colour of the byte at `index` of the shaped text.
    #[cfg(test)]
    pub(crate) fn color_at(&self, index: usize) -> Option<Hsla> {
        let mut end = 0;
        self.decorations.iter().find_map(|run| {
            end += run.len as usize;
            (index < end).then_some(run.color)
        })
    }

    fn paint(&self, origin: Point<Pixels>, line_height: Pixels, window: &mut Window, cx: &mut App) {
        self.layout
            .paint(
                origin,
                line_height,
                TextAlign::Left,
                None,
                &self.decorations,
                window,
                cx,
            )
            .ok();
    }
}

impl MeasuredLine for Line {
    fn width(&self) -> f32 {
        self.layout.width.into()
    }

    fn x_for_index(&self, index: usize) -> f32 {
        self.layout.x_for_index(index).into()
    }

    fn index_for_x(&self, x: f32) -> usize {
        self.layout.closest_index_for_x(px(x))
    }
}

/// The decoration runs that colour a row: the style runs with adjacent
/// same-colour ones merged.
fn decoration_runs(runs: &[StyleRun<Hsla>]) -> Vec<DecorationRun> {
    let mut out: Vec<DecorationRun> = Vec::with_capacity(runs.len());
    for run in runs {
        if run.len == 0 {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.color == run.style => last.len += run.len as u32,
            _ => out.push(DecorationRun {
                len: run.len as u32,
                color: run.style,
                background_color: None,
                underline: None,
                strikethrough: None,
            }),
        }
    }
    out
}

/// Measures text with the window's shaper, in the editor font.
struct Shaper {
    text_system: Arc<WindowTextSystem>,
    font: Font,
    font_size: Pixels,
}

impl TextMeasure for Shaper {
    type Style = Hsla;
    type Line = Line;

    fn shape(&mut self, text: &str, runs: &[StyleRun<Hsla>]) -> Line {
        let whole = [TextRun {
            len: text.len(),
            font: self.font.clone(),
            color: Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
        Line {
            layout: self
                .text_system
                .layout_line(text, self.font_size, &whole, None),
            decorations: decoration_runs(runs),
        }
    }
}

/// Char widths in the editor font, in pixels, for soft wrap and long rows.
/// Shapes each char once, so fallback fonts are measured too.
#[derive(Clone)]
pub(crate) struct GlyphWidths {
    text_system: Arc<TextSystem>,
    font_id: FontId,
    font_size: Pixels,
    cache: HashMap<char, f32>,
}

impl GlyphWidths {
    pub(crate) fn new(text_system: Arc<TextSystem>, font: &Font, font_size: Pixels) -> Self {
        Self {
            font_id: text_system.resolve_font(font),
            text_system,
            font_size,
            cache: HashMap::new(),
        }
    }
}

impl CharWidth for GlyphWidths {
    fn char_width(&mut self, ch: char) -> f32 {
        let Self {
            text_system,
            font_id,
            font_size,
            cache,
        } = self;
        *cache
            .entry(ch)
            .or_insert_with(|| text_system.layout_width(*font_id, *font_size, ch).into())
    }
}

pub(crate) struct EditorElement {
    view: Entity<EditorView>,
}

impl EditorElement {
    pub(crate) fn new(view: Entity<EditorView>) -> Self {
        Self { view }
    }
}

impl IntoElement for EditorElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<FrameLayout>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<FrameLayout> {
        let started = Instant::now();
        let theme = &self.view.read(cx).theme;
        let font = gpui::font(theme.font_family.clone());
        let font_size = theme.font_size;
        let line_height = (font_size * theme.line_height).round();
        let line_height_f32: f32 = line_height.into();
        let text_system = window.text_system().clone();
        let font_id = text_system.resolve_font(&font);
        let em: f32 = text_system
            .em_advance(font_id, font_size)
            .unwrap_or(font_size * 0.6)
            .into();

        // Soft wrap fills the text area; catch the rows up with any edits.
        let width: f32 = bounds.size.width.into();
        self.view.update(cx, |view, _| {
            let rows = view.model.text().row_count();
            view.model
                .fit_wrap_width(layout::wrap_width(width, rows, em));
            view.model.sync_display();
        });
        // Wrap a slice of what's pending around the top of the viewport,
        // keeping the text at the top where it is unless the caret is about
        // to be scrolled into view, and ask for another frame to continue.
        // Long rows' indexes stay valid while the font and tab size do.
        let tab_size = self.view.read(cx).model.tab_size();
        let carried = self.view.update(cx, |view, _| {
            let layout = view.layout.as_mut()?;
            let widths = &layout.glyph_widths;
            (widths.font_id == font_id
                && widths.font_size == font_size
                && layout.tab_size == tab_size)
                .then(|| (mem::take(&mut layout.long_rows), widths.clone()))
        });
        let (mut long_rows, mut glyph_widths) = carried.unwrap_or_else(|| {
            let widths = GlyphWidths::new(cx.text_system().clone(), &font, font_size);
            (LongRows::default(), widths)
        });
        let pending = self.view.update(cx, |view, _| {
            if !view.model.display().wrap_is_pending() {
                return false;
            }
            let (pending, top) = view
                .model
                .wrap_pending_at(f32::from(view.scroll.y) / line_height_f32);
            if !view.autoscroll {
                view.scroll.y = px(top * line_height_f32);
            }
            pending
        });
        if pending {
            window.request_animation_frame();
        }

        let view = self.view.read(cx);
        let theme = &view.theme;
        let model = &view.model;
        let selections = model.selections();
        let input = FrameInput {
            display: model.display(),
            tab_size,
            selections: &selections,
            primary_head: model.primary_selection().head(),
            scroll: Vec2 {
                x: view.scroll.x.into(),
                y: view.scroll.y.into(),
            },
            autoscroll: view.autoscroll,
            width,
            height: bounds.size.height.into(),
            line_height: line_height.into(),
            em,
            styles: FrameStyles {
                text: theme.foreground,
                line_number: theme.gutter_foreground,
                active_line_number: theme.gutter_active_foreground,
            },
        };
        // A tree for another version (never expected) would misplace colours.
        let syntax = view
            .syntax
            .as_ref()
            .map(SyntaxDriver::syntax)
            .filter(|syntax| syntax.version() == model.snapshot().version());
        let mut shaper = Shaper {
            text_system,
            font,
            font_size,
        };
        let frame = layout::layout_frame(
            &input,
            &mut shaper,
            &mut long_rows,
            &mut glyph_widths,
            |range| styled_highlights(syntax, theme, range),
            |rows| fold_headers(syntax, rows),
        );
        let chevrons = [FOLDED_CHEVRON, UNFOLDED_CHEVRON].map(|chevron| {
            let run = StyleRun {
                len: chevron.len(),
                style: theme.gutter_foreground,
            };
            shaper.shape(chevron, &[run])
        });
        let to_window = |rect: Rect| {
            Bounds::new(
                bounds.origin + point(px(rect.x), px(rect.y)),
                size(px(rect.width), px(rect.height)),
            )
        };
        let clickable = frame
            .fold_click_areas()
            .map(|area| window.insert_hitbox(to_window(area.rect), HitboxBehavior::Normal))
            .collect();

        let scroll = point(px(frame.scroll.x), px(frame.scroll.y));
        self.view.update(cx, |view, cx| {
            view.scroll = scroll;
            view.autoscroll = false;
            view.load_injected_languages(cx);
        });
        Some(FrameLayout {
            origin: bounds.origin,
            frame,
            long_rows,
            glyph_widths,
            tab_size,
            chevrons,
            clickable,
            prepaint_started: started,
            prepaint_took: started.elapsed(),
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        layout: &mut Option<FrameLayout>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let started = Instant::now();
        let view = self.view.read(cx);
        let theme = view.theme.clone();
        let focus = view.focus.clone();
        let layout = layout.take().expect("prepaint ran");
        let frame = &layout.frame;
        let line_height = px(frame.line_height);

        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, theme.background));
            for &line in &frame.active_lines {
                window.paint_quad(fill(layout.bounds(line), theme.active_line));
            }
            let text_bounds = layout.bounds(frame.text_bounds());
            window.with_content_mask(
                Some(ContentMask {
                    bounds: text_bounds,
                }),
                |window| {
                    for &selection in &frame.selections {
                        window.paint_quad(fill(layout.bounds(selection), theme.selection));
                    }
                    for row in &frame.rows {
                        let origin = layout.to_window(frame.line_origin(row));
                        row.line.paint(origin, line_height, window, cx);
                    }
                    for marker in &frame.fold_markers {
                        let rect = marker.rect;
                        let origin = layout.to_window(Vec2 {
                            x: rect.x,
                            y: rect.y,
                        });
                        marker.line.paint(origin, line_height, window, cx);
                    }
                    if focus.is_focused(window) {
                        for &cursor in &frame.cursors {
                            window.paint_quad(fill(layout.bounds(cursor), theme.cursor));
                        }
                    }
                },
            );
            for (origin, number) in &frame.line_numbers {
                number.paint(layout.to_window(*origin), line_height, window, cx);
            }
            for toggle in &frame.fold_toggles {
                let chevron = &layout.chevrons[usize::from(!toggle.folded)];
                let rect = toggle.rect;
                let origin = layout.to_window(Vec2 {
                    x: rect.x + (rect.width - chevron.width()) / 2.,
                    y: rect.y,
                });
                chevron.paint(origin, line_height, window, cx);
            }
        });
        for hitbox in &layout.clickable {
            window.set_cursor_style(CursorStyle::PointingHand, hitbox);
        }

        let painted_at = Instant::now();
        let timing = FrameTiming {
            prepaint_started: layout.prepaint_started,
            prepaint: layout.prepaint_took,
            paint: painted_at - started,
            painted_at,
        };
        self.view.update(cx, |view, _| {
            view.layout = Some(layout);
            view.frame_timings.record(timing);
        });
    }
}

/// The buffer rows in `rows` that head a fold candidate of the syntax tree.
fn fold_headers(syntax: Option<&Syntax>, rows: Range<u32>) -> Vec<u32> {
    let Some(syntax) = syntax else {
        return Vec::new();
    };
    let mut headers: Vec<u32> = fe_syntax::fold_ranges(syntax, rows)
        .into_iter()
        .map(|range| *range.start())
        .collect();
    headers.dedup();
    headers
}

/// The highlights in `range` that the theme colours.
fn styled_highlights(
    syntax: Option<&Syntax>,
    theme: &EditorTheme,
    range: Range<usize>,
) -> Vec<StyledSpan<Hsla>> {
    let Some(syntax) = syntax else {
        return Vec::new();
    };
    syntax
        .highlights(range)
        .into_iter()
        .filter_map(|span| {
            Some(StyledSpan {
                style: theme.syntax_color(span.highlight)?,
                range: span.range,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
