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
    CharWidth, LongRows, MAX_CLUSTER_BYTES,
    layout::{
        self, Frame, FrameInput, FrameStyles, MeasuredLine, Rect, StyleRun, StyledSpan,
        TextMeasure, Vec2,
    },
};
use fe_editor::{
    decorations::DecorationKind,
    diagnostics::{DiagnosticTag, Severity},
};
use fe_syntax::{Syntax, SyntaxDriver};
use fe_vim::jump::JumpLabelState;
use gpui::{
    App, BorderStyle, Bounds, ContentMask, Context, CursorStyle, DecorationRun, DispatchPhase,
    Element, ElementId, ElementInputHandler, Entity, Font, FontId, FontWeight, Global,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId,
    LineLayout, Modifiers, ModifiersChangedEvent, MouseDownEvent, MouseEvent, MouseExitEvent,
    MouseMoveEvent, MouseUpEvent, PathBuilder, PinchEvent, Pixels, Point, ScrollWheelEvent,
    SharedString, Style, TextAlign, TextRun, TextSystem, UnderlineStyle, WeakEntity, Window,
    WindowId, WindowTextSystem, fill, outline, point, px, relative, size,
};

use crate::{
    EditorTheme, EditorView, FrameTiming,
    vim::{CaretShape, UNDERLINE_HEIGHT},
};

mod annotation_column;
mod decorations;
use decorations::{Decorations, Layer};
mod ghost_text;
mod host_blocks;
mod indent_guides;
#[cfg(test)]
pub(crate) use host_blocks::PaintedBlock;
mod proposal;

