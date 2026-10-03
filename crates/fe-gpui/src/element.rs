use fe_buffer::Bias;
use fe_display::ExpandedRow;
use gpui::{
    App, Bounds, ContentMask, Element, ElementId, ElementInputHandler, Entity, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Point, ShapedLine, SharedString, Style,
    TextAlign, TextRun, Window, fill, point, px, relative, size,
};

use crate::EditorView;

/// Longest prefix of a row that is shaped and painted. Rows beyond this
/// (minified files, data dumps) are cut off rather than stalling a frame.
const MAX_SHAPED_ROW_BYTES: usize = 8 * 1024;

/// What one frame laid out: kept by the view for hit testing and IME.
pub(crate) struct FrameLayout {
    pub(crate) bounds: Bounds<Pixels>,
    /// Window x of column 0 of the text, before horizontal scrolling.
    pub(crate) text_left: Pixels,
    pub(crate) line_height: Pixels,
    pub(crate) first_row: u32,
    pub(crate) rows: Vec<RowLayout>,
}

impl FrameLayout {
    pub(crate) fn row(&self, row: u32) -> Option<&RowLayout> {
        self.rows.get(row.checked_sub(self.first_row)? as usize)
    }

    pub(crate) fn visible_rows(&self) -> u32 {
        (self.bounds.size.height / self.line_height).floor() as u32
    }
}

#[derive(Clone)]
pub(crate) struct RowLayout {
    pub(crate) row: u32,
    pub(crate) expanded: ExpandedRow,
    pub(crate) line: ShapedLine,
}

