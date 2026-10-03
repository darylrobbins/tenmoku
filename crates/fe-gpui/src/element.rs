use std::{ops::Range, sync::Arc};

use fe_display::layout::{
    self, Frame, FrameInput, FrameStyles, MeasuredLine, Rect, StyleRun, StyledSpan, TextMeasure,
    Vec2,
};
use fe_syntax::Syntax;
use gpui::{
    App, Bounds, ContentMask, Element, ElementId, ElementInputHandler, Entity, Font,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, Pixels, Point, ShapedLine,
    Style, TextAlign, TextRun, Window, WindowTextSystem, fill, point, px, relative, size,
};

use crate::{EditorTheme, EditorView};

/// What one frame laid out: kept by the view for hit testing and IME.
pub(crate) struct FrameLayout {
    /// Window position of the editor's top-left corner, which the frame's
    /// coordinates are relative to.
    pub(crate) origin: Point<Pixels>,
    pub(crate) frame: Frame<Line>,
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
        let view = self.view.read(cx);
        let theme = &view.theme;
        let model = &view.model;

        let font = gpui::font(theme.font_family.clone());
        let font_size = theme.font_size;
        let line_height = (font_size * theme.line_height).round();
        let text_system = window.text_system().clone();
        let font_id = text_system.resolve_font(&font);
        let em = text_system
            .em_advance(font_id, font_size)
            .unwrap_or(font_size * 0.6);

        let selections = model.selections();
        let input = FrameInput {
            text: model.text(),
            tab_size: model.tab_size(),
            selections: &selections,
            primary_head: model.primary_selection().head(),
            scroll: Vec2 {
                x: view.scroll.x.into(),
                y: view.scroll.y.into(),
            },
            autoscroll: view.autoscroll,
            width: bounds.size.width.into(),
            height: bounds.size.height.into(),
            line_height: line_height.into(),
            em: em.into(),
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

        self.view.update(cx, |view, _| view.layout = Some(layout));
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
