use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    mem,
    ops::Range,
    sync::Arc,
    time::{Duration, Instant},
};

use fe_buffer::Text;
use fe_display::{
    BlockMap, CharWidth, LongRows, MAX_CLUSTER_BYTES,
    layout::{
        self, Frame, FrameInput, FrameStyles, MeasuredLine, Rect, StyleRun, StyledSpan,
        TextMeasure, Vec2,
    },
};
use fe_syntax::{Syntax, SyntaxDriver};
use fe_vim::jump::JumpLabelState;
use gpui::{
    App, BorderStyle, Bounds, ContentMask, CursorStyle, DecorationRun, DispatchPhase, Element,
    ElementId, ElementInputHandler, Entity, Font, FontId, FontWeight, Global, GlobalElementId,
    Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, LineLayout,
    ModifiersChangedEvent, MouseDownEvent, MouseEvent, MouseExitEvent, MouseMoveEvent,
    MouseUpEvent, PathBuilder, PinchEvent, Pixels, Point, ScrollWheelEvent, SharedString, Style,
    TextAlign, TextRun, TextSystem, UnderlineStyle, WeakEntity, Window, WindowId, WindowTextSystem,
    fill, outline, point, px, relative, size,
};

use crate::{
    EditorTheme, EditorView, FrameTiming,
    vim::{CaretShape, UNDERLINE_HEIGHT},
};

