//! Lays out and paints a [`FileViewer`]'s text with
//! [`fe_display::layout::layout_frame`]: the gutter with its line numbers,
//! the rows, the selection and the caret. No syntax colours, no folds and
//! no soft wrap.
//!
//! It measures with the editor's [`GlyphWidths`]. The editor element's
//! shaped line and shaper are private to it, so this file has its own: a
//! line coloured by its style runs, as there, without the merging of
//! same-coloured runs that syntax colours call for.

use std::{mem, sync::Arc};

use fe_display::{
    LongRows,
    layout::{
        self, Frame, FrameInput, FrameStyles, MeasuredLine, Rect, StyleRun, TextMeasure, Vec2,
    },
};
use fe_editor::ViewerModel;
use gpui::{
    App, Bounds, ContentMask, Context, DecorationRun, Element, ElementId, Entity, Font, FontId,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, LineLayout, Pixels, Point,
    Style, TextAlign, TextRun, Window, WindowTextSystem, fill, point, px, relative, size,
};

use super::FileViewer;
use crate::element::{
    GlyphWidths,
    a11y_text::{self, A11yText, RunKey},
};

/// The text area's accessible text, shared by [`FileViewerElement`], which
/// builds it in prepaint, and the text area's node, which pushes it.
pub(crate) type A11yTextCell = std::rc::Rc<std::cell::RefCell<A11yTextState>>;

#[derive(Default)]
pub(crate) struct A11yTextState {
    /// This frame's, while a screen reader is on.
    pub(crate) text: Option<A11yText>,
    /// The runs' node ids in the tree last sent, for a screen reader's
    /// selection.
    ids: std::collections::HashMap<gpui::accesskit::NodeId, RunKey>,
}

/// The [`A11yText`] of `layout`'s frame and the viewer's selection: the
/// editor's builder, cap and projection, always read-only. The viewer has
/// no folds or wrap, and its text is bytes of the file.
pub(crate) fn build_a11y_text(
    layout: &mut ViewerLayout,
    viewer: &FileViewer,
    scale: f32,
) -> A11yText {
    let model = &viewer.model;
    a11y_text::build(a11y_text::Input {
        display: model.display(),
        frame: &layout.frame,
        long_rows: &mut layout.long_rows,
        widths: &mut layout.glyph_widths,
        tab_size: layout.tab_size,
        anchor: viewer.anchor(),
        head: model.head(),
        origin: (layout.origin.x.into(), layout.origin.y.into()),
        scale,
        cap: a11y_text::Cap::EDITOR,
        read_only: true,
    })
}

/// `area`, the text area's node, with the text runs in `cell` as its
/// children, the selection on it and a handler for a screen reader's
/// `SetTextSelection`. It's the focused node while the viewer is focused.
pub(crate) fn attach_a11y_text<E: gpui::StatefulInteractiveElement>(
    area: E,
    cell: A11yTextCell,
    focused: bool,
    cx: &mut Context<FileViewer>,
) -> E {
    use gpui::accesskit::{Action, ActionData};
    let built = cell.clone();
    let area = area.a11y_synthetic_children(move |builder| {
        let mut state = built.borrow_mut();
        let A11yTextState { text, ids } = &mut *state;
        ids.clear();
        let Some(text) = text else {
            return;
        };
        let keys: std::collections::HashMap<_, _> = (text.runs.iter())
            .map(|run| {
                let key = run.key;
                (
                    key,
                    builder.synthetic_node_id((key.buffer_row, key.segment)),
                )
            })
            .collect();
        let nodes = text.write(builder.parent_node(), |key| keys[&key]);
        for ((id, node), run) in nodes.into_iter().zip(&text.runs) {
            if builder.push_child(id, node) {
                ids.insert(id, run.key);
            }
        }
    });
    let view = cx.entity().downgrade();
    let area = area.on_a11y_action(Action::SetTextSelection, move |data, _, cx| {
        let Some(ActionData::SetTextSelection(selection)) = data else {
            return;
        };
        let offsets = {
            let state = cell.borrow();
            let offset = |position: &gpui::accesskit::TextPosition| {
                let key = state.ids.get(&position.node)?;
                state.text.as_ref()?.offset(*key, position.character_index)
            };
            offset(&selection.anchor).zip(offset(&selection.focus))
        };
        if let Some((anchor, focus)) = offsets {
            (view.update(cx, |view, cx| view.select_from_a11y(anchor, focus, cx))).ok();
        }
    });
    match focused {
        true => area.aria_active_descendant(),
        false => area,
    }
}

/// What one frame laid out: kept by the view for hit testing.
pub(crate) struct ViewerLayout {
    /// Window position of the text area's top-left corner, which the
    /// frame's coordinates are relative to.
    pub(crate) origin: Point<Pixels>,
    pub(crate) frame: Frame<ViewerLine>,
    /// The indexes of long rows, kept from frame to frame while the font
    /// and tab size they're measured with stay the same.
    long_rows: LongRows,
    glyph_widths: GlyphWidths,
    font_id: FontId,
    font_size: Pixels,
    tab_size: usize,
}

