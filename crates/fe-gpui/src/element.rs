use std::{
    collections::HashMap,
    ops::Range,
    sync::Arc,
    time::{Duration, Instant},
};

use fe_display::{
    CharWidth,
    layout::{
        self, Frame, FrameInput, FrameStyles, MeasuredLine, Rect, StyleRun, StyledSpan,
        TextMeasure, Vec2,
    },
};
use fe_syntax::Syntax;
use gpui::{
    App, Bounds, ContentMask, Element, ElementId, ElementInputHandler, Entity, Font, FontId,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, Pixels, Point, ShapedLine,
    Style, TextAlign, TextRun, TextSystem, Window, WindowTextSystem, fill, point, px, relative,
    size,
};

use crate::{EditorTheme, EditorView, FrameTiming};

/// What one frame laid out: kept by the view for hit testing and IME.
pub(crate) struct FrameLayout {
    /// Window position of the editor's top-left corner, which the frame's
    /// coordinates are relative to.
    pub(crate) origin: Point<Pixels>,
    pub(crate) frame: Frame<Line>,
    /// How long prepaint took, for [`FrameTiming`].
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

/// A row or line number shaped by GPUI.
#[derive(Clone)]
pub(crate) struct Line(ShapedLine);

impl MeasuredLine for Line {
    fn width(&self) -> f32 {
        self.0.width.into()
    }

    fn x_for_index(&self, index: usize) -> f32 {
        self.0.x_for_index(index).into()
    }

    fn index_for_x(&self, x: f32) -> usize {
        self.0.closest_index_for_x(px(x))
    }
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
        let runs: Vec<TextRun> = runs
            .iter()
            .map(|run| TextRun {
                len: run.len,
                font: self.font.clone(),
                color: run.style,
                background_color: None,
                underline: None,
                strikethrough: None,
            })
            .collect();
        Line(
            self.text_system
                .shape_line(text.to_string().into(), self.font_size, &runs, None),
        )
    }
}

/// Char widths in the editor font, in pixels, for soft wrap. Shapes each
/// char once, so fallback fonts are measured too.
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
                .set_wrap_width(layout::wrap_width(width, rows, em));
            view.model.sync_display();
        });

        let view = self.view.read(cx);
        let theme = &view.theme;
        let model = &view.model;
        let selections = model.selections();
        let input = FrameInput {
            display: model.display(),
            tab_size: model.tab_size(),
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
            .filter(|syntax| syntax.version() == model.snapshot().version());
        let mut shaper = Shaper {
            text_system,
            font,
            font_size,
        };
        let frame = layout::layout_frame(&input, &mut shaper, |range| {
            styled_highlights(syntax, theme, range)
        });

        let scroll = point(px(frame.scroll.x), px(frame.scroll.y));
        self.view.update(cx, |view, _| {
            view.scroll = scroll;
            view.autoscroll = false;
        });
        Some(FrameLayout {
            origin: bounds.origin,
            frame,
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
                        let origin = layout.to_window(frame.row_origin(row.row));
                        row.line
                            .0
                            .paint(origin, line_height, TextAlign::Left, None, window, cx)
                            .ok();
                    }
                    for (origin, marker) in &frame.fold_markers {
                        let origin = layout.to_window(*origin);
                        marker
                            .0
                            .paint(origin, line_height, TextAlign::Left, None, window, cx)
                            .ok();
                    }
                    if focus.is_focused(window) {
                        for &cursor in &frame.cursors {
                            window.paint_quad(fill(layout.bounds(cursor), theme.cursor));
                        }
                    }
                },
            );
            for (origin, number) in &frame.line_numbers {
                number
                    .0
                    .paint(
                        layout.to_window(*origin),
                        line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    )
                    .ok();
            }
        });

        let painted_at = Instant::now();
        let timing = FrameTiming {
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