mod decorations;
use decorations::{Decorations, Layer};

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
    /// The buffer version laid out: positions in the frame are offsets in
    /// that text, which a later edit may have shortened.
    pub(crate) version: fe_buffer::Version,
    /// Over the fold column, whose hovering shows the chevrons of folds that
    /// are open.
    fold_column: Hitbox,
    /// Over the fold toggles and markers, which show a pointing hand.
    clickable: Vec<Hitbox>,
    /// Over the text, right of the gutter, where a hover range is found.
    text: Hitbox,
    /// The search matches to paint ([`match_rects`]).
    matches: Vec<Rect>,
    /// The bracket pair at the caret to outline ([`bracket_rects`]).
    brackets: Vec<Rect>,
    /// A pending vim jump's backdrop and labels ([`jump_layout`]).
    pub(crate) jump: Option<JumpLayout>,
    /// What an input method was composing, and the buffer offset it was
    /// drawn at, as the frame's inlay.
    composition: Option<(usize, String)>,
    /// The decorations the model's providers have for the frame's rows.
    pub(crate) decorations: Decorations,
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

    /// Whether a window position is over the text, not the gutter.
    pub(crate) fn over_text(&self, position: Point<Pixels>, window: &Window) -> bool {
        self.text.is_hovered_at(position, window)
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

    /// The buffer offset where the grapheme or glyph under a window
    /// position starts, on a row this frame shows: of `e` and a combining
    /// accent, or of a ligature, its first char. `None` past the row's end.
    pub(crate) fn char_at(&self, text: &Text, position: Point<Pixels>) -> Option<usize> {
        let at = self.to_frame(position);
        let frame = &self.frame;
        let display_row = ((at.y + frame.scroll.y) / frame.line_height).floor();
        let row = frame.row(u32::try_from(display_row as i64).ok()?)?;
        let line_x = at.x - frame.line_origin(row).x;
        if line_x < 0. {
            return None;
        }
        // The glyph with the last start at or before x: a combining mark
        // drawn over its base is such a glyph.
        let index = row.line.layout.index_for_x(px(line_x))?;
        let offset = row.shown().start + row.expanded.to_buffer(index, fe_buffer::Bias::Left);
        Some(grapheme_start(text, offset, row.start))
    }

    /// The offset a buffer row ends at, if a window position is just past
    /// that end on a row this frame shows, within the width of a space:
    /// where an empty diagnostic at the end is underlined
    /// ([`diagnostic_marks`]).
    pub(crate) fn row_end_at(&mut self, position: Point<Pixels>) -> Option<usize> {
        let at = self.to_frame(position);
        let Self {
            frame,
            long_rows,
            glyph_widths,
            ..
        } = self;
        let display_row = ((at.y + frame.scroll.y) / frame.line_height).floor();
        let row = frame.row(u32::try_from(display_row as i64).ok()?)?;
        if !row.last {
            return None;
        }
        let end = frame.view_x(row.x_for_offset(row.end, long_rows, glyph_widths));
        let space = glyph_widths.char_width(' ');
        (end <= at.x && at.x < end + space).then_some(row.end)
    }

    /// How many columns, each a space wide, a window position is past the
    /// end of a buffer row this frame shows, to the nearest; 0 short of the
    /// end or off the frame's rows. A box selection's column goes that far.
    pub(crate) fn columns_past_row_end(&mut self, position: Point<Pixels>) -> usize {
        let at = self.to_frame(position);
        let Self {
            frame,
            long_rows,
            glyph_widths,
            ..
        } = self;
        let display_row = ((at.y + frame.scroll.y) / frame.line_height).floor();
        let Some(row) = u32::try_from(display_row as i64)
            .ok()
            .and_then(|display_row| frame.row(display_row))
            .filter(|row| row.last)
        else {
            return 0;
        };
        let end = frame.view_x(row.x_for_offset(row.end, long_rows, glyph_widths));
        let space = glyph_widths.char_width(' ');
        if at.x <= end || space <= 0. {
            return 0;
        }
        ((at.x - end) / space).round() as usize
    }

    /// The window bounds of a caret at `offset`, if its row is on screen.
    pub(crate) fn caret_bounds(&mut self, offset: usize) -> Option<Bounds<Pixels>> {
        let caret = self
            .frame
            .caret_bounds(offset, &mut self.long_rows, &mut self.glyph_widths)?;
        Some(self.bounds(caret))
    }

    /// Whether this frame drew `text` composing at `offset` as its inlay,
    /// so the inlay's geometry is the composition's.
    pub(crate) fn shows_composition(&self, offset: usize, text: &str) -> bool {
        self.frame.inlay.is_some()
            && self
                .composition
                .as_ref()
                .is_some_and(|(at, shown)| *at == offset && shown == text)
    }

    /// The window bounds of `range`, bytes of the inlay's text, if this
    /// frame laid the inlay out.
    pub(crate) fn inlay_bounds(&self, range: Range<usize>) -> Option<Bounds<Pixels>> {
        let inlay = self.frame.inlay?;
        let start = inlay_x(&self.frame, range.start)?;
        let end = inlay_x(&self.frame, range.end)?;
        Some(self.bounds(Rect {
            x: start,
            width: end - start,
            ..inlay
        }))
    }

    /// The byte of the inlay's text nearest a window position on the
    /// inlay's row, from its start to its end, if this frame laid it out.
    /// `None` nearer the text before or after it.
    pub(crate) fn inlay_index_at(&self, position: Point<Pixels>) -> Option<usize> {
        let at = self.to_frame(position);
        let inlay = self.frame.inlay?;
        if !(inlay.y..inlay.y + inlay.height).contains(&at.y) {
            return None;
        }
        let (row, shown) = inlay_row(&self.frame)?;
        let line_x = at.x - self.frame.line_origin(row).x;
        let index = row.line.index_for_x(line_x);
        (line_x >= 0. && (shown.start..=shown.end).contains(&index)).then(|| index - shown.start)
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

/// The row the inlay is laid out in, and its columns there.
fn inlay_row(frame: &Frame<Line>) -> Option<(&layout::RowLayout<Line>, Range<usize>)> {
    (frame.rows.iter()).find_map(|row| Some((row, row.expanded.inlay_range()?)))
}

/// Scrolls `frame` `dx` further right than its layout did.
fn shift_frame(frame: &mut Frame<Line>, dx: f32) {
    frame.scroll.x += dx;
    let markers = frame.fold_markers.iter_mut().map(|marker| &mut marker.rect);
    let rects = (frame.selections.iter_mut())
        .chain(&mut frame.cursors)
        .chain(&mut frame.inlay)
        .chain(markers);
    for rect in rects {
        rect.x -= dx;
    }
}

/// The x, in the frame, of byte `index` of the inlay's text.
fn inlay_x(frame: &Frame<Line>, index: usize) -> Option<f32> {
    let inlay = frame.inlay?;
    let (row, shown) = inlay_row(frame)?;
    let x = |index: usize| row.line.x_for_index(shown.start + index.min(shown.len()));
    Some(inlay.x + x(index) - x(0))
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

/// Clusters [`GlyphWidths`] holds before it drops them all.
const MAX_CACHED_CLUSTERS: usize = 4096;

/// Char and grapheme cluster widths in the editor font, in pixels, for soft
/// wrap and long rows. Shapes each char and multi-char cluster once, so
/// fallback fonts are measured too, and so are clusters drawn as one glyph,
/// such as emoji sequences. Clusters are cached by a hash of their text, so
/// a long one isn't kept.
#[derive(Clone)]
pub(crate) struct GlyphWidths {
    text_system: Arc<TextSystem>,
    font: Font,
    font_id: FontId,
    font_size: Pixels,
    cache: HashMap<char, f32>,
    clusters: HashMap<u64, f32>,
}

impl GlyphWidths {
    pub(crate) fn new(text_system: Arc<TextSystem>, font: &Font, font_size: Pixels) -> Self {
        Self {
            font_id: text_system.resolve_font(font),
            text_system,
            font: font.clone(),
            font_size,
            cache: HashMap::new(),
            clusters: HashMap::new(),
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
            ..
        } = self;
        *cache
            .entry(ch)
            .or_insert_with(|| text_system.layout_width(*font_id, *font_size, ch).into())
    }

    /// Shapes `cluster` whole, unless it's longer than
    /// [`MAX_CLUSTER_BYTES`], such as thousands of stacked combining marks:
    /// then it sums its chars' widths rather than shape it, as the row index
    /// does for such clusters, so the work stays bounded.
    fn cluster_width(&mut self, cluster: &str) -> f32 {
        let mut chars = cluster.chars();
        if let (Some(ch), None) = (chars.next(), chars.next()) {
            return self.char_width(ch);
        }
        if cluster.len() > MAX_CLUSTER_BYTES {
            return cluster.chars().map(|ch| self.char_width(ch)).sum();
        }
        let key = {
            let mut hasher = DefaultHasher::new();
            cluster.hash(&mut hasher);
            hasher.finish()
        };
        if let Some(&width) = self.clusters.get(&key) {
            return width;
        }
        if self.clusters.len() >= MAX_CACHED_CLUSTERS {
            self.clusters.clear();
        }
        // A text system of its own, whose line cache goes with it, rather
        // than a window's, which would keep each cluster until its frame
        // ends.
        let shaper = WindowTextSystem::new(self.text_system.clone());
        let run = TextRun {
            len: cluster.len(),
            font: self.font.clone(),
            color: Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let width = shaper
            .layout_line(cluster, self.font_size, &[run], None)
            .width
            .into();
        self.clusters.insert(key, width);
        width
    }
}

#[cfg(test)]
mod glyph_width_tests {
    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn clusters_are_shaped_whole_up_to_the_limit(cx: &mut TestAppContext) {
        let text_system = cx.text_system().clone();
        let font = gpui::font("Menlo");
        let mut widths = GlyphWidths::new(text_system.clone(), &font, px(13.));
        let shaper = WindowTextSystem::new(text_system);
        // A combining enclosing circle is 3 bytes.
        let longest = format!("a{}", "\u{20dd}".repeat((MAX_CLUSTER_BYTES - 1) / 3));
        assert_eq!(longest.len(), MAX_CLUSTER_BYTES);
        for cluster in ["e\u{301}".to_string(), longest] {
            let run = TextRun {
                len: cluster.len(),
                font: font.clone(),
                color: Hsla::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let whole: f32 = shaper
                .layout_line(&cluster, px(13.), &[run], None)
                .width
                .into();
            assert_eq!(widths.cluster_width(&cluster), whole);
        }
        // Cached by hash: neither cluster's text is kept.
        assert_eq!(widths.clusters.len(), 2);
        assert_eq!(widths.cluster_width("x"), widths.char_width('x'));
        assert_eq!(widths.clusters.len(), 2);

        // A longer one is measured char by char, not shaped or cached.
        let long = format!("a{}", "\u{20dd}".repeat(1 << 16));
        let summed = long.chars().map(|ch| widths.char_width(ch)).sum::<f32>();
        assert_eq!(widths.cluster_width(&long), summed);
        assert_eq!(widths.clusters.len(), 2);
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
        self.view.update(cx, |view, cx| {
            let rows = view.model.text().row_count();
            view.model
                .fit_wrap_width(layout::wrap_width(width, rows, em));
            view.model.sync_display();
            view.sync_visual();
            // Caught up with edits made through another view of the buffer:
            // count them.
            view.sync_find(cx);
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
        // What an input method is composing, drawn at its caret.
        let composition = view.composition();
        let inlay = composition.map(|(offset, composition)| layout::Inlay {
            offset,
            // An inlay is one line.
            text: composition.text.replace(['\n', '\r'], " "),
            style: theme.foreground,
        });
        let mut lay_out = |input: &FrameInput<Hsla>| {
            layout::layout_frame_with_inlay(
                input,
                &BlockMap::default(),
                inlay.as_ref(),
                &mut shaper,
                &mut long_rows,
                &mut glyph_widths,
                |range| styled_highlights(syntax, theme, range),
                |rows| fold_headers(syntax, rows),
            )
        };
        let mut frame = lay_out(&input);
        // Autoscroll keeps the buffer caret in view, which is before the
        // inlay. While composing, the composition's caret inside it stays in
        // view too, as far inside the edge as autoscroll keeps a caret: the
        // frame scrolls further, then shifts the rest of the way where layout
        // doesn't scroll so far. Wrapped rows don't scroll, and a long row's
        // limit leaves the inlay out.
        let caret = composition.map(|(_, composition)| composition.selected.end);
        let past_edge = |frame: &Frame<Line>| {
            // A text area narrower than the margin keeps the whole caret in
            // it.
            let text = frame.text_bounds();
            let caret_width = frame.cursors.first().map_or(2., |caret| caret.width);
            let margin = (em * 4.).clamp(caret_width, text.width.max(caret_width));
            let edge = text.x + text.width - margin;
            let x = inlay_x(frame, caret?)?;
            (x > edge).then_some(x - edge)
        };
        // The scroll to keep for the next frame, where this one shifts past it.
        let mut kept_scroll = None;
        if let Some(past) = past_edge(&frame) {
            if model.display().wrap_width().is_none() {
                let scrolled = lay_out(&FrameInput {
                    scroll: Vec2 {
                        x: frame.scroll.x + past,
                        y: frame.scroll.y,
                    },
                    autoscroll: false,
                    ..input
                });
                // A long row's slice goes by buffer x, so scrolling a wide
                // composition's caret into view can leave the slice past the
                // composition's anchor, and the inlay out of the frame. Keep
                // the frame that has it, and shift it the rest of the way:
                // the next frame starts from the same scroll, not this one.
                if scrolled.inlay.is_some() {
                    frame = scrolled;
                } else {
                    kept_scroll = Some(frame.scroll);
                    // The other long rows' slices were shaped for the scroll
                    // this frame leaves; take theirs from the frame laid out
                    // for where it ends up, which is `past` further on.
                    let others = frame.rows.iter_mut().zip(scrolled.rows);
                    for (row, shaped) in others {
                        let long = row.slice.is_some() && row.expanded.inlay_range().is_none();
                        if long && row.row == shaped.row && row.start == shaped.start {
                            *row = shaped;
                        }
                    }
                }
            }
            if let Some(past) = past_edge(&frame) {
                shift_frame(&mut frame, past);
            }
        }
        let composed = composition.map(|(offset, composition)| (offset, composition.text.clone()));
        let to_window = |rect: Rect| {
            Bounds::new(
                bounds.origin + point(px(rect.x), px(rect.y)),
                size(px(rect.width), px(rect.height)),
            )
        };
        let fold_column =
            window.insert_hitbox(to_window(frame.fold_column), HitboxBehavior::Normal);
        let clickable = frame
            .fold_click_areas()
            .map(|area| window.insert_hitbox(to_window(area.rect), HitboxBehavior::Normal))
            .collect();
        let text = window.insert_hitbox(to_window(frame.text_bounds()), HitboxBehavior::Normal);

        let kept = kept_scroll.unwrap_or(frame.scroll);
        let scroll = point(px(kept.x), px(kept.y));
        self.view.update(cx, |view, cx| {
            // Scrolled to reveal the caret, say: what the popover was found
            // under has moved, and it isn't drawn from this frame on
            // ([`crate::hover::popover_layer`]).
            if view.scroll != scroll {
                view.hide_popover(cx);
            }
            view.scroll = scroll;
            view.autoscroll = false;
            view.load_injected_languages(cx);
        });
        let view = self.view.read(cx);
        let mut layout = FrameLayout {
            origin: bounds.origin,
            frame,
            long_rows,
            glyph_widths,
            tab_size,
            version: view.model.snapshot().version(),
            fold_column,
            clickable,
            text,
            matches: Vec::new(),
            brackets: Vec::new(),
            jump: None,
            composition: composed,
            decorations: Decorations::default(),
            prepaint_started: started,
            prepaint_took: Duration::ZERO,
        };
        layout.matches = match_rects(&mut layout, view);
        layout.brackets = bracket_rects(&mut layout, view);
        layout.jump = jump_layout(&mut layout, view);
        layout.decorations = Decorations::layout(&mut layout, view, em, window);
        // The completion menu goes by its word's start in this frame, which
        // the view doesn't hold until paint.
        let word = self.view.read(cx).completion_word_start();
        let word = word.and_then(|start| layout.caret_bounds(start));
        let (viewport, line_height) = (window.viewport_size(), px(layout.frame.line_height));
        self.view.update(cx, |view, cx| {
            view.place_completion(word, viewport, line_height, cx)
        });
        layout.prepaint_took = started.elapsed();
        Some(layout)
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
        let accepts_text = view.accepts_text();
        let column_hovered = view.fold_column_hovered;
        let mut layout = layout.take().expect("prepaint ran");
        let shape = view.caret_shape();
        let block = shape != CaretShape::Bar;
        let focused = focus.is_focused(window);
        // Not in an inactive window: modifiers released in another app, as
        // after cmd-tab, never reach it.
        let hover_shown = window.is_window_active();
        let hover = hover_shown.then(|| view.hover_range()).flatten();
        let underlines = hover.as_ref().map_or_else(Vec::new, |range| {
            underlines(&mut layout, range, theme.foreground)
        });
        let carets: Vec<_> = match focused {
            false => Vec::new(),
            true if block => {
                let text = view.model.text();
                // In a visual mode the block is the vim cursor, which for a
                // forward charwise selection is a grapheme before the head.
                let visual = view
                    .vim
                    .as_ref()
                    .filter(|vim| vim.mode_name().starts_with("visual"))
                    .and_then(|vim| vim.visual_cursor(&view.model));
                let heads: Vec<usize> = match visual {
                    Some(cursor) => vec![cursor],
                    None => view
                        .model
                        .selections()
                        .into_iter()
                        .map(|s| s.head())
                        .collect(),
                };
                let carets = block_carets(&mut layout, text, heads.into_iter());
                match shape {
                    // Replace and operator-pending mode's: a bar under the
                    // grapheme the block covers (§9 item 7).
                    CaretShape::Underline => (carets.into_iter())
                        .map(|block| {
                            let height = px(UNDERLINE_HEIGHT).min(block.size.height);
                            Bounds::new(
                                point(block.origin.x, block.bottom() - height),
                                size(block.size.width, height),
                            )
                        })
                        .collect(),
                    _ => carets,
                }
            }
            true => {
                let mut carets: Vec<_> = (layout.frame.cursors.iter())
                    .map(|&cursor| layout.bounds(cursor))
                    .collect();
                // Composing, the primary caret is the composition's.
                if let Some((at, composition)) = view.composition()
                    && let Some(primary) = layout.caret_bounds(at)
                    && let end = composition.selected.end
                    && let Some(caret) = layout.inlay_bounds(end..end)
                {
                    // A frame shifted for the composition may round it.
                    let near = |caret: &Bounds<Pixels>| {
                        caret.origin.y == primary.origin.y
                            && (caret.origin.x - primary.origin.x).abs() < px(0.5)
                    };
                    carets.retain(|caret| !near(caret));
                    carets.push(Bounds::new(caret.origin, primary.size));
                }
                carets
            }
        };
        // Composed text is underlined in the text colour.
        let composition_underline = layout.frame.inlay.map(|inlay| {
            let thickness = COMPOSITION_UNDERLINE;
            layout.bounds(Rect {
                y: inlay.y + inlay.height - thickness,
                height: thickness,
                ..inlay
            })
        });
        let selections = selection_rects(&mut layout, view);
        let matches = mem::take(&mut layout.matches);
        let brackets = mem::take(&mut layout.brackets);
        let (squiggles, marks) = {
            let diagnostics = view.model.diagnostics();
            diagnostic_marks(&mut layout, view.model.text(), &diagnostics, &theme)
        };
        let jump = layout.jump.as_ref().map(|jump| {
            let labels: Vec<_> = (jump.labels.iter())
                .map(|label| (layout.bounds(label.rect), label.text.clone(), label.state))
                .collect();
            (
                jump.backdrop.map(|backdrop| layout.bounds(backdrop)),
                labels,
            )
        });
        // The underline paints over the text, as the bar does.
        let block = shape == CaretShape::Block;
        // Over a backdrop, so it isn't dimmed; under the text otherwise.
        let block_on_top = block
            && jump
                .as_ref()
                .is_some_and(|(backdrop, _)| backdrop.is_some());
        #[cfg(test)]
        JUMP_PAINTS.with(|paints| paints.borrow_mut().clear());
        let frame = &layout.frame;
        let line_height = px(frame.line_height);
        #[cfg(test)]
        let mut painted = Vec::new();
        #[cfg(test)]
        let mut painted_selections = Vec::new();
        #[cfg(test)]
        let mut painted_matches = Vec::new();
        #[cfg(test)]
        let mut painted_chevrons = Vec::new();
        #[cfg(test)]
        let mut painted_brackets = Vec::new();

        // In vim's normal mode nothing types: neither keys nor an input
        // method insert text.
        if accepts_text {
            window.handle_input(
                &focus,
                ElementInputHandler::new(bounds, self.view.clone()),
                cx,
            );
        }
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
                    layout.decorations.paint(Layer::Backgrounds, window, cx);
                    // Under the selection, which stays visible over a match.
                    for &found in &matches {
                        let found = layout.bounds(found);
                        window.paint_quad(fill(found, search_match_color(&theme)));
                        #[cfg(test)]
                        if found.intersects(&text_bounds) {
                            painted_matches.push(found);
                            decorations::note("matches");
                        }
                    }
                    for &selection in &selections {
                        let selection = layout.bounds(selection);
                        window.paint_quad(fill(selection, theme.selection));
                        #[cfg(test)]
                        painted_selections.push(selection);
                        #[cfg(test)]
                        decorations::note("selection");
                    }
                    for &bracket in &brackets {
                        let bracket = layout.bounds(bracket);
                        window.paint_quad(outline(
                            bracket,
                            theme.cursor.opacity(0.5),
                            BorderStyle::Solid,
                        ));
                        #[cfg(test)]
                        painted_brackets.push(bracket);
                    }
                    // Under the text, so the glyph stays readable.
                    if block && !block_on_top {
                        for &caret in &carets {
                            window.paint_quad(fill(caret, theme.cursor.opacity(BLOCK_ALPHA)));
                            #[cfg(test)]
                            painted.push(caret);
                        }
                    }
                    for row in &frame.rows {
                        let origin = layout.to_window(frame.line_origin(row));
                        row.line.paint(origin, line_height, window, cx);
                    }
                    #[cfg(test)]
                    decorations::note("text");
                    for marker in &frame.fold_markers {
                        let rect = marker.rect;
                        let origin = layout.to_window(Vec2 {
                            x: rect.x,
                            y: rect.y,
                        });
                        marker.line.paint(origin, line_height, window, cx);
                    }
                    for (squiggle, style) in &squiggles {
                        window.paint_underline(squiggle.origin, squiggle.size.width, style);
                    }
                    layout.decorations.paint(Layer::Underlines, window, cx);
                    layout.decorations.paint(Layer::AfterLine, window, cx);
                    for &(underline, color) in &underlines {
                        window.paint_quad(fill(underline, color));
                    }
                    if let Some(underline) = composition_underline {
                        window.paint_quad(fill(underline, theme.foreground));
                    }
                    if let Some((backdrop, labels)) = &jump {
                        paint_jump(*backdrop, labels, &theme, window, cx);
                    }
                    if block_on_top {
                        for &caret in &carets {
                            window.paint_quad(fill(caret, theme.cursor.opacity(BLOCK_ALPHA)));
                            #[cfg(test)]
                            painted.push(caret);
                            #[cfg(test)]
                            JUMP_PAINTS.with(|paints| {
                                paints
                                    .borrow_mut()
                                    .push(JumpPaint::BlockCaret { bounds: caret })
                            });
                        }
                    }
                    if !block {
                        for &caret in &carets {
                            window.paint_quad(fill(caret, theme.cursor));
                            #[cfg(test)]
                            painted.push(caret);
                        }
                    }
                },
            );
            for (origin, number) in &frame.line_numbers {
                number.paint(layout.to_window(*origin), line_height, window, cx);
            }
            for &(_, mark, color) in &marks {
                window.paint_quad(fill(mark, color).corner_radii(mark.size.width / 2.));
            }
            layout.decorations.paint(Layer::Gutter, window, cx);
            // Folded rows' chevrons always show; the rest only while the
            // pointer is over the fold column.
            for toggle in &frame.fold_toggles {
                if !toggle.folded && !column_hovered {
                    continue;
                }
                let chevron = layout.bounds(frame.chevron_box(toggle));
                paint_chevron(chevron, toggle.folded, theme.gutter_foreground, window);
                #[cfg(test)]
                painted_chevrons.push((toggle.buffer_row, toggle.folded, chevron));
            }
        });
        for hitbox in &layout.clickable {
            window.set_cursor_style(CursorStyle::PointingHand, hitbox);
        }
        if hover.is_some() {
            window.set_cursor_style(CursorStyle::PointingHand, &layout.text);
        }
        // Window-wide, so leaving the editor for another part of the window
        // clears the flag too. Every event that places the pointer counts,
        // not just moves: a window can move under a still pointer and then
        // be clicked or scrolled.
        let hitbox = layout.fold_column.clone();
        let view = self.view.downgrade();
        on_pointer_event::<MouseMoveEvent>(&view, &hitbox, window);
        on_pointer_event::<MouseDownEvent>(&view, &hitbox, window);
        on_pointer_event::<MouseUpEvent>(&view, &hitbox, window);
        on_pointer_event::<ScrollWheelEvent>(&view, &hitbox, window);
        on_pointer_event::<PinchEvent>(&view, &hitbox, window);
        {
            let view = view.clone();
            window.on_mouse_event(move |_: &MouseExitEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    let id = window.window_handle().window_id();
                    cx.default_global::<PointerOutside>().0.insert(id);
                    set_fold_column_hovered(&view, false, cx);
                    view.update(cx, |view, cx| {
                        view.set_hover(None, cx);
                        view.hide_popover(cx);
                    })
                    .ok();
                }
            });
        }
        // Window-wide too, so moving off the text anywhere clears the hover
        // range and the popover. Only moves find them: a scroll clears
        // them, and a click leaves them be.
        {
            let (view, text) = (view.clone(), layout.text.clone());
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    let over = text
                        .is_hovered_at(event.position, window)
                        .then_some(event.position);
                    view.update(cx, |view, cx| {
                        view.hover_at(over, &event.modifiers, window, cx);
                        view.hover_diagnostics(over, cx);
                    })
                    .ok();
                }
            });
        }
        add_hover_editor(&view, window, cx);
        // Once this frame is the rendered one, check the column against the
        // still pointer, so a column moving under or away from it (a dock
        // opening, the gutter widening) shows or hides the chevrons; and the
        // hover range, which may have moved from under it or come unbound.
        window.defer(cx, move |window, cx| {
            reconcile_fold_column(&view, &hitbox, window, cx);
            // GPUI's context stack reads the rendered frame, empty before
            // the first one.
            view.update(cx, |view, cx| {
                view.note_ancestors(window);
                view.reconcile_hover(window, cx);
            })
            .ok();
        });

        let painted_at = Instant::now();
        let timing = FrameTiming {
            prepaint_started: layout.prepaint_started,
            prepaint: layout.prepaint_took,
            paint: painted_at - started,
            painted_at,
        };
        self.view.update(cx, |view, _| {
            view.layout = Some(layout);
            view.scrolled_top = None;
            view.scrolled_display_top = None;
            view.viewport_height_changed();
            view.frame_timings.record(timing);
            view.drop_stale_hover(hover_shown);
            #[cfg(test)]
            {
                view.painted_carets = painted;
                view.painted_selections = painted_selections;
                view.painted_matches = painted_matches;
                view.painted_chevrons = painted_chevrons;
                view.painted_brackets = painted_brackets;
                let underlines = underlines.iter().map(|&(bounds, _)| bounds).collect();
                view.painted_hover = (underlines, hover.is_some());
                view.painted_squiggles = squiggles;
                view.painted_composition = composition_underline;
                view.painted_marks = marks.iter().map(|&(row, _, color)| (row, color)).collect();
            }
        });
    }
}