impl ViewerLayout {
    fn to_frame(&self, position: Point<Pixels>) -> Vec2 {
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

    /// The text offset under a window position.
    pub(crate) fn offset_at(&mut self, model: &ViewerModel, position: Point<Pixels>) -> usize {
        let position = self.to_frame(position);
        self.frame.offset_at(
            model.text(),
            position,
            &mut self.long_rows,
            &mut self.glyph_widths,
        )
    }

    /// The window bounds of a caret at `offset`, if its row is on screen.
    #[cfg(test)]
    pub(crate) fn caret_bounds(&mut self, offset: usize) -> Option<Bounds<Pixels>> {
        let caret = self
            .frame
            .caret_bounds(offset, &mut self.long_rows, &mut self.glyph_widths)?;
        Some(self.bounds(caret))
    }
}

/// A row or line number shaped by GPUI in one font run, coloured at paint
/// time.
#[derive(Clone)]
pub(crate) struct ViewerLine {
    layout: Arc<LineLayout>,
    decorations: Vec<DecorationRun>,
}

impl ViewerLine {
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

impl MeasuredLine for ViewerLine {
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

/// Shapes text with the window's shaper, in the editor font.
struct Shaper {
    text_system: Arc<WindowTextSystem>,
    font: Font,
    font_size: Pixels,
}

impl TextMeasure for Shaper {
    type Style = Hsla;
    type Line = ViewerLine;

    fn shape(&mut self, text: &str, runs: &[StyleRun<Hsla>]) -> ViewerLine {
        let whole = [TextRun {
            len: text.len(),
            font: self.font.clone(),
            color: Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
        let decorations = runs
            .iter()
            .filter(|run| run.len > 0)
            .map(|run| DecorationRun {
                len: run.len as u32,
                color: run.style,
                background_color: None,
                underline: None,
                strikethrough: None,
            })
            .collect();
        ViewerLine {
            layout: self
                .text_system
                .layout_line(text, self.font_size, &whole, None),
            decorations,
        }
    }
}

pub(crate) struct FileViewerElement {
    view: Entity<FileViewer>,
}

impl FileViewerElement {
    pub(crate) fn new(view: Entity<FileViewer>) -> Self {
        Self { view }
    }
}

impl IntoElement for FileViewerElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for FileViewerElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<ViewerLayout>;

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
    ) -> Option<ViewerLayout> {
        let viewer = self.view.read(cx);
        let theme = &viewer.theme;
        let font = gpui::font(theme.font_family.clone());
        let font_size = theme.font_size;
        let line_height = (font_size * theme.line_height).round();
        let text_system = window.text_system().clone();
        let font_id = text_system.resolve_font(&font);
        let em: f32 = text_system
            .em_advance(font_id, font_size)
            .unwrap_or(font_size * 0.6)
            .into();
        let tab_size = viewer.model.tab_size();
        let carried = self.view.update(cx, |viewer, _| {
            // The last frame's layout stays for hit testing until this one
            // is painted.
            let layout = viewer.layout.as_mut()?;
            (layout.font_id == font_id
                && layout.font_size == font_size
                && layout.tab_size == tab_size)
                .then(|| {
                    (
                        mem::take(&mut layout.long_rows),
                        layout.glyph_widths.clone(),
                    )
                })
        });
        let (mut long_rows, mut glyph_widths) = carried.unwrap_or_else(|| {
            let widths = GlyphWidths::new(cx.text_system().clone(), &font, font_size);
            (LongRows::default(), widths)
        });

        let viewer = self.view.read(cx);
        let theme = &viewer.theme;
        let model = &viewer.model;
        let selections = model.resolved_selections();
        let input = FrameInput {
            display: model.display(),
            tab_size,
            selections: &selections,
            primary_head: model.head(),
            scroll: Vec2 {
                x: viewer.scroll.x.into(),
                y: viewer.scroll.y.into(),
            },
            autoscroll: viewer.autoscroll,
            width: bounds.size.width.into(),
            height: bounds.size.height.into(),
            line_height: line_height.into(),
            em,
            styles: FrameStyles {
                text: theme.foreground,
                line_number: theme.gutter_foreground,
                active_line_number: theme.gutter_active_foreground,
            },
        };
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
            |_| Vec::new(),
            |_| Vec::new(),
        );
        let scroll = point(px(frame.scroll.x), px(frame.scroll.y));
        self.view.update(cx, |viewer, _| {
            viewer.scroll = scroll;
            viewer.autoscroll = false;
        });
        let mut layout = ViewerLayout {
            origin: bounds.origin,
            frame,
            long_rows,
            glyph_widths,
            font_id,
            font_size,
            tab_size,
        };
        // The text a screen reader is given, which the text area's node
        // reads once this element has prepainted.
        let viewer = self.view.read(cx);
        let a11y = window
            .is_a11y_active()
            .then(|| build_a11y_text(&mut layout, viewer, window.scale_factor()));
        viewer.a11y_text.borrow_mut().text = a11y;
        Some(layout)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        layout: &mut Option<ViewerLayout>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let viewer = self.view.read(cx);
        let theme = viewer.theme.clone();
        let focused = viewer.focus.is_focused(window);
        let layout = layout.take().expect("prepaint ran");
        let frame = &layout.frame;
        let line_height = px(frame.line_height);
        #[cfg(test)]
        let mut painted_rows = Vec::new();

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
                        #[cfg(test)]
                        painted_rows.push((row.buffer_row, row.expanded.text().to_owned(), origin));
                    }
                    if focused {
                        for &caret in &frame.cursors {
                            window.paint_quad(fill(layout.bounds(caret), theme.cursor));
                        }
                    }
                },
            );
            for (origin, number) in &frame.line_numbers {
                number.paint(layout.to_window(*origin), line_height, window, cx);
            }
        });

        self.view.update(cx, |viewer, cx| {
            viewer.layout = Some(layout);
            // Reading the rows may have found the file changed.
            viewer.check_index(cx);
            #[cfg(test)]
            {
                viewer.painted_rows = painted_rows;
            }
        });
    }
}