/// The frame's blocks: the lens rows and the host's ([`host_blocks`]), a
/// lens before a host's block above the same row, the proposals' hunks
/// ([`proposal`]) and the ghost text's lines after its first
/// ([`ghost_text`]).
fn block_map(
    view: &EditorView,
    lenses: &[crate::Lens],
    hunks: &[proposal::Hunk],
    ghost: Option<&ghost_text::Ghost>,
) -> fe_display::BlockMap {
    let ghost = ghost.and_then(|ghost| ghost_text::block(ghost, view));
    let host = host_blocks::lens_blocks(lenses, view).chain(host_blocks::blocks(view));
    fe_display::BlockMap::new((host.chain(proposal::blocks(hunks, view))).chain(ghost))
}

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
    /// The host's annotation column's rows ([`annotation_column`]).
    annotation_column: annotation_column::Placed,
    /// Over the column, if there is one, whose entries show popovers.
    annotation_hitbox: Option<Hitbox>,
    /// The search matches to paint ([`match_rects`]).
    matches: Vec<Rect>,
    /// The previewed search match to paint over them
    /// ([`current_match_rects`]).
    current_match: Vec<Rect>,
    /// The pattern ([`fe_vim::search::Pattern::regex_str`]) and text version
    /// of a frame whose highlighting ran out of time, until either changes.
    highlight_ran_out: Option<(String, fe_buffer::Version)>,
    /// The bracket pair at the caret to outline ([`bracket_rects`]).
    brackets: Vec<Rect>,
    /// A pending vim jump's backdrop and labels ([`jump_layout`]).
    pub(crate) jump: Option<JumpLayout>,
    /// What an input method was composing, and the buffer offset it was
    /// drawn at, as the frame's inlay.
    composition: Option<(usize, String)>,
    /// The decorations the model's providers have for the frame's rows.
    pub(crate) decorations: Decorations,
    /// The indent guides over the frame's rows ([`indent_guides`]).
    pub(crate) indent_guides: Vec<indent_guides::Placed>,
    /// The host's blocks in view ([`host_blocks`]).
    host_blocks: Vec<host_blocks::Placed>,
    /// The lens rows in view ([`host_blocks::lens_layout`]).
    lens_rows: Vec<host_blocks::PlacedLens>,
    /// The proposals' hunks in view ([`proposal`]).
    pub(crate) proposals: proposal::Layout,
    /// Their new texts, indexed, kept from frame to frame.
    pub(crate) proposal_texts: proposal::Texts,
    /// The ghost text's block rows in view ([`ghost_text`]).
    ghost: ghost_text::Placed,
    /// The hints the frame laid out, shaped small.
    inlay_hints: Vec<crate::inlay_hints::Shown>,
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
        let row = host_blocks::row_at(frame, at.y)?;
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

    /// The buffer row and display column (in chars) nearest a window
    /// position, read from the shaped row, so a position inside an expanded
    /// tab keeps its column. `None` where the row's text isn't all shaped
    /// from its start, as with a hint or inlay, a long row's slice or a soft
    /// wrapped continuation, or off the frame's rows.
    pub(crate) fn display_column_at(
        &self,
        text: &Text,
        position: Point<Pixels>,
    ) -> Option<(u32, usize)> {
        let at = self.to_frame(position);
        let frame = &self.frame;
        let row = host_blocks::row_at(frame, at.y)?;
        let whole = row.slice.is_none()
            && row.hints.is_empty()
            && row.expanded.inlay_range().is_none()
            && row.start == text.row_start(row.buffer_row);
        let line_x = at.x - frame.line_origin(row).x;
        if !whole || line_x < 0. {
            return None;
        }
        let index = row.line.x_to_boundary(line_x);
        Some((row.buffer_row, row.expanded.char_column(index)))
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
        let row = host_blocks::row_at(frame, at.y)?;
        if !row.last {
            return None;
        }
        let end = frame.view_x(row.x_for_offset(row.end, long_rows, glyph_widths));
        let space = glyph_widths.char_width(' ');
        (end <= at.x && at.x < end + space).then_some(row.end)
    }

    /// The display columns of an unwrapped row the frame shows: from its
    /// horizontal scroll to its right edge, in the width of a space.
    pub(crate) fn view_columns(&self) -> Option<Range<u32>> {
        let space = self.glyph_widths.clone().char_width(' ');
        let frame = &self.frame;
        visible_columns(frame.scroll.x, frame.width - frame.text_left, space)
    }

    /// How many columns, each a space wide, a window position is past the
    /// end of the buffer row [`Self::offset_at`] finds there, to the
    /// nearest; 0 short of the end. Off the frame's rows, or below the last,
    /// that's the row it clamps to, so a box selection's column goes as far
    /// there as on the row.
    pub(crate) fn columns_past_row_end(&mut self, text: &Text, position: Point<Pixels>) -> usize {
        let offset = self.offset_at(text, position);
        let at = self.to_frame(position);
        let Self {
            frame,
            long_rows,
            glyph_widths,
            ..
        } = self;
        let Some(row) = (host_blocks::row_at(frame, at.y))
            .or_else(|| frame.row_containing(offset))
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

    /// Whether the frame's search highlighting ran out of time.
    #[cfg(test)]
    pub(crate) fn highlight_ran_out(&self) -> bool {
        self.highlight_ran_out.is_some()
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
        .chain(frame.hints.iter_mut().map(|(_, rect)| rect))
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

    pub(crate) fn paint(
        &self,
        origin: Point<Pixels>,
        line_height: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) {
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

impl Line {
    /// The byte boundary of the shaped text closest to `x`.
    fn x_to_boundary(&self, x: f32) -> usize {
        MeasuredLine::index_for_x(self, x)
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
pub(crate) struct Shaper {
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
        let annotation_width = self.view.update(cx, |view, cx| {
            let rows = view.model.text().row_count();
            let annotation_width =
                view.annotation_width(&text_system, &font, font_id, font_size, em);
            // Less any room the view keeps for text after a line.
            let wrap = match view.gutter {
                true => {
                    layout::wrap_width_with_annotation_column(width, rows, em, annotation_width)
                }
                false => layout::wrap_width_without_gutter(width, em),
            } - view.wrap_reserve * em;
            view.model.fit_wrap_width(wrap.max(em));
            view.model.sync_display();
            view.sync_visual();
            view.sync_yank_flash();
            view.sync_inlay_hints();
            // Caught up with edits made through another view of the buffer:
            // count them.
            view.sync_find(cx);
            view.resolved_annotations();
            annotation_width
        });
        let mut proposal_texts = self.view.update(cx, |view, _| {
            let layout = view.layout.as_mut();
            layout.map(|layout| mem::take(&mut layout.proposal_texts))
        });
        let proposal_texts = proposal_texts.get_or_insert_default();
        let hunks = proposal::hunks(self.view.read(cx), proposal_texts);
        let ghost = ghost_text::shown(self.view.read(cx));
        let lenses = host_blocks::lenses(self.view.read(cx));
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
            // The scroll goes by screen rows, which the blocks before and
            // after the wrap place. A top inside a block keeps its place
            // above the display row below it.
            let screen_top = f32::from(view.scroll.y) / line_height_f32;
            let blocks = block_map(view, &lenses, &hunks, ghost.as_ref());
            let display_top = blocks.to_display_top(screen_top);
            let in_block = screen_top - blocks.to_screen_top(display_top);
            let (pending, top) = view.model.wrap_pending_at(display_top);
            if !view.autoscroll {
                let top =
                    block_map(view, &lenses, &hunks, ghost.as_ref()).to_screen_top(top) + in_block;
                view.scroll.y = px(top.max(0.) * line_height_f32);
            }
            pending
        });
        if pending {
            window.request_animation_frame();
        }

        let view = self.view.read(cx);
        let host_top = view.host_top_holds();
        let theme = &view.theme;
        let model = &view.model;
        let selections = model.selections();
        let input = FrameInput {
            display: model.display(),
            tab_size,
            selections: &selections,
            // A search preview's match is revealed as the caret would be.
            primary_head: view
                .preview_reveal()
                .unwrap_or_else(|| model.primary_selection().head()),
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
        // What an input method is composing, drawn at its caret, or else
        // the ghost text's first line.
        let composition = view.composition();
        let inlay = composition.map(|(offset, composition)| layout::Inlay {
            offset,
            // An inlay is one line.
            text: composition.text.replace(['\n', '\r'], " "),
            style: theme.foreground,
        });
        let inlay = inlay.or_else(|| Some(ghost_text::inlay(ghost.as_ref()?, view)));
        let blocks = block_map(view, &lenses, &hunks, ghost.as_ref());
        let gutter = view.gutter;
        let hints = crate::inlay_hints::inlays(view);
        let unnecessary = unnecessary_ranges(&model.diagnostics());
        let queried = decorations::Queried::default();
        // The colours of the frame's `Foreground` decorations, found once
        // the frame's rows are known.
        let foreground: std::cell::RefCell<Vec<StyledSpan<Hsla>>> = Default::default();
        let mut lay_out = |input: &FrameInput<Hsla>| match gutter {
            // fe-display lays hints out only beside no annotation column.
            true if annotation_width > 0. => layout::layout_frame_with_annotation_column(
                input,
                annotation_width,
                &blocks,
                inlay.as_ref(),
                &mut shaper,
                &mut long_rows,
                &mut glyph_widths,
                |range| styled_highlights(syntax, theme, &unnecessary, &foreground.borrow(), range),
                |rows| fold_headers(syntax, rows),
            ),
            true => layout::layout_frame_with_hints(
                input,
                &blocks,
                inlay.as_ref(),
                &hints,
                &mut shaper,
                &mut long_rows,
                &mut glyph_widths,
                |range| styled_highlights(syntax, theme, &unnecessary, &foreground.borrow(), range),
                |rows| fold_headers(syntax, rows),
            ),
            false => layout::layout_frame_without_gutter_with_hints(
                input,
                &blocks,
                inlay.as_ref(),
                &hints,
                &mut shaper,
                &mut long_rows,
                &mut glyph_widths,
                |range| styled_highlights(syntax, theme, &unnecessary, &foreground.borrow(), range),
            ),
        };
        // A frame laid out, and again with the colours of the `Foreground`
        // decorations over its rows where they change the spans. The
        // providers are asked once for the rows' range, as the painted
        // decorations then are.
        let mut settle = |input: &FrameInput<Hsla>| {
            let mut frame = lay_out(input);
            for _ in 0..2 {
                let (Some(first), Some(last)) = (frame.rows.first(), frame.rows.last()) else {
                    break;
                };
                let decorations = queried.get(view, first.start..last.end);
                let found = foreground_spans(&decorations, theme);
                if found == *foreground.borrow() {
                    break;
                }
                *foreground.borrow_mut() = found;
                frame = lay_out(input);
            }
            frame
        };
        let mut frame = settle(&input);
        // Next or previous hunk: its block comes into view too, though that
        // leaves the caret, at the hunk's start, out of it.
        let reveal = view.reveal_hunk.and_then(|id| {
            let top = frame.scroll.y / frame.line_height;
            let rows = frame.height / frame.line_height;
            proposal::reveal(&hunks, id, &frame.block_map, top, rows)
        });
        if let Some(top) = reveal {
            frame = settle(&FrameInput {
                scroll: Vec2 {
                    x: frame.scroll.x,
                    y: top * frame.line_height,
                },
                autoscroll: false,
                styles: input.styles.clone(),
                ..input
            });
        }
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
                let scrolled = settle(&FrameInput {
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
        // A text block's line, or a ghost text row, wider than the rows: the
        // view scrolls as far as it reaches too, where layout clamped short of it. Soft-wrapped
        // rows don't scroll, so then only the blocks' lines do.
        let mut host_x = frame.scroll.x;
        if kept_scroll.is_none() && !input.autoscroll {
            let host = host_blocks::widest(&frame, view, &mut shaper, &mut glyph_widths);
            let proposed = proposal::widest(
                &frame,
                &hunks,
                proposal_texts,
                view,
                &mut shaper,
                &mut glyph_widths,
            );
            let ghosted =
                ghost_text::widest(&frame, ghost.as_ref(), view, &mut shaper, &mut glyph_widths);
            let widest = host.max(proposed).max(ghosted);
            let text_width = f64::from(frame.width - frame.text_left);
            let reach = (widest - text_width + f64::from(em) * 4.).max(0.) as f32;
            let wanted = input.scroll.x.min(reach).max(0.);
            if model.display().wrap_width().is_some() {
                host_x = wanted;
                if wanted > 0. {
                    kept_scroll = Some(Vec2 {
                        x: wanted,
                        y: frame.scroll.y,
                    });
                }
            } else {
                let past = wanted - frame.scroll.x;
                if past > 0. {
                    shift_frame(&mut frame, past);
                }
                host_x = frame.scroll.x;
            }
        }
        let placed = host_blocks::layout(&frame, view, &mut shaper, &mut glyph_widths, host_x);
        let mut small = Shaper {
            text_system: shaper.text_system.clone(),
            font: shaper.font.clone(),
            font_size: font_size * host_blocks::LENS_SCALE,
        };
        let mut lens_rows =
            host_blocks::lens_layout(&frame, &lenses, view, &mut shaper, &mut small);
        let mut proposals = proposal::layout(
            &frame,
            &hunks,
            proposal_texts,
            view,
            &mut shaper,
            &mut glyph_widths,
            host_x,
        );
        let ghost = ghost_text::layout(
            &frame,
            ghost.as_ref(),
            view,
            &mut shaper,
            &mut glyph_widths,
            host_x,
        );
        let inlay_hints = crate::inlay_hints::layout(&frame, view, &mut small);
        let annotations = annotation_column::layout(&frame, view, &mut shaper, em);
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
        proposal::insert_hitboxes(&mut proposals, to_window, window);
        host_blocks::insert_lens_hitboxes(&mut lens_rows, to_window, window);

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
            view.reveal_hunk = None;
            view.preview_revealed();
            view.report_scroll_top(host_top, cx);
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
            annotation_column: annotations,
            annotation_hitbox: None,
            matches: Vec::new(),
            current_match: Vec::new(),
            highlight_ran_out: None,
            brackets: Vec::new(),
            jump: None,
            composition: composed,
            decorations: Decorations::default(),
            indent_guides: Vec::new(),
            host_blocks: placed,
            lens_rows,
            proposals,
            proposal_texts: mem::take(proposal_texts),
            ghost,
            inlay_hints,
            prepaint_started: started,
            prepaint_took: Duration::ZERO,
        };
        let (column, entries) =
            annotation_column::insert_hitboxes(&layout.annotation_column, &layout, view, window);
        layout.annotation_hitbox = column;
        layout.clickable.extend(entries);
        layout.matches = match_rects(&mut layout, view);
        layout.current_match = current_match_rects(&mut layout, view);
        layout.brackets = bracket_rects(&mut layout, view);
        layout.jump = jump_layout(&mut layout, view);
        layout.decorations = Decorations::layout(&mut layout, view, &queried, em, window);
        // Without a gutter, its marks aren't drawn, nor can be clicked.
        if !gutter {
            (layout.decorations.placed)
                .retain(|placed| !matches!(placed.kind, DecorationKind::Gutter(_)));
        }
        layout.indent_guides = indent_guides::layout(&mut layout, view, window);
        // A hand over each change mark, across the whole change lane.
        let lane = layout.bounds(layout.frame.change_lane);
        for placed in &layout.decorations.placed {
            if matches!(placed.kind, DecorationKind::Gutter(_)) {
                let hitbox = Bounds::new(
                    point(lane.origin.x, placed.bounds.origin.y),
                    size(lane.size.width, placed.bounds.size.height),
                );
                layout
                    .clickable
                    .push(window.insert_hitbox(hitbox, HitboxBehavior::Normal));
            }
        }
        // The completion menu goes by its word's start in this frame, which
        // the view doesn't hold until paint.
        let word = self.view.read(cx).completion_word_start();
        let word = word.and_then(|start| layout.caret_bounds(start));
        let (viewport, line_height) = (window.viewport_size(), px(layout.frame.line_height));
        self.view.update(cx, |view, cx| {
            view.place_completion(word, viewport, line_height, cx)
        });
        // Signature help goes by the caret, within the editor.
        let caret = self.view.read(cx).signature_help_caret();
        let caret = caret.and_then(|caret| layout.caret_bounds(caret));
        self.view
            .update(cx, |view, _| view.anchor_signature_help(caret, bounds));
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
                let mut carets = block_carets(&mut layout, text, heads.into_iter());
                // Composing in replace mode, the primary caret is the
                // composition's: under the grapheme after its caret.
                let composing = view
                    .composition()
                    .filter(|_| shape == CaretShape::Underline);
                if let Some((at, composition)) = composing
                    && let Some(primary) = block_rect(&mut layout, text, at)
                    && let Some(caret) = composition_block(
                        &mut layout,
                        text,
                        at,
                        &composition.text,
                        composition.selected.end,
                    )
                {
                    let primary = layout.bounds(primary);
                    carets.retain(|caret| caret.origin != primary.origin);
                    carets.push(caret);
                }
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
        // Composed text is underlined in the text colour; ghost text isn't.
        let composed = layout.composition.is_some();
        let composition_underline = layout.frame.inlay.filter(|_| composed).map(|inlay| {
            let thickness = COMPOSITION_UNDERLINE;
            layout.bounds(Rect {
                y: inlay.y + inlay.height - thickness,
                height: thickness,
                ..inlay
            })
        });
        let selections = selection_rects(&mut layout, view);
        let matches = mem::take(&mut layout.matches);
        let current_match = mem::take(&mut layout.current_match);
        let brackets = mem::take(&mut layout.brackets);
        let (squiggles, mut marks, strikes) = {
            let diagnostics = view.model.diagnostics();
            diagnostic_marks(&mut layout, view.model.text(), &diagnostics, &theme)
        };
        // The marks go in the gutter.
        if !view.gutter {
            marks.clear();
        }
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
        let mut painted_current_match = Vec::new();
        #[cfg(test)]
        let mut painted_chevrons = Vec::new();
        #[cfg(test)]
        let mut painted_brackets = Vec::new();
        #[cfg(test)]
        let mut painted_blocks = Vec::new();
        #[cfg(test)]
        let mut painted_annotations = Vec::new();
        let mut proposal_buttons = Vec::new();
        let mut lens_rows = Vec::new();
        #[cfg(test)]
        let mut painted_proposals = Vec::new();
        #[cfg(test)]
        let mut painted_ghost = None;
        #[cfg(test)]
        let mut painted_inlay_hints = Vec::new();

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
            proposal::paint_bands(&layout.proposals, &layout, window);
            let text_bounds = layout.bounds(frame.text_bounds());
            window.with_content_mask(
                Some(ContentMask {
                    bounds: text_bounds,
                }),
                |window| {
                    host_blocks::paint(
                        &layout.host_blocks,
                        &layout,
                        &theme,
                        window,
                        cx,
                        #[cfg(test)]
                        &mut painted_blocks,
                    );
                    host_blocks::paint_lenses(
                        &layout.lens_rows,
                        &layout,
                        window,
                        cx,
                        &mut lens_rows,
                    );
                    proposal::paint(
                        &layout.proposals,
                        &layout,
                        window,
                        cx,
                        &mut proposal_buttons,
                        #[cfg(test)]
                        &mut painted_proposals,
                    );
                    layout.decorations.paint(Layer::Backgrounds, window, cx);
                    indent_guides::paint(&layout.indent_guides, window);
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
                    // As the find bar's current match shows, selected.
                    for &found in &current_match {
                        let found = layout.bounds(found);
                        window.paint_quad(fill(found, theme.selection));
                        #[cfg(test)]
                        painted_current_match.push(found);
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
                    ghost_text::paint(
                        &layout.ghost,
                        &layout,
                        window,
                        cx,
                        #[cfg(test)]
                        &mut painted_ghost,
                    );
                    crate::inlay_hints::paint(
                        &layout.inlay_hints,
                        &layout,
                        theme.gutter_foreground,
                        window,
                        cx,
                        #[cfg(test)]
                        &mut painted_inlay_hints,
                    );
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
                    for &(strike, color) in &strikes {
                        window.paint_quad(fill(strike, color));
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
            annotation_column::paint(
                &layout.annotation_column,
                &layout,
                window,
                cx,
                #[cfg(test)]
                &mut painted_annotations,
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
            let column = layout.annotation_hitbox.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    view.update(cx, |view, cx| {
                        let (position, modifiers) = (event.position, &event.modifiers);
                        pointer_moved(
                            view,
                            position,
                            modifiers,
                            &text,
                            column.as_ref(),
                            window,
                            cx,
                        );
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
            view.proposal_buttons = proposal_buttons;
            view.lens_rows = lens_rows;
            #[cfg(test)]
            {
                view.painted_carets = painted;
                view.painted_selections = painted_selections;
                view.painted_matches = painted_matches;
                view.painted_current_match = painted_current_match;
                view.painted_chevrons = painted_chevrons;
                view.painted_brackets = painted_brackets;
                view.painted_blocks = painted_blocks;
                view.painted_annotations = painted_annotations;
                view.painted_proposals = painted_proposals;
                view.painted_ghost = painted_ghost;
                view.painted_inlay_hints = painted_inlay_hints;
                let underlines = underlines.iter().map(|&(bounds, _)| bounds).collect();
                view.painted_hover = (underlines, hover.is_some());
                view.painted_squiggles = squiggles;
                view.painted_strikes = strikes.iter().map(|&(bounds, _)| bounds).collect();
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

/// The pointer moved to `position`, with `modifiers` held: finds the hover
/// range and the popover for what's there, over `text` or the annotation
/// `column`. Over the popover itself, there's no hover range, and the
/// popover stays.
fn pointer_moved(
    view: &mut EditorView,
    position: Point<Pixels>,
    modifiers: &Modifiers,
    text: &Hitbox,
    column: Option<&Hitbox>,
    window: &mut Window,
    cx: &mut Context<EditorView>,
) {
    if view.pointer_on_popover(position) {
        view.hover_at(None, modifiers, window, cx);
        view.keep_popover();
        return;
    }
    let over = text.is_hovered_at(position, window).then_some(position);
    let over_column = column
        .filter(|column| column.is_hovered_at(position, window))
        .map(|_| position);
    view.hover_at(over, modifiers, window, cx);
    // An annotation's popover, or else the text's.
    if !view.hover_annotation(over_column, window, cx) {
        view.hover_diagnostics(over, window, cx);
    }
}

/// [`pointer_moved`] for the still pointer, if it's in the window, over the
/// last frame: once the popover it left has hidden, what's under it starts
/// its own wait.
pub(crate) fn pointer_still(
    view: &mut EditorView,
    window: &mut Window,
    cx: &mut Context<EditorView>,
) {
    let id = window.window_handle().window_id();
    let outside = cx
        .try_global::<PointerOutside>()
        .is_some_and(|outside| outside.0.contains(&id));
    let Some(layout) = view.layout.as_ref().filter(|_| !outside) else {
        return;
    };
    let (text, column) = (layout.text.clone(), layout.annotation_hitbox.clone());
    let (position, modifiers) = (window.mouse_position(), window.modifiers());
    pointer_moved(
        view,
        position,
        &modifiers,
        &text,
        column.as_ref(),
        window,
        cx,
    );
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
/// severe last, so on top; and, at the gutter's left (right of any
/// annotation column), a dot on the first
/// display row of each buffer row one covers, in the colour of the most
/// severe covering it. An empty range is underlined under the char after
/// it, or a space's width at a row's end.
fn diagnostic_marks(
    layout: &mut FrameLayout,
    text: &Text,
    diagnostics: &[fe_editor::diagnostics::DiagnosticEntry<'_>],
    theme: &EditorTheme,
) -> (Vec<Squiggle>, Vec<Mark>, Vec<Strike>) {
    let FrameLayout {
        frame,
        long_rows,
        glyph_widths,
        ..
    } = layout;
    let (Some(first), Some(last)) = (frame.rows.first(), frame.rows.last()) else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let shown = first.start..last.end;
    let mut pieces = Vec::new();
    let mut strikes = Vec::new();
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
            // Faded instead, for a hint.
            if !(entry.severity == Severity::Hint && is_unnecessary(entry)) {
                pieces.push((entry.severity, rect));
            }
            if entry.tags.contains(&DiagnosticTag::Deprecated) && start < end {
                let color = (row.line)
                    .color_at(row.expanded.to_display(start - row.shown().start))
                    .unwrap_or(theme.foreground);
                let middle = Rect {
                    y: baseline - ascent / 2.,
                    height: 1.,
                    ..rect
                };
                strikes.push((middle, color));
            }
        }
    }
    pieces.sort_by_key(|(severity, _)| *severity);
    let side = (frame.line_height * 0.35).round();
    let marks: Vec<(u32, Rect, Hsla)> = (frame.rows.iter())
        .filter(|row| row.start == text.row_start(row.buffer_row))
        .filter_map(|row| {
            let severity = rows.get(&row.buffer_row)?;
            let rect = Rect {
                x: frame.annotation_column.width + side / 2.,
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
    let strikes = (strikes.into_iter())
        .map(|(rect, color)| (layout.bounds(rect), color))
        .collect();
    (squiggles, marks, strikes)
}

/// A strike through deprecated text: its box in window coordinates and the
/// text's colour.
type Strike = (Bounds<Pixels>, Hsla);

/// How much of its alpha text covered by an Unnecessary diagnostic keeps,
/// as in VS Code's `editorUnnecessaryCode.opacity`.
const UNNECESSARY_OPACITY: f32 = 0.67;

fn is_unnecessary(entry: &fe_editor::diagnostics::DiagnosticEntry<'_>) -> bool {
    entry.tags.contains(&DiagnosticTag::Unnecessary)
}

/// The ranges Unnecessary diagnostics cover, sorted and merged so
/// overlapping ones fade once.
fn unnecessary_ranges(
    diagnostics: &[fe_editor::diagnostics::DiagnosticEntry<'_>],
) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = (diagnostics.iter())
        .filter(|entry| is_unnecessary(entry) && !entry.range.is_empty())
        .map(|entry| entry.range.clone())
        .collect();
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

/// `spans` with the parts inside `fade` (sorted, merged) at
/// [`UNNECESSARY_OPACITY`] of their alpha, and the parts of `fade` within
/// `range` that they leave uncovered added in `text`, faded.
fn fade_spans(
    spans: Vec<StyledSpan<Hsla>>,
    fade: &[Range<usize>],
    range: &Range<usize>,
    text: Hsla,
) -> Vec<StyledSpan<Hsla>> {
    let faded = |color: Hsla| Hsla {
        a: color.a * UNNECESSARY_OPACITY,
        ..color
    };
    let mut out = Vec::new();
    let mut covered = Vec::new();
    for span in spans {
        let mut at = span.range.start;
        for inside in fade {
            let (start, end) = (inside.start.max(at), inside.end.min(span.range.end));
            if start >= end {
                continue;
            }
            if at < start {
                out.push(StyledSpan {
                    range: at..start,
                    style: span.style,
                });
            }
            out.push(StyledSpan {
                range: start..end,
                style: faded(span.style),
            });
            at = end;
        }
        if at < span.range.end {
            out.push(StyledSpan {
                range: at..span.range.end,
                style: span.style,
            });
        }
        covered.push(span.range);
    }
    // What of the faded ranges no span covers.
    covered.sort_by_key(|range| range.start);
    for inside in fade {
        let mut at = inside.start.max(range.start);
        let end = inside.end.min(range.end);
        for span in &covered {
            if span.end <= at || span.start >= end {
                continue;
            }
            if at < span.start {
                out.push(StyledSpan {
                    range: at..span.start,
                    style: faded(text),
                });
            }
            at = at.max(span.end);
        }
        if at < end {
            out.push(StyledSpan {
                range: at..end,
                style: faded(text),
            });
        }
    }
    out.sort_by_key(|span| span.range.start);
    out
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
/// Ghost text's inlay at `head` is left out: the block covers the buffer's
/// grapheme after it.
fn block_rect(layout: &mut FrameLayout, text: &Text, head: usize) -> Option<Rect> {
    let FrameLayout {
        frame,
        long_rows,
        glyph_widths,
        ghost,
        ..
    } = layout;
    let row = frame.row_containing(head)?;
    let bar = frame.caret_bounds(head, long_rows, glyph_widths)?;
    let line = text.offset_to_point(head).row;
    let line_end = text.row_start(line) + text.row_len(line) as usize;
    // Measured on the head's row: a wrap point's caret is on the next.
    let inlay = match ghost.inlay_offset() == Some(head) {
        true => row.inlay_width,
        false => 0.,
    };
    let end = next_grapheme(text, head, row.end.min(line_end))
        .map(|next| frame.view_x(row.x_for_offset(next, long_rows, glyph_widths) - inlay))
        .filter(|&end| end > bar.x);
    let width = end.map_or_else(|| glyph_widths.char_width(' '), |end| end - bar.x);
    Some(Rect { width, ..bar })
}

/// The block over the grapheme after the caret of `composing`, drawn at
/// buffer offset `at` with its caret at byte `caret`, if this frame laid it
/// out: the composition's own next grapheme, or past its end the buffer's
/// after it ([`block_rect`]), one space wide at a line's end.
fn composition_block(
    layout: &mut FrameLayout,
    text: &Text,
    at: usize,
    composing: &str,
    caret: usize,
) -> Option<Bounds<Pixels>> {
    use unicode_segmentation::UnicodeSegmentation as _;
    let inlay = layout.frame.inlay?;
    let start = layout.inlay_bounds(caret..caret)?;
    let next =
        (composing.get(caret..)?.graphemes(true).next()).map(|grapheme| caret + grapheme.len());
    let width = match next {
        Some(next) => layout.inlay_bounds(caret..next)?.size.width,
        None => {
            // The block at `at` runs from before the composition to the end
            // of the buffer's grapheme after it.
            let block = block_rect(layout, text, at)?;
            let after = block.x + block.width - (inlay.x + inlay.width);
            match after > 0. {
                true => px(after),
                false => px(layout.glyph_widths.char_width(' ')),
            }
        }
    };
    Some(Bounds::new(start.origin, size(width, start.size.height)))
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
    /// Overrides the highlight time limit on this thread.
    pub(crate) static TEST_LIMIT: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
    /// How many matches the last [`match_rects`] on this thread looked at.
    pub(crate) static CANDIDATES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many match ranges the last [`match_rects`] on this thread kept:
    /// those reaching the displayed part of their row.
    pub(crate) static FOUND: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The rects of the search matches in the frame's displayed rows, at most
/// [`MAX_MATCHES`] of them, the first in view, of
/// [`EditorView::match_pattern`]: a vim search line's preview, else the
/// find pattern ([`EditorView::set_find`]) while one is set, else vim's
/// last search, none with vim off or after `:noh`. Folded rows aren't searched, and for vim's matches
/// off the displayed part of a wrapped or scrolled line don't use the
/// budget. A zero-width match is one space wide.
///
/// Each displayed buffer row's matches are enumerated once, from the row's
/// start, keeping those that reach what's displayed of it: from the text
/// area's left edge on its first displayed row to the right edge on its
/// last. The find pattern's [`MAX_MATCHES`] go to those, for the whole
/// frame, so matches scrolled off don't use them.
///
/// The frame has one [`fe_vim::search::time_limit`] for all its rows. Once
/// it runs out the frame paints what it has and searches no more rows, and
/// the layout records the pattern and text version
/// ([`FrameLayout::highlight_ran_out`]): later frames search nothing and
/// paint no matches until an edit or a different pattern, so each key
/// doesn't spend the limit again.
fn match_rects(layout: &mut FrameLayout, view: &EditorView) -> Vec<Rect> {
    // How many more ranges the frame may find: bounded for the find
    // pattern only.
    let previewed = view.previewed_pattern();
    let find = view.find_pattern().filter(|_| previewed.is_none());
    let clip = find.is_some();
    let (pattern, mut budget) = match find {
        Some(pattern) => (Some(pattern), MAX_MATCHES),
        None => (view.match_pattern(), usize::MAX),
    };
    let Some(pattern) = pattern else {
        return Vec::new();
    };
    #[cfg(test)]
    CANDIDATES.with(|n| n.set(0));
    #[cfg(test)]
    FOUND.with(|n| n.set(0));
    let latched = view
        .layout
        .as_ref()
        .and_then(|last| last.highlight_ran_out.as_ref());
    if latched
        .is_some_and(|(key, version)| key == pattern.regex_str() && *version == layout.version)
    {
        layout.highlight_ran_out = latched.cloned();
        return Vec::new();
    }
    let FrameLayout {
        frame,
        long_rows,
        glyph_widths,
        ..
    } = layout;
    let text = view.model.text();
    let space = glyph_widths.char_width(' ');
    let visible = frame.text_bounds();
    let version = layout.version;
    let mut latch = None;
    let mut rects = Vec::new();
    let limit = fe_vim::search::time_limit();
    #[cfg(test)]
    let limit = TEST_LIMIT.with(|l| l.get()).unwrap_or(limit);
    // A preview's, which changes as the line is typed, as quickly as its
    // match is found.
    let limit = match previewed {
        Some(_) => limit.min(fe_vim::SEARCH_PREVIEW_LIMIT),
        None => limit,
    };
    let mut deadline = fe_vim::search::Deadline::after(limit);
    // A buffer row's matches that reach the text area on its displayed rows,
    // kept for its other wrapped rows: found once a frame, searched from the
    // row's start for its context but only as far as the last displayed part.
    let mut whole: Option<(u32, Vec<Range<usize>>)> = None;
    let edge = space;
    let (left_x, right_x) = (visible.x - edge, visible.x + visible.width + edge);
    'rows: for (ix, row) in frame.rows.iter().enumerate() {
        if whole.as_ref().is_none_or(|(at, _)| *at != row.buffer_row) {
            if deadline.ran_out() {
                break;
            }
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
            let all = fe_vim::search::matches_reaching_until(
                text,
                row.buffer_row,
                window,
                pattern,
                budget,
                &mut deadline,
            );
            // The clock is only read between regex calls, so a last call
            // that overran the limit is recorded here.
            deadline.expired();
            if deadline.ran_out() {
                latch = Some((pattern.regex_str().to_string(), version));
            }
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
                break 'rows;
            }
        }
    }
    layout.highlight_ran_out = latch;
    rects
}

/// The rects of the match a vim search line's preview shows
/// ([`EditorView::search_preview`]) on the frame's displayed rows: one per
/// row it covers, an empty match one space wide.
fn current_match_rects(layout: &mut FrameLayout, view: &EditorView) -> Vec<Rect> {
    let Some(range) = view.search_preview() else {
        return Vec::new();
    };
    let FrameLayout {
        frame,
        long_rows,
        glyph_widths,
        ..
    } = layout;
    let space = glyph_widths.char_width(' ');
    frame
        .rows
        .iter()
        .filter_map(|row| {
            let (start, end) = (range.start.max(row.start), range.end.min(row.end));
            let shown = match range.is_empty() {
                true => row.contains(range.start),
                false => start < end,
            };
            if !shown {
                return None;
            }
            let left = frame.view_x(row.x_for_offset(start, long_rows, glyph_widths));
            let width = match range.is_empty() {
                true => space,
                false => frame.view_x(row.x_for_offset(end, long_rows, glyph_widths)) - left,
            };
            Some(Rect {
                x: left,
                y: frame.row_top(row.row),
                width,
                height: frame.line_height,
            })
        })
        .collect()
}

/// What a pending vim jump paints, in frame coordinates.
pub(crate) struct JumpLayout {
    /// Over the text area, dimming it once `{c1}` is typed; `None` before,
    /// so the text the user reads to pick a target isn't dimmed.
    pub(crate) backdrop: Option<Rect>,
    /// In text order.
    pub(crate) labels: Vec<JumpLabelLayout>,
}

/// One label of a [`JumpLayout`]: a box over its target's char.
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

/// While vim has a jump pending, its labels on the frame's rows
/// ([`fe_vim::VimState::jump_overlay`]), each a block over its target's
/// char, and a backdrop once a char is typed. The rows are those the jump's
/// session takes from this frame's viewport ([`fe_vim::jump::view_rows`]).
/// At most [`MAX_MATCHES`] labels, live ones before concealed ones, so a jump over more targets still shows the labels it waits for.
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
    let mut labels: Vec<(usize, JumpLabelLayout)> = (overlay.labels.into_iter())
        .filter_map(|label| {
            let rect = block_rect(layout, text, label.offset)?;
            let beside = rect.x + rect.width <= visible.x || rect.x >= visible.x + visible.width;
            (!beside).then(|| {
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

/// A pending jump over the text: the backdrop, if any, then each label, an
/// opaque box with its glyph in bold in the theme's `jump_label_foreground`.
/// A live label's box is `jump_label`; a concealed one's is
/// `jump_label_preview`, with no glyph.
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
        let fill_color = match state {
            JumpLabelState::Live => theme.jump_label,
            JumpLabelState::Concealed => theme.jump_label_preview,
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

/// The highlights in `range` that the theme colours, with bracket pairs in
/// their depth's colour over them when the theme colours pairs, and the
/// `Foreground` colours over those.
fn styled_highlights(
    syntax: Option<&Syntax>,
    theme: &EditorTheme,
    unnecessary: &[Range<usize>],
    foreground: &[StyledSpan<Hsla>],
    range: Range<usize>,
) -> Vec<StyledSpan<Hsla>> {
    let spans: Vec<_> = syntax
        .map(|syntax| syntax.highlights(range.clone()))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|span| {
            Some(StyledSpan {
                style: theme.syntax_color(span.highlight)?,
                range: span.range,
            })
        })
        .collect();
    let spans = match syntax {
        Some(syntax) if theme.bracket_pair_colors => {
            let brackets = fe_syntax::bracket_depths(syntax, range.clone())
                .into_iter()
                .map(|bracket| StyledSpan {
                    style: theme.bracket_color(bracket.depth),
                    range: bracket.range,
                });
            over(spans, brackets)
        }
        _ => spans,
    };
    let spans = over(spans, clipped(foreground, &range));
    if unnecessary.is_empty() {
        return spans;
    }
    fade_spans(spans, unnecessary, &range, theme.foreground)
}

/// The colours of the `Foreground` decorations among `decorations`, in paint
/// order, sorted and non-overlapping, later decorations winning where they
/// overlap. A name the theme doesn't colour is left out.
fn foreground_spans(
    decorations: &[fe_editor::decorations::Decoration],
    theme: &EditorTheme,
) -> Vec<StyledSpan<Hsla>> {
    let mut out: Vec<StyledSpan<Hsla>> = Vec::new();
    for decoration in decorations {
        let DecorationKind::Foreground(name) = &decoration.kind else {
            continue;
        };
        if decoration.range.is_empty() {
            continue;
        }
        let Some(style) = fe_syntax::Highlight::from_capture(name)
            .and_then(|highlight| theme.syntax_color(highlight))
        else {
            continue;
        };
        let span = StyledSpan {
            style,
            range: decoration.range.clone(),
        };
        match out.last() {
            Some(last) if last.range.end > span.range.start => out = over(out, [span]),
            _ => out.push(span),
        }
    }
    out
}

/// The part of the sorted, non-overlapping `spans` within `range`.
fn clipped(spans: &[StyledSpan<Hsla>], range: &Range<usize>) -> Vec<StyledSpan<Hsla>> {
    let first = spans.partition_point(|span| span.range.end <= range.start);
    spans[first..]
        .iter()
        .take_while(|span| span.range.start < range.end)
        .map(|span| StyledSpan {
            style: span.style,
            range: span.range.start.max(range.start)..span.range.end.min(range.end),
        })
        .collect()
}

/// `spans` with `top` laid over them, cutting them where they overlap. Both
/// are sorted and non-overlapping, and so is the result.
fn over(
    spans: Vec<StyledSpan<Hsla>>,
    top: impl IntoIterator<Item = StyledSpan<Hsla>>,
) -> Vec<StyledSpan<Hsla>> {
    let mut top = top.into_iter().peekable();
    let mut merged = Vec::with_capacity(spans.len());
    // Where the last span laid over them ends: a span it crosses is cut
    // short there, or dropped.
    let mut covered = 0;
    for span in spans {
        let mut start = span.range.start.max(covered);
        while let Some(above) = top.next_if(|above| above.range.start < span.range.end) {
            if above.range.start > start {
                merged.push(StyledSpan {
                    range: start..above.range.start,
                    style: span.style,
                });
            }
            covered = covered.max(above.range.end);
            start = start.max(above.range.end);
            merged.push(above);
        }
        if start < span.range.end {
            merged.push(StyledSpan {
                range: start..span.range.end,
                style: span.style,
            });
        }
    }
    merged.extend(top);
    merged
}

impl EditorView {
    /// The colour the last frame drew the text at buffer `offset` in: None
    /// before the first frame, or when `offset` wasn't drawn.
    pub fn drawn_color(&self, offset: usize) -> Option<Hsla> {
        let frame = &self.layout.as_ref()?.frame;
        let row = frame
            .rows
            .iter()
            .find(|row| row.shown().contains(&offset))?;
        let shown = row.shown();
        row.line
            .color_at(row.expanded.to_display(offset - shown.start))
    }
}

/// The columns, each `space` wide, of an unwrapped row that `text_width`
/// shows once scrolled `scroll_x` right; `None` if a space has no width.
fn visible_columns(scroll_x: f32, text_width: f32, space: f32) -> Option<Range<u32>> {
    if space <= 0. {
        return None;
    }
    let left = (scroll_x / space).round().max(0.) as u32;
    let width = (text_width / space).floor().max(1.) as u32;
    Some(left..left + width)
}

#[cfg(test)]
mod highlight_limit_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod jump_tests;

#[cfg(test)]
mod jump_style_tests;

#[cfg(test)]
#[cfg(test)]
#[cfg(test)]
mod jump_wrap_tests;

#[cfg(test)]
mod bracket_tests;