/// The windows the pointer has left and not moved in since, as the editors'
/// listeners saw it. GPUI keeps the last position after the pointer leaves.
#[derive(Default)]
struct PointerOutside(HashSet<WindowId>);

impl Global for PointerOutside {}

/// On each `E`, which places the pointer in the window, reconciles the
/// view's fold column hover with it.
fn on_pointer_event<E: MouseEvent>(
    view: &WeakEntity<EditorView>,
    column: &Hitbox,
    window: &mut Window,
) {
    let (view, column) = (view.clone(), column.clone());
    window.on_mouse_event(move |_: &E, phase, window, cx| {
        if phase == DispatchPhase::Bubble {
            let id = window.window_handle().window_id();
            cx.default_global::<PointerOutside>().0.remove(&id);
            reconcile_fold_column(&view, &column, window, cx);
        }
    });
}

/// The editors drawn in each window, as weak handles, for
/// [`modifiers_changed`].
#[derive(Default)]
struct HoverEditors(HashMap<WindowId, Vec<WeakEntity<EditorView>>>);

impl Global for HoverEditors {}

/// Adds `view` to the editors drawn in `window`, dropping those released.
fn add_hover_editor(view: &WeakEntity<EditorView>, window: &Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    let editors = cx.default_global::<HoverEditors>().0.entry(id).or_default();
    editors.retain(|editor| editor.upgrade().is_some());
    if !editors.iter().any(|editor| editor == view) {
        editors.push(view.clone());
    }
}

