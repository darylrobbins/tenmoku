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
use fe_syntax::{Syntax, SyntaxDriver};
use gpui::{
    App, Bounds, ContentMask, CursorStyle, DecorationRun, DispatchPhase, Element, ElementId,
    ElementInputHandler, Entity, Font, FontId, Global, GlobalElementId, Hitbox, HitboxBehavior,
    Hsla, InspectorElementId, IntoElement, LayoutId, LineLayout, MouseDownEvent, MouseEvent,
    MouseExitEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, PinchEvent, Pixels, Point,
    ScrollWheelEvent, Style, TextAlign, TextRun, TextSystem, WeakEntity, Window, WindowId,
    WindowTextSystem, fill, point, px, relative, size,
};

use crate::{EditorTheme, EditorView, FrameTiming};

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
    /// Over the fold column, whose hovering shows the chevrons of folds that
    /// are open.
    fold_column: Hitbox,
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
            fold_column,
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
        let accepts_text = view.accepts_text();
        let column_hovered = view.fold_column_hovered;
        let mut layout = layout.take().expect("prepaint ran");
        let block = view.block_cursor();
        let carets: Vec<_> = match focus.is_focused(window) {
            false => Vec::new(),
            true if block => {
                let text = view.model.text();
                let heads = view.model.selections().into_iter().map(|s| s.head());
                block_carets(&mut layout, text, heads)
            }
            true => layout
                .frame
                .cursors
                .iter()
                .map(|&cursor| layout.bounds(cursor))
                .collect(),
        };
        let frame = &layout.frame;
        let line_height = px(frame.line_height);
        #[cfg(test)]
        let mut painted = Vec::new();
        #[cfg(test)]
        let mut painted_chevrons = Vec::new();

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
                    for &selection in &frame.selections {
                        window.paint_quad(fill(layout.bounds(selection), theme.selection));
                    }
                    // Under the text, so the glyph stays readable.
                    if block {
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
                    for marker in &frame.fold_markers {
                        let rect = marker.rect;
                        let origin = layout.to_window(Vec2 {
                            x: rect.x,
                            y: rect.y,
                        });
                        marker.line.paint(origin, line_height, window, cx);
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
                }
            });
        }
        // Once this frame is the rendered one, check the column against the
        // still pointer, so a column moving under or away from it (a dock
        // opening, the gutter widening) shows or hides the chevrons.
        window.defer(cx, move |window, cx| {
            reconcile_fold_column(&view, &hitbox, window, cx);
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
            view.frame_timings.record(timing);
            #[cfg(test)]
            {
                view.painted_carets = painted;
                view.painted_chevrons = painted_chevrons;
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

/// Vim's normal-mode cursor at each of `heads` on screen: a block over the
/// grapheme after it, to the next grapheme boundary on its display row, or
/// one space wide at a line's end.
fn block_carets(
    layout: &mut FrameLayout,
    text: &Text,
    heads: impl Iterator<Item = usize>,
) -> Vec<Bounds<Pixels>> {
    let FrameLayout {
        frame,
        long_rows,
        glyph_widths,
        ..
    } = layout;
    let mut blocks = Vec::new();
    for head in heads {
        let (Some(row), Some(bar)) = (
            frame.row_containing(head),
            frame.caret_bounds(head, long_rows, glyph_widths),
        ) else {
            continue;
        };
        let line = text.offset_to_point(head).row;
        let line_end = text.row_start(line) + text.row_len(line) as usize;
        // Measured on the head's row: a wrap point's caret is on the next.
        let end = next_grapheme(text, head, row.end.min(line_end))
            .map(|next| frame.view_x(row.x_for_offset(next, long_rows, glyph_widths)))
            .filter(|&end| end > bar.x);
        let width = end.map_or_else(|| glyph_widths.char_width(' '), |end| end - bar.x);
        blocks.push(Rect { width, ..bar });
    }
    blocks.into_iter().map(|rect| layout.bounds(rect)).collect()
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