impl RowLayout {
    /// X offset (from column 0) of a buffer column in this row.
    pub(crate) fn x_for_column(&self, column: usize) -> Pixels {
        let display = self.expanded.to_display(column);
        self.line.x_for_index(display.min(self.line.len()))
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

pub(crate) struct Prepaint {
    layout: Option<FrameLayout>,
    text_bounds: Bounds<Pixels>,
    scroll: Point<Pixels>,
    active_lines: Vec<Bounds<Pixels>>,
    selections: Vec<Bounds<Pixels>>,
    cursors: Vec<Bounds<Pixels>>,
    gutter: Vec<(Point<Pixels>, ShapedLine)>,
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

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
    ) -> Prepaint {
        let view = self.view.read(cx);
        let theme = view.theme.clone();
        let model = &view.model;
        let text = model.text().clone();
        let tab_size = model.tab_size();
        let selections = model.selections();
        let primary_head = model.primary_selection().head();
        let mut scroll = view.scroll;
        let autoscroll = view.autoscroll;

        let font = gpui::font(theme.font_family.clone());
        let font_size = theme.font_size;
        let line_height = (font_size * theme.line_height).round();
        let text_system = window.text_system().clone();
        let font_id = text_system.resolve_font(&font);
        let em = text_system
            .em_advance(font_id, font_size)
            .unwrap_or(font_size * 0.6);

        let row_count = text.row_count();
        let digits = row_count.to_string().len().max(3) as f32;
        let gutter_width = em * (digits + 4.);
        let text_left = bounds.left() + gutter_width;
        let text_bounds = Bounds::new(
            point(text_left, bounds.top()),
            size(bounds.size.width - gutter_width, bounds.size.height),
        );
        let height = bounds.size.height;

        // Keep the primary cursor a few rows from the top and bottom edges.
        let head = text.offset_to_point(primary_head);
        if autoscroll {
            let margin = (line_height * 3.).min(height / 3.);
            let head_top = line_height * head.row as f32;
            if head_top < scroll.y + margin {
                scroll.y = head_top - margin;
            }
            if head_top + line_height > scroll.y + height - margin {
                scroll.y = head_top + line_height - height + margin;
            }
        }
        // Allow scrolling until the last row reaches the top.
        let max_scroll_y = line_height * row_count.saturating_sub(1) as f32;
        scroll.y = scroll.y.clamp(px(0.), max_scroll_y);

        let first_row = (scroll.y / line_height).floor() as u32;
        let end_row = (((scroll.y + height) / line_height).ceil() as u32).min(row_count);
        let row_top = |row: u32| bounds.top() + line_height * row as f32 - scroll.y;

        let mut rows = Vec::with_capacity((end_row - first_row) as usize);
        for row in first_row..end_row {
            let start = text.row_start(row);
            let len = (text.row_len(row) as usize).min(MAX_SHAPED_ROW_BYTES);
            let end = text.clip_offset(start + len, Bias::Left);
            let expanded = ExpandedRow::new(&text.slice_to_string(start..end), tab_size);
            let shown: SharedString = expanded.text().to_string().into();
            let runs = if shown.is_empty() {
                Vec::new()
            } else {
                vec![TextRun {
                    len: shown.len(),
                    font: font.clone(),
                    color: theme.foreground,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }]
            };
            let line = text_system.shape_line(shown, font_size, &runs, None);
            rows.push(RowLayout {
                row,
                expanded,
                line,
            });
        }

        // Horizontally, keep the cursor inside the text area too.
        let visible = |row: u32| rows.get(row.checked_sub(first_row)? as usize);
        if autoscroll && let Some(row) = visible(head.row) {
            let x = row.x_for_column(head.column as usize);
            let margin = em * 4.;
            if x < scroll.x + margin {
                scroll.x = x - margin;
            }
            if x > scroll.x + text_bounds.size.width - margin {
                scroll.x = x - text_bounds.size.width + margin;
            }
        }
        let widest = rows
            .iter()
            .map(|row| row.line.width)
            .fold(px(0.), |a, b| a.max(b));
        let max_scroll_x = (widest - text_bounds.size.width + em * 4.).max(px(0.));
        scroll.x = scroll.x.clamp(px(0.), max_scroll_x);
        let x_at = |row: &RowLayout, column: usize| text_left + row.x_for_column(column) - scroll.x;

        let mut active_lines = Vec::new();
        let mut selection_quads = Vec::new();
        let mut cursors = Vec::new();
        let mut active_rows = Vec::new();
        for selection in &selections {
            let head = text.offset_to_point(selection.head());
            if let Some(row) = visible(head.row) {
                cursors.push(Bounds::new(
                    point(x_at(row, head.column as usize), row_top(head.row)),
                    size(px(2.), line_height),
                ));
            }
            if selection.range.is_empty() {
                active_rows.push(head.row);
                continue;
            }
            let start = text.offset_to_point(selection.range.start);
            let end = text.offset_to_point(selection.range.end);
            for row_ix in start.row.max(first_row)..=end.row.min(end_row.saturating_sub(1)) {
                let Some(row) = visible(row_ix) else { continue };
                let from = if row_ix == start.row {
                    start.column as usize
                } else {
                    0
                };
                let to = if row_ix == end.row {
                    end.column as usize
                } else {
                    text.row_len(row_ix) as usize
                };
                let mut right = x_at(row, to);
                if row_ix < end.row {
                    // Show that the line break is selected too.
                    right += em * 0.6;
                }
                selection_quads.push(Bounds::from_corners(
                    point(x_at(row, from), row_top(row_ix)),
                    point(right, row_top(row_ix) + line_height),
                ));
            }
        }
        active_rows.sort_unstable();
        active_rows.dedup();
        for &row in &active_rows {
            if visible(row).is_some() {
                active_lines.push(Bounds::new(
                    point(bounds.left(), row_top(row)),
                    size(bounds.size.width, line_height),
                ));
            }
        }

        let gutter = rows
            .iter()
            .map(|row| {
                let number: SharedString = (row.row + 1).to_string().into();
                let color = if active_rows.binary_search(&row.row).is_ok() {
                    theme.gutter_active_foreground
                } else {
                    theme.gutter_foreground
                };
                let run = TextRun {
                    len: number.len(),
                    font: font.clone(),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let line = text_system.shape_line(number, font_size, &[run], None);
                let origin = point(text_left - em * 2. - line.width, row_top(row.row));
                (origin, line)
            })
            .collect();

        self.view.update(cx, |view, _| {
            view.scroll = scroll;
            view.autoscroll = false;
        });

        Prepaint {
            layout: Some(FrameLayout {
                bounds,
                text_left,
                line_height,
                first_row,
                rows,
            }),
            text_bounds,
            scroll,
            active_lines,
            selections: selection_quads,
            cursors,
            gutter,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let view = self.view.read(cx);
        let theme = view.theme.clone();
        let focus = view.focus.clone();
        let layout = prepaint.layout.take().expect("prepaint ran");
        let line_height = layout.line_height;

        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, theme.background));
            for line in &prepaint.active_lines {
                window.paint_quad(fill(*line, theme.active_line));
            }
            window.with_content_mask(
                Some(ContentMask {
                    bounds: prepaint.text_bounds,
                }),
                |window| {
                    for selection in &prepaint.selections {
                        window.paint_quad(fill(*selection, theme.selection));
                    }
                    for row in &layout.rows {
                        let origin = point(
                            layout.text_left - prepaint.scroll.x,
                            bounds.top() + line_height * row.row as f32 - prepaint.scroll.y,
                        );
                        row.line
                            .paint(origin, line_height, TextAlign::Left, None, window, cx)
                            .ok();
                    }
                    if focus.is_focused(window) {
                        for cursor in &prepaint.cursors {
                            window.paint_quad(fill(*cursor, theme.cursor));
                        }
                    }
                },
            );
            for (origin, number) in &prepaint.gutter {
                number
                    .paint(*origin, line_height, TextAlign::Left, None, window, cx)
                    .ok();
            }
        });

        self.view.update(cx, |view, _| view.layout = Some(layout));
    }
}