/// A modifier pressed or released, which GPUI tells only the focused
/// element's path: the focused editor passes it on to every editor drawn in
/// its window, as the pointer may be over another. While a control that
/// isn't an editor has focus, such as a panel, the host's root element
/// passes it on ([`EditorView::modifiers_changed`]); gpui-pre 0.3.7 has no
/// window-wide modifier listener. A release alone is also a keystroke,
/// which `crate::keymap` passes on from anywhere.
pub(crate) fn modifiers_changed(event: &ModifiersChangedEvent, window: &mut Window, cx: &mut App) {
    hover_with_modifiers(&event.modifiers, window, cx);
}

/// Finds every editor's hover range drawn in `window` again with
/// `modifiers` held. One no longer drawn finds no pointer over its text, so
/// it only clears its hover range.
pub(crate) fn hover_with_modifiers(modifiers: &gpui::Modifiers, window: &Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    let editors = cx
        .try_global::<HoverEditors>()
        .and_then(|editors| editors.0.get(&id).cloned())
        .unwrap_or_default();
    for editor in editors.iter().filter_map(WeakEntity::upgrade) {
        editor.update(cx, |editor, cx| {
            editor.hover_at_pointer(modifiers, window, cx)
        });
    }
}

/// The pointer's window position, if it's over the text of the rendered
/// frame, uncovered, and hasn't left the window.
pub(crate) fn pointer_over_text(
    layout: &FrameLayout,
    window: &Window,
    cx: &App,
) -> Option<Point<Pixels>> {
    let id = window.window_handle().window_id();
    let outside = cx
        .try_global::<PointerOutside>()
        .is_some_and(|outside| outside.0.contains(&id));
    let position = window.mouse_position();
    (!outside && layout.text.is_hovered_at(position, window)).then_some(position)
}

/// A 1 px underline under each piece of `range` on the frame's rows, just
/// below the baseline, in the colour of the text it starts under, else
/// `color`.
fn underlines(
    layout: &mut FrameLayout,
    range: &Range<usize>,
    color: Hsla,
) -> Vec<(Bounds<Pixels>, Hsla)> {
    let FrameLayout {
        frame,
        long_rows,
        glyph_widths,
        ..
    } = layout;
    let mut pieces = Vec::new();
    for row in &frame.rows {
        let (start, end) = (range.start.max(row.start), range.end.min(row.end));
        if start >= end {
            continue;
        }
        let left = frame.view_x(row.x_for_offset(start, long_rows, glyph_widths));
        let right = frame.view_x(row.x_for_offset(end, long_rows, glyph_widths));
        let line = &row.line.layout;
        let (ascent, descent): (f32, f32) = (line.ascent.into(), line.descent.into());
        let baseline =
            frame.row_top(row.row) + (frame.line_height - ascent - descent) / 2. + ascent;
        let shown = row.shown();
        let color = (shown.start..shown.end)
            .contains(&start)
            .then(|| {
                row.line
                    .color_at(row.expanded.to_display(start - shown.start))
            })
            .flatten()
            .unwrap_or(color);
        let rect = Rect {
            x: left,
            y: baseline.round() + 1.,
            width: right - left,
            height: 1.,
        };
        pieces.push((rect, color));
    }
    pieces
        .into_iter()
        .map(|(rect, color)| (layout.bounds(rect), color))
        .collect()
}

/// How thick a diagnostic's wavy underline is; its wave is three times
/// as tall.
const SQUIGGLE: f32 = 1.;

/// A squiggle: where it starts and how wide it is, in window coordinates,
/// its height the stroke's thickness, and its style.
type Squiggle = (Bounds<Pixels>, UnderlineStyle);

/// A gutter mark: the buffer row, its box in window coordinates, and its
/// colour.
type Mark = (u32, Bounds<Pixels>, Hsla);

/// For `diagnostics`, in the frame's text: a wavy underline under each
/// piece of each one on the frame's rows, in its severity's colour, the most
/// severe last, so on top; and, at the gutter's left, a dot on the first
/// display row of each buffer row one covers, in the colour of the most
/// severe covering it. An empty range is underlined under the char after
/// it, or a space's width at a row's end.
fn diagnostic_marks(
    layout: &mut FrameLayout,
    text: &Text,
    diagnostics: &[fe_editor::diagnostics::DiagnosticEntry<'_>],
    theme: &EditorTheme,
) -> (Vec<Squiggle>, Vec<Mark>) {
    let FrameLayout {
        frame,
        long_rows,
        glyph_widths,
        ..
    } = layout;
    let (Some(first), Some(last)) = (frame.rows.first(), frame.rows.last()) else {
        return (Vec::new(), Vec::new());
    };
    let shown = first.start..last.end;
    let mut pieces = Vec::new();
    let mut rows: HashMap<u32, fe_editor::diagnostics::Severity> = HashMap::new();
    for entry in diagnostics {
        let range = entry.range.clone();
        if range.start > shown.end || range.end < shown.start {
            continue;
        }
        // Every shown buffer row it covers: through the row its end is
        // on, unless that end is the row's start.
        let first_row = text.offset_to_point(range.start).row;
        let mut last_row = text.offset_to_point(range.end).row;
        if last_row > first_row && range.end == text.row_start(last_row) {
            last_row -= 1;
        }
        let covered = first_row.max(first.buffer_row)..=last_row.min(last.buffer_row);
        for row in covered {
            let worst = rows.entry(row).or_insert(entry.severity);
            *worst = (*worst).max(entry.severity);
        }
        let end = match range.is_empty() {
            true => text.clip_offset(range.end + 1, fe_buffer::Bias::Right),
            false => range.end,
        };
        for row in &frame.rows {
            let (start, end) = (range.start.max(row.start), end.min(row.end));
            // An empty one only on the row its caret would be on, not
            // also at the start of the next.
            let on_row = match range.is_empty() {
                true => row.contains(range.start),
                false => start < end && row.contains(start),
            };
            if !on_row {
                continue;
            }
            let left = frame.view_x(row.x_for_offset(start, long_rows, glyph_widths));
            let right = match start < end {
                true => frame.view_x(row.x_for_offset(end, long_rows, glyph_widths)),
                false => left + glyph_widths.char_width(' '),
            };
            let line = &row.line.layout;
            let (ascent, descent): (f32, f32) = (line.ascent.into(), line.descent.into());
            let baseline =
                frame.row_top(row.row) + (frame.line_height - ascent - descent) / 2. + ascent;
            let rect = Rect {
                x: left,
                y: baseline.round() + 1.,
                width: right - left,
                height: SQUIGGLE,
            };
            pieces.push((entry.severity, rect));
        }
    }
    pieces.sort_by_key(|(severity, _)| *severity);
    let side = (frame.line_height * 0.35).round();
    let marks: Vec<(u32, Rect, Hsla)> = (frame.rows.iter())
        .filter(|row| row.start == text.row_start(row.buffer_row))
        .filter_map(|row| {
            let severity = rows.get(&row.buffer_row)?;
            let rect = Rect {
                x: side / 2.,
                y: frame.row_top(row.row) + (frame.line_height - side) / 2.,
                width: side,
                height: side,
            };
            Some((row.buffer_row, rect, theme.diagnostic_color(*severity)))
        })
        .collect();
    let squiggles = (pieces.into_iter())
        .map(|(severity, rect)| {
            let style = UnderlineStyle {
                thickness: px(rect.height),
                color: Some(theme.diagnostic_color(severity)),
                wavy: true,
            };
            (layout.bounds(rect), style)
        })
        .collect();
    let marks = (marks.into_iter())
        .map(|(row, rect, color)| (row, layout.bounds(rect), color))
        .collect();
    (squiggles, marks)
}

/// Sets whether the pointer is over `column` in the rendered frame. Unlike
/// [`Hitbox::is_hovered`], this holds after a key press too (GPUI's
/// keyboard modality suppresses hover, but a key doesn't move the pointer),
/// while still honouring clipping and whatever covers the column.
fn reconcile_fold_column(
    view: &WeakEntity<EditorView>,
    column: &Hitbox,
    window: &Window,
    cx: &mut App,
) {
    let id = window.window_handle().window_id();
    let outside = cx
        .try_global::<PointerOutside>()
        .is_some_and(|outside| outside.0.contains(&id));
    let hovered = !outside && column.is_hovered_at(window.mouse_position(), window);
    set_fold_column_hovered(view, hovered, cx);
}

fn set_fold_column_hovered(view: &WeakEntity<EditorView>, hovered: bool, cx: &mut App) {
    view.update(cx, |view, cx| {
        if view.fold_column_hovered != hovered {
            view.fold_column_hovered = hovered;
            cx.notify();
        }
    })
    .ok();
}

/// The three points of a chevron stroked in `chevron`: pointing down for a
/// fold that's open, right for a folded one. It spans 60% of the box across
/// and 30% along.
pub(crate) fn chevron_points(chevron: Bounds<Pixels>, folded: bool) -> [Point<Pixels>; 3] {
    let side = chevron.size.width;
    let centre = chevron.center();
    let (across, along) = (side * 0.3, side * 0.15);
    let at = |x: Pixels, y: Pixels| centre + point(x, y);
    if folded {
        [at(-along, -across), at(along, px(0.)), at(-along, across)]
    } else {
        [at(-across, -along), at(px(0.), along), at(across, -along)]
    }
}

fn paint_chevron(chevron: Bounds<Pixels>, folded: bool, color: Hsla, window: &mut Window) {
    let [start, tip, end] = chevron_points(chevron, folded);
    let mut path = PathBuilder::stroke((chevron.size.width / 12.).max(px(1.)));
    path.move_to(start);
    path.line_to(tip);
    path.line_to(end);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// How opaque a block cursor is, over the cursor colour.
const BLOCK_ALPHA: f32 = 0.6;

/// How thick the underline under composed text is.
const COMPOSITION_UNDERLINE: f32 = 1.;

/// Vim's normal-mode cursor at each of `heads` on screen: a block over the
/// grapheme after it ([`block_rect`]).
fn block_carets(
    layout: &mut FrameLayout,
    text: &Text,
    heads: impl Iterator<Item = usize>,
) -> Vec<Bounds<Pixels>> {
    let blocks: Vec<Rect> = heads
        .filter_map(|head| block_rect(layout, text, head))
        .collect();
    blocks.into_iter().map(|rect| layout.bounds(rect)).collect()
}

/// A block over the grapheme after `head`, to the next grapheme boundary on
/// its display row, or one space wide at a line's end; `None` off screen.
fn block_rect(layout: &mut FrameLayout, text: &Text, head: usize) -> Option<Rect> {
    let FrameLayout {
        frame,
        long_rows,
        glyph_widths,
        ..
    } = layout;
    let row = frame.row_containing(head)?;
    let bar = frame.caret_bounds(head, long_rows, glyph_widths)?;
    let line = text.offset_to_point(head).row;
    let line_end = text.row_start(line) + text.row_len(line) as usize;
    // Measured on the head's row: a wrap point's caret is on the next.
    let end = next_grapheme(text, head, row.end.min(line_end))
        .map(|next| frame.view_x(row.x_for_offset(next, long_rows, glyph_widths)))
        .filter(|&end| end > bar.x);
    let width = end.map_or_else(|| glyph_widths.char_width(' '), |end| end - bar.x);
    Some(Rect { width, ..bar })
}

/// The frame's selection rects. In a vim visual mode, an empty row the
/// visual extent takes shows as one space wide, as nvim shows it, rather
/// than as the narrower mark of a selected line break; in a block, which
/// selects nothing on an empty row, if the block starts at column 0.
fn selection_rects(layout: &mut FrameLayout, view: &EditorView) -> Vec<Rect> {
    let mut rects = layout.frame.selections.clone();
    let model = &view.model;
    let text = model.text();
    let empty = |row: &fe_display::layout::RowLayout<_>| text.row_len(row.buffer_row) == 0;
    if !layout.frame.rows.iter().any(empty) {
        return rects;
    }
    let Some((anchor, cursor, block)) = view.vim.as_ref().and_then(|vim| {
        let block = vim.mode() == fe_vim::Mode::Visual(fe_vim::VisualKind::Block);
        Some((vim.visual_anchor(model)?, vim.visual_cursor(model)?, block))
    }) else {
        return rects;
    };
    let row_of = |offset: usize| text.offset_to_point(offset).row;
    let at_column_0 = |offset: usize| offset == text.row_start(row_of(offset));
    if block && !at_column_0(anchor) && !at_column_0(cursor) {
        return rects;
    }
    let rows = row_of(anchor.min(cursor))..=row_of(anchor.max(cursor));
    let FrameLayout {
        frame,
        long_rows,
        glyph_widths,
        ..
    } = layout;
    let space = glyph_widths.char_width(' ');
    for row in &frame.rows {
        if !rows.contains(&row.buffer_row) || !empty(row) {
            continue;
        }
        let Some(bar) = frame.caret_bounds(row.start, long_rows, glyph_widths) else {
            continue;
        };
        rects.retain(|rect| (rect.y - bar.y).abs() >= 0.5);
        rects.push(Rect {
            width: space,
            ..bar
        });
    }
    rects
}

/// The rects of the bracket pair at the caret: none unless the only
/// selection is a caret and the syntax tree is for the text drawn. A bracket
/// on a row that isn't shown, or in a fold, has none; the other still does.
fn bracket_rects(layout: &mut FrameLayout, view: &EditorView) -> Vec<Rect> {
    let model = &view.model;
    let selection = model.primary_selection();
    if !selection.range.is_empty() || model.selections().len() != 1 {
        return Vec::new();
    }
    let Some(syntax) = view
        .syntax
        .as_ref()
        .map(SyntaxDriver::syntax)
        .filter(|syntax| syntax.version() == model.snapshot().version())
        // An edited tree has its offsets moved but not its structure.
        .filter(|syntax| !syntax.needs_parse())
    else {
        return Vec::new();
    };
    let Some(pair) = fe_syntax::bracket_pair(syntax, selection.head()) else {
        return Vec::new();
    };
    let FrameLayout {
        frame,
        long_rows,
        glyph_widths,
        ..
    } = layout;
    [pair.open, pair.close]
        .into_iter()
        .filter_map(|range| {
            let row = frame.row_containing(range.start)?;
            let left = frame.view_x(row.x_for_offset(range.start, long_rows, glyph_widths));
            let right = frame.view_x(row.x_for_offset(range.end, long_rows, glyph_widths));
            Some(Rect {
                x: left,
                y: frame.row_top(row.row),
                width: right - left,
                height: frame.line_height,
            })
        })
        .collect()
}

/// The most search matches painted in a frame.
pub(crate) const MAX_MATCHES: usize = 1_000;

/// The theme's search-match colour, for vim's matches and the find
/// pattern's: its foreground, faint, until the theme has a slot for it.
fn search_match_color(theme: &EditorTheme) -> Hsla {
    theme.foreground.opacity(0.22)
}

#[cfg(test)]
thread_local! {
    /// How many matches the last [`match_rects`] on this thread looked at.
    pub(crate) static CANDIDATES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many match ranges the last [`match_rects`] on this thread kept:
    /// those reaching the displayed part of their row.
    pub(crate) static FOUND: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The rects of the search matches in the frame's displayed rows, at most
/// [`MAX_MATCHES`] of them, the first in view: the find pattern's
/// ([`EditorView::set_find`]) while one is set, else vim's, none with vim
/// off or after `:noh`. Folded rows aren't searched, and for vim's matches
/// off the displayed part of a wrapped or scrolled line don't use the
/// budget. A zero-width match is one space wide.
///
/// Each displayed buffer row's matches are enumerated once, from the row's
/// start, keeping those that reach what's displayed of it: from the text
/// area's left edge on its first displayed row to the right edge on its
/// last. The find pattern's [`MAX_MATCHES`] go to those, for the whole
/// frame, so matches scrolled off don't use them.
fn match_rects(layout: &mut FrameLayout, view: &EditorView) -> Vec<Rect> {
    // How many more ranges the frame may find: bounded for the find
    // pattern only.
    let clip = view.find_pattern().is_some();
    let (pattern, mut budget) = match view.find_pattern() {
        Some(pattern) => (Some(pattern), MAX_MATCHES),
        None => (
            view.vim.as_ref().and_then(|vim| vim.highlight()),
            usize::MAX,
        ),
    };
    let Some(pattern) = pattern else {
        return Vec::new();
    };
    let FrameLayout {
        frame,
        long_rows,
        glyph_widths,
        ..
    } = layout;
    #[cfg(test)]
    CANDIDATES.with(|n| n.set(0));
    #[cfg(test)]
    FOUND.with(|n| n.set(0));
    let text = view.model.text();
    let space = glyph_widths.char_width(' ');
    let visible = frame.text_bounds();
    let mut rects = Vec::new();
    // A buffer row's matches that reach the text area on its displayed rows,
    // kept for its other wrapped rows: found once a frame, searched from the
    // row's start for its context but only as far as the last displayed part.
    let mut whole: Option<(u32, Vec<Range<usize>>)> = None;
    let edge = space;
    let (left_x, right_x) = (visible.x - edge, visible.x + visible.width + edge);
    for (ix, row) in frame.rows.iter().enumerate() {
        if whole.as_ref().is_none_or(|(at, _)| *at != row.buffer_row) {
            let last = frame.rows[ix..]
                .iter()
                .take_while(|next| next.buffer_row == row.buffer_row)
                .last()
                .unwrap_or(row);
            // Conservative at the right: a row that fits entirely reaches
            // its end, else one character past the hit-test offset, which
            // clamps a wrapped row's last character and rounds a tab down.
            let fits =
                frame.view_x(last.x_for_offset(last.end, long_rows, glyph_widths)) <= right_x;
            let mut at = |shown: &fe_display::layout::RowLayout<_>, x: f32| {
                let y = frame.row_top(shown.row) + frame.line_height / 2.;
                frame.offset_at(text, Vec2 { x, y }, long_rows, glyph_widths)
            };
            let start = at(row, left_x);
            let end = match fits {
                true => last.end,
                false => {
                    let hit = at(last, right_x);
                    text.clip_offset(hit + 1, fe_buffer::Bias::Right)
                }
            };
            let window = start..end;
            let all =
                fe_vim::search::matches_reaching(text, row.buffer_row, window, pattern, budget);
            budget = budget.saturating_sub(all.len());
            #[cfg(test)]
            FOUND.with(|n| n.set(n.get() + all.len()));
            whole = Some((row.buffer_row, all));
        }
        let found = whole.as_ref().map_or(&[][..], |(_, all)| all.as_slice());
        // The matches ordered by position: from the first that reaches the
        // displayed part to the first that starts past it.
        let displayed = row.shown();
        let first = found.partition_point(|range| range.end < displayed.start);
        for range in found[first..]
            .iter()
            .take_while(|range| range.start <= displayed.end)
            .cloned()
        {
            #[cfg(test)]
            CANDIDATES.with(|n| n.set(n.get() + 1));
            let (start, end) = (range.start.max(row.start), range.end.min(row.end));
            let shown = match range.is_empty() {
                true => row.contains(range.start),
                false => start < end,
            };
            if !shown {
                continue;
            }
            let left = frame.view_x(row.x_for_offset(start, long_rows, glyph_widths));
            let width = match range.is_empty() {
                true => space,
                false => frame.view_x(row.x_for_offset(end, long_rows, glyph_widths)) - left,
            };
            // Off to a side of a scrolled line: not worth the budget.
            if left + width <= visible.x || left >= visible.x + visible.width {
                continue;
            }
            // The find pattern's are clipped to the text area; vim's
            // paint as they always have.
            let (left, width) = match clip {
                true => {
                    let right = (left + width).min(visible.x + visible.width);
                    let left = left.max(visible.x);
                    (left, right - left)
                }
                false => (left, width),
            };
            rects.push(Rect {
                x: left,
                y: frame.row_top(row.row),
                width,
                height: frame.line_height,
            });
            if rects.len() >= MAX_MATCHES {
                return rects;
            }
        }
    }
    rects
}

/// What a pending vim jump paints, in frame coordinates.
pub(crate) struct JumpLayout {
    /// Over the text area, dimming it once `{c1}` is typed; `None` before,
    /// so the text the user reads to pick a target isn't dimmed.
    pub(crate) backdrop: Option<Rect>,
    /// In text order.
    pub(crate) labels: Vec<JumpLabelLayout>,
}

/// One label of a [`JumpLayout`]: a box over its target's char, or for a
/// direct target, its char's block grown by [`JUMP_DIRECT_BORDER`] on
/// every side, for the border to ring it.
pub(crate) struct JumpLabelLayout {
    /// Its target's, for the tests.
    #[cfg(test)]
    pub(crate) offset: usize,
    pub(crate) rect: Rect,
    pub(crate) text: SharedString,
    pub(crate) state: JumpLabelState,
}

/// How opaque the backdrop of the theme's background is over the text: the
/// largest tenth that keeps body text at WCAG AA (4.5:1) in every bundled
/// theme (Dark 6.0, Light 4.59, Ember 6.6; 0.4 drops Light to 3.5), so the
/// text stays readable while the labels stand out.
const JUMP_BACKDROP_ALPHA: f32 = 0.3;

/// The width of a direct target's border, which rings its char's block
/// from outside so nothing covers the char.
const JUMP_DIRECT_BORDER: Pixels = px(2.);

/// While vim has a jump pending, its labels on the frame's rows
/// ([`fe_vim::VimState::jump_overlay`]), each a block over its target's
/// char (a direct target's ring around it), and a backdrop once a char is
/// typed. The rows are those the jump's session takes from this frame's
/// viewport ([`fe_vim::jump::view_rows`]), so the letters after `{c1}` are
/// the labels after `{c2}`: a partly shown last row has none. At most
/// [`MAX_MATCHES`] labels, live, preview and direct ones before concealed
/// ones, so a jump over more targets still shows the labels it waits for.
/// `None` with no jump pending. Labels off to a side of a scrolled line are
/// left out.
fn jump_layout(layout: &mut FrameLayout, view: &EditorView) -> Option<JumpLayout> {
    let vim = view.vim.as_ref()?;
    let frame = &layout.frame;
    // What [`EditorView::viewport`] reports once this frame is drawn.
    let rows = match frame.rows.is_empty() {
        true => 0..0,
        false => {
            let viewport = fe_editor::commands::Viewport {
                top_row: view.model.display().row(frame.first_row).buffer_row,
                rows: frame.visible_rows(),
            };
            fe_vim::jump::view_rows(&view.model, viewport, Some(frame.first_row))
        }
    };
    let overlay = vim.jump_overlay(&view.model, rows)?;
    let text = view.model.text();
    let visible = frame.text_bounds();
    let ring = f32::from(JUMP_DIRECT_BORDER);
    let mut labels: Vec<(usize, JumpLabelLayout)> = (overlay.labels.into_iter())
        .filter_map(|label| {
            let rect = block_rect(layout, text, label.offset)?;
            let beside = rect.x + rect.width <= visible.x || rect.x >= visible.x + visible.width;
            (!beside).then(|| {
                let rect = match label.state {
                    JumpLabelState::Direct => Rect {
                        x: rect.x - ring,
                        y: rect.y - ring,
                        width: rect.width + 2. * ring,
                        height: rect.height + 2. * ring,
                    },
                    _ => rect,
                };
                let layout = JumpLabelLayout {
                    #[cfg(test)]
                    offset: label.offset,
                    rect,
                    text: label.text.into(),
                    state: label.state,
                };
                (label.offset, layout)
            })
        })
        .collect();
    if labels.len() > MAX_MATCHES {
        // Stable: text order within each kind.
        labels.sort_by_key(|(_, label)| label.state == JumpLabelState::Concealed);
        labels.truncate(MAX_MATCHES);
        labels.sort_by_key(|&(offset, _)| offset);
    }
    let typed = vim.jump_query().is_some_and(|query| !query.is_empty());
    Some(JumpLayout {
        backdrop: typed.then(|| layout.frame.text_bounds()),
        labels: labels.into_iter().map(|(_, label)| label).collect(),
    })
}

/// One thing a pending jump painted, in window coordinates, for the tests.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JumpPaint {
    Backdrop {
        bounds: Bounds<Pixels>,
        color: Hsla,
    },
    /// A label's box and its glyph's colour and weight, if it has one.
    Label {
        bounds: Bounds<Pixels>,
        fill: Hsla,
        glyph: Option<(Hsla, FontWeight)>,
    },
    /// A direct target's marker: a border around its char's block, no fill.
    Direct {
        bounds: Bounds<Pixels>,
        color: Hsla,
        width: Pixels,
    },
    /// A piece of a direct target's border, as painted: the border less
    /// what overlaps another target's char or a label.
    DirectEdge {
        bounds: Bounds<Pixels>,
        color: Hsla,
    },
    /// A block caret, painted over the backdrop.
    BlockCaret {
        bounds: Bounds<Pixels>,
    },
}

#[cfg(test)]
thread_local! {
    /// What the last frame painted on this thread for a pending jump, in
    /// order.
    pub(crate) static JUMP_PAINTS: std::cell::RefCell<Vec<JumpPaint>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The [`JUMP_DIRECT_BORDER`] border just inside `ring`, a direct target's
/// laid-out rect, as rects to fill: less every other direct target's char
/// block and every label's box, so where targets sit side by side (or a row
/// apart) no border covers another's char or label, and their borders
/// merge.
fn direct_ring(
    ring: Bounds<Pixels>,
    labels: &[(Bounds<Pixels>, SharedString, JumpLabelState)],
) -> Vec<Bounds<Pixels>> {
    let w = JUMP_DIRECT_BORDER;
    let (x0, y0) = (ring.origin.x, ring.origin.y);
    let (x1, y1) = (ring.right(), ring.bottom());
    let rect = |x0: Pixels, y0: Pixels, x1: Pixels, y1: Pixels| {
        Bounds::from_corners(point(x0, y0), point(x1, y1))
    };
    let mut edges = vec![
        rect(x0, y0, x1, y0 + w),
        rect(x0, y1 - w, x1, y1),
        rect(x0, y0 + w, x0 + w, y1 - w),
        rect(x1 - w, y0 + w, x1, y1 - w),
    ];
    let others = (labels.iter()).filter(|(other, ..)| *other != ring);
    for (other, _, state) in others {
        let other = match state {
            JumpLabelState::Direct => rect(
                other.origin.x + w,
                other.origin.y + w,
                other.right() - w,
                other.bottom() - w,
            ),
            _ => *other,
        };
        edges = (edges.into_iter())
            .flat_map(|edge| subtract(edge, other))
            .collect();
    }
    edges
}

/// What of `a` is outside `b`: up to four rects.
fn subtract(a: Bounds<Pixels>, b: Bounds<Pixels>) -> Vec<Bounds<Pixels>> {
    if !a.intersects(&b) {
        return vec![a];
    }
    let (top, bottom) = (b.origin.y.max(a.origin.y), b.bottom().min(a.bottom()));
    let pieces = [
        // Above, below, then beside within b's rows.
        (a.origin.x, a.origin.y, a.right(), top),
        (a.origin.x, bottom, a.right(), a.bottom()),
        (a.origin.x, top, b.origin.x.min(a.right()), bottom),
        (b.right().max(a.origin.x), top, a.right(), bottom),
    ];
    (pieces.into_iter())
        .filter(|&(x0, y0, x1, y1)| x1 > x0 && y1 > y0)
        .map(|(x0, y0, x1, y1)| Bounds::from_corners(point(x0, y0), point(x1, y1)))
        .collect()
}

/// A pending jump over the text: the backdrop, if any, then each label, an
/// opaque box with its glyph in bold in the theme's `jump_label_foreground`.
/// A live label's box is `jump_label`; a preview's is `jump_label_preview`,
/// and so is a concealed one's, which has no glyph. A direct target, the one
/// its `{c2}` jumps straight to, is no label but a [`JUMP_DIRECT_BORDER`]
/// border of `jump_label` just outside its char's block, so nothing covers
/// the char, cut back where it meets another target ([`direct_ring`]).
fn paint_jump(
    backdrop: Option<Bounds<Pixels>>,
    labels: &[(Bounds<Pixels>, SharedString, JumpLabelState)],
    theme: &EditorTheme,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(backdrop) = backdrop {
        let color = theme.background.opacity(JUMP_BACKDROP_ALPHA);
        window.paint_quad(fill(backdrop, color));
        #[cfg(test)]
        JUMP_PAINTS.with(|paints| {
            paints.borrow_mut().push(JumpPaint::Backdrop {
                bounds: backdrop,
                color,
            })
        });
    }
    let font = Font {
        weight: FontWeight::BOLD,
        ..gpui::font(theme.font_family.clone())
    };
    for (rect, label, state) in labels {
        if *state == JumpLabelState::Direct {
            let color = theme.jump_label;
            #[cfg(test)]
            JUMP_PAINTS.with(|paints| {
                paints.borrow_mut().push(JumpPaint::Direct {
                    bounds: *rect,
                    color,
                    width: JUMP_DIRECT_BORDER,
                })
            });
            for edge in direct_ring(*rect, labels) {
                window.paint_quad(fill(edge, color));
                #[cfg(test)]
                JUMP_PAINTS.with(|paints| {
                    paints.borrow_mut().push(JumpPaint::DirectEdge {
                        bounds: edge,
                        color,
                    })
                });
            }
            continue;
        }
        let fill_color = match state {
            JumpLabelState::Live => theme.jump_label,
            JumpLabelState::Preview | JumpLabelState::Concealed | JumpLabelState::Direct => {
                theme.jump_label_preview
            }
        };
        window.paint_quad(fill(*rect, fill_color));
        let glyph = (*state != JumpLabelState::Concealed)
            .then_some((theme.jump_label_foreground, font.weight));
        #[cfg(test)]
        JUMP_PAINTS.with(|paints| {
            paints.borrow_mut().push(JumpPaint::Label {
                bounds: *rect,
                fill: fill_color,
                glyph,
            })
        });
        let Some((color, _)) = glyph else {
            continue;
        };
        let run = TextRun {
            len: label.len(),
            font: font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window
            .text_system()
            .shape_line(label.clone(), theme.font_size, &[run], None);
        line.paint(
            rect.origin,
            rect.size.height,
            TextAlign::Center,
            Some(rect.size.width),
            window,
            cx,
        )
        .ok();
    }
}

/// The start of the grapheme `offset` is in, no further back than
/// `row_start` or [`MAX_CLUSTER_BYTES`].
fn grapheme_start(text: &Text, offset: usize, row_start: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation as _;
    let from = text.clip_offset(
        offset.saturating_sub(MAX_CLUSTER_BYTES).max(row_start),
        fe_buffer::Bias::Left,
    );
    let to = text.clip_offset(offset + 1, fe_buffer::Bias::Right);
    let slice = text.slice_to_string(from..to);
    slice
        .grapheme_indices(true)
        .map(|(ix, _)| from + ix)
        .take_while(|&start| start <= offset)
        .last()
        .unwrap_or(offset)
}

/// The grapheme boundary after `offset`, if it's before `line_end`. A
/// cluster longer than [`MAX_CLUSTER_BYTES`] ends there.
fn next_grapheme(text: &Text, offset: usize, line_end: usize) -> Option<usize> {
    use unicode_segmentation::UnicodeSegmentation as _;
    if offset >= line_end {
        return None;
    }
    let end = text.clip_offset(
        (offset + MAX_CLUSTER_BYTES).min(line_end),
        fe_buffer::Bias::Left,
    );
    let slice = text.slice_to_string(offset..end);
    let grapheme = slice.graphemes(true).next()?;
    Some(offset + grapheme.len())
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

#[cfg(test)]
mod jump_tests;

#[cfg(test)]
mod jump_style_tests;

#[cfg(test)]
mod jump_direct_tests;

#[cfg(test)]
mod jump_ring_tests;

#[cfg(test)]
mod jump_wrap_tests;

#[cfg(test)]
mod bracket_tests;
