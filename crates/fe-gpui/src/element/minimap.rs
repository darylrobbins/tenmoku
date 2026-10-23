//! The minimap beside the text ([`fe_display::minimap`]): its width comes
//! off the text area's right before soft wrap and the frame are laid out;
//! once the frame's scroll is known, its display rows are drawn as runs in
//! the colours the text gets, over `minimap_background`, with the slider
//! over the rows the editor shows.
//!
//! Rows are drawn from [`TILE_ROWS`]-row tile images at device pixels,
//! transparent where there are no runs, which scrolling only moves. The
//! rows' runs are cached ([`RowRunsCache`]) across frames and edits, each
//! with an id that changes only when its runs do, and a tile is rebuilt
//! only when its rows' ids, or its size or scale, changed. A frame builds
//! runs and tiles, nearest the slider first, for [`FRAME_BUDGET`] at most
//! (at least [`CHUNK_ROWS`] rows), and asks for another frame for the
//! rest; a tile not built yet leaves the background showing, and one built
//! before is drawn as it was until it's built again. Highlights are queried on the main thread, where
//! the syntax lives: the budget, not a background executor, keeps a jump
//! from paying for the whole minimap at once.
//!
//! Selections, find matches, diagnostics and git's changes are drawn beside
//! the tiles, never in them ([`marks`]).

use std::{
    collections::{HashMap, VecDeque},
    hash::{DefaultHasher, Hash, Hasher},
    ops::Range,
    sync::Arc,
    time::{Duration, Instant},
};

use fe_buffer::{Bias, Version};
use fe_display::{
    layout::{Rect, StyledSpan},
    minimap::{
        MinimapLayout, MinimapOptions, MinimapRun, MinimapShowSlider, MinimapSide, MinimapViewport,
        RowRunsCache,
    },
};
use fe_syntax::Syntax;
use gpui::{
    App, Bounds, ContentMask, Corners, CursorStyle, DispatchPhase, Hitbox, HitboxBehavior, Hsla,
    MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, Pixels, RenderImage, Rgba,
    ScrollWheelEvent, WeakEntity, Window, fill,
};

use super::{FrameLayout, PointerOutside, foreground_spans, styled_highlights};
use crate::{EditorTheme, EditorView};

#[path = "minimap_marks.rs"]
mod marks;

/// How opaque a run's colour is over the minimap's background, as VS Code's
/// blocks are faint beside the text.
const RUN_ALPHA: f32 = 0.6;

/// The most text, in bytes, between two rows the minimap draws (hidden by a
/// fold, or the rest of a long row) that their spans are queried across.
const CHUNK_GAP: usize = 16 * 1024;

/// Display rows a tile holds.
pub(crate) const TILE_ROWS: u32 = 64;

/// How long a frame spends building the minimap's runs and tiles, past its
/// first chunk of rows, before leaving the rest to the next frame: a jump to
/// rows none of which are cached costs a frame about this and a chunk, not
/// the whole minimap. Highlighting a chunk of Rust takes about 0.2 ms.
const FRAME_BUDGET: Duration = Duration::from_micros(150);

/// Rows built at a time, between looks at the budget.
const CHUNK_ROWS: usize = 16;

/// Tiles kept beyond those shown, for scrolling back: the rest are dropped
/// from the window's atlas.
pub(crate) const SPARE_TILES: usize = 8;

/// Rows of runs kept around the rows shown.
const CACHED_ROWS: usize = 4096;

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
    /// The tiles built, each with its rect: the rows of those not built yet
    /// show the background.
    tiles: Vec<(Rect, Arc<RenderImage>)>,
    marks: marks::Marks,
    /// Each run drawn on the rows shown, with its rect and colour.
    #[cfg(test)]
    runs: Vec<(Rect, Hsla)>,
    /// The tiles drawn from their rows' runs as cached, by index.
    #[cfg(test)]
    tile_indices: Vec<u32>,
    /// Every tile drawn, by index.
    #[cfg(test)]
    image_indices: Vec<u32>,
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
    entity: &WeakEntity<EditorView>,
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
        view.minimap_cache.borrow_mut().release(window);
        return None;
    }
    let drawn = view.minimap_cache.borrow_mut().update(
        &options,
        &geometry,
        view,
        syntax,
        unnecessary,
        window,
    );
    if !drawn.complete {
        window.request_animation_frame();
    }
    let mut cache = view.minimap_cache.borrow_mut();
    if drawn.sweep_left && !cache.sweep_asked {
        cache.sweep_asked = true;
        let entity = entity.clone();
        window.on_next_frame(move |window, cx| sweep_step(&entity, window, cx));
    }
    drop(cache);
    let sampling = Sampling::new(geometry.row_height, window.scale_factor(), rows);
    let max_column = options.max_column.max(1);
    let marks = marks::layout(
        &geometry,
        sampling,
        options.column_width(),
        max_column,
        view,
    );
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
        tiles: drawn.tiles,
        marks,
        #[cfg(test)]
        runs: drawn.runs,
        #[cfg(test)]
        tile_indices: drawn.indices,
        #[cfg(test)]
        image_indices: drawn.images,
        hitbox,
    })
}

/// What [`Cache::update`] drew for a frame.
struct Drawn {
    tiles: Vec<(Rect, Arc<RenderImage>)>,
    /// Whether every tile shown was drawn, from runs up to date.
    complete: bool,
    /// Whether tiles are left for the sweep's steps.
    sweep_left: bool,
    #[cfg(test)]
    runs: Vec<(Rect, Hsla)>,
    #[cfg(test)]
    indices: Vec<u32>,
    /// Every tile drawn, as it was or not.
    #[cfg(test)]
    images: Vec<u32>,
}

/// A tile's image and the rows it was drawn from.
struct Tile {
    /// Its rows' runs' ids, in order.
    ids: Vec<u64>,
    /// What it was drawn at: drawn at another size or scale, it's
    /// stretched until it's drawn again.
    key: TileKey,
    image: Arc<RenderImage>,
    /// The last frame that showed it.
    used: u64,
}

/// What the cached runs were built with, besides the rows' text, their
/// highlights and their decorations.
struct RunsKey {
    theme: EditorTheme,
    tab_size: usize,
    max_column: u32,
}

/// The state of the syntax the runs were highlighted with: the language,
/// the tree's version and whether it's been reparsed since, its window, and
/// the view's count of parses and language loads that landed.
#[derive(Clone, PartialEq)]
struct HighlightsKey {
    epoch: u64,
    syntax: Option<SyntaxState>,
}

/// A syntax's language, version, whether it needs a parse, its window and
/// whether it failed.
type SyntaxState = (usize, Version, bool, Option<Range<usize>>, bool);

impl HighlightsKey {
    fn new(epoch: u64, syntax: Option<&Syntax>) -> Self {
        Self {
            epoch,
            syntax: syntax.map(|syntax| {
                (
                    std::ptr::from_ref(syntax.language()).addr(),
                    syntax.version(),
                    syntax.needs_parse(),
                    syntax.window(),
                    syntax.failed(),
                )
            }),
        }
    }
}

/// What a tile's pixels are, besides its rows' runs.
#[derive(Clone, Copy, PartialEq)]
struct TileKey {
    scale_factor: f32,
    /// The minimap's width, a column's and a row's height, in logical
    /// pixels.
    width: f32,
    column_width: f32,
    row_height: f32,
    /// Every how many display rows one is drawn.
    stride: u32,
}

/// The display rows the tiles draw: all, or for rows less than a device
/// pixel tall, as `Fill` gives a long file, every `stride`th, so each row
/// drawn is a device pixel tall at least and what's drawn and kept is as
/// many rows as the minimap has pixels, not as the file has rows. A tile
/// holds [`TILE_ROWS`] rows drawn.
#[derive(Clone, Copy)]
struct Sampling {
    stride: u32,
    /// How many display rows there are.
    rows: u32,
}

impl Sampling {
    fn new(row_height: f32, scale_factor: f32, rows: u32) -> Self {
        let device = row_height * scale_factor;
        let stride = match device >= 1. || device <= 0. {
            true => 1,
            false => ((1. / device).ceil() as u32).max(1),
        };
        Self { stride, rows }
    }

    /// Display rows a tile spans.
    fn span(&self) -> u32 {
        TILE_ROWS.saturating_mul(self.stride)
    }

    /// The tiles over display rows `rows`.
    fn tiles(&self, rows: Range<u32>) -> Range<u32> {
        rows.start / self.span()..rows.end.div_ceil(self.span())
    }

    /// The display rows tile `index` draws.
    fn rows_of(&self, index: u32) -> Vec<u32> {
        let first = index.saturating_mul(self.span());
        (0..TILE_ROWS)
            .map(|n| first.saturating_add(n * self.stride))
            .take_while(|&row| row < self.rows)
            .collect()
    }
}

/// The minimap's rows' runs and its tiles, kept across frames.
pub(crate) struct Cache {
    rows: RowRunsCache<Hsla>,
    runs_key: Option<RunsKey>,
    highlights: Option<HighlightsKey>,
    tiles: HashMap<u32, Tile>,
    /// The sweep: the tiles shown at the last frame whose rows'
    /// decorations are still to be looked at since, nearest the slider
    /// first, and the rows they draw. Every frame begins it again; between
    /// frames it goes on in steps that draw nothing unless rows changed
    /// ([`sweep_step`]), so a frame is never drawn only for it, and any
    /// frame, whatever asked for it, sweeps again from the start.
    sweep: VecDeque<u32>,
    sweep_sampling: Option<Sampling>,
    /// Whether a sweep step is asked for at the next frame.
    sweep_asked: bool,
    /// Counts updates, for the tiles' last use.
    frame: u64,
    /// See [`FRAME_BUDGET`].
    pub(crate) budget: Duration,
    /// The tiles each update built, by index, for tests to take.
    #[cfg(test)]
    pub(crate) built: Vec<u32>,
    /// The images dropped from the atlas.
    #[cfg(test)]
    pub(crate) released: Vec<Arc<RenderImage>>,
    /// The tiles each update looked at the decorations of, by index.
    #[cfg(test)]
    pub(crate) looked: Vec<u32>,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            rows: RowRunsCache::new(CACHED_ROWS),
            runs_key: None,
            highlights: None,
            tiles: HashMap::new(),
            sweep: VecDeque::new(),
            sweep_sampling: None,
            sweep_asked: false,
            frame: 0,
            // Unoptimized builds take ten times as long or more, which
            // would leave a debug app's minimap, and most tests' and the
            // screenshots', unfinished for many frames; the tests of the
            // budget set it.
            budget: if cfg!(test) || cfg!(debug_assertions) {
                Duration::from_secs(1)
            } else {
                FRAME_BUDGET
            },
            #[cfg(test)]
            built: Vec::new(),
            #[cfg(test)]
            released: Vec::new(),
            #[cfg(test)]
            looked: Vec::new(),
        }
    }
}

/// The work [`Cache::update`] does, in order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    /// Tiles that can't be drawn, from the rows without runs.
    Build,
    /// Tiles drawn from runs that may be out of date, or at another size.
    Verify,
    /// The sweep's first tiles.
    Sweep,
}

impl Cache {
    /// How many tiles hold an image.
    #[cfg(test)]
    pub(crate) fn live_tiles(&self) -> usize {
        self.tiles.len()
    }

    /// How many rows' runs are cached.
    #[cfg(test)]
    pub(crate) fn cached_rows(&self) -> usize {
        self.rows.len()
    }

    /// The images of the tiles held.
    #[cfg(test)]
    pub(crate) fn images(&self) -> Vec<Arc<RenderImage>> {
        self.tiles.values().map(|tile| tile.image.clone()).collect()
    }

    /// Drops every tile from `window`'s atlas, and the rows' runs with
    /// the text they're of, for a minimap off or hidden.
    pub(crate) fn release(&mut self, window: &mut Window) {
        for (_, tile) in std::mem::take(&mut self.tiles) {
            self.drop_image(tile.image, window);
        }
        if self.runs_key.is_some() {
            self.rows = RowRunsCache::new(CACHED_ROWS);
            self.runs_key = None;
            self.highlights = None;
        }
        self.sweep.clear();
    }

    /// The tiles' images, given up for the view's release, which drops
    /// them from every window's atlas.
    pub(crate) fn take_images(&mut self) -> Vec<Arc<RenderImage>> {
        self.tiles.drain().map(|(_, tile)| tile.image).collect()
    }

    /// Brings the runs and tiles of the rows `geometry` shows up to date,
    /// within the budget, and returns the tiles to draw.
    fn update(
        &mut self,
        options: &MinimapOptions,
        geometry: &MinimapLayout,
        view: &EditorView,
        syntax: Option<&Syntax>,
        unnecessary: &[Range<usize>],
        window: &mut Window,
    ) -> Drawn {
        let started = Instant::now();
        self.frame += 1;
        let display = view.model.display();
        let (tab_size, max_column) = (view.model.tab_size(), options.max_column.max(1));
        let same_runs = self.runs_key.as_ref().is_some_and(|key| {
            key.theme == view.theme && key.tab_size == tab_size && key.max_column == max_column
        });
        if !same_runs {
            // Every run's colours or columns may change.
            self.rows.clear();
            self.runs_key = Some(RunsKey {
                theme: view.theme.clone(),
                tab_size,
                max_column,
            });
        }
        self.rows.sync(display);
        let highlights = HighlightsKey::new(view.highlight_epoch, syntax);
        if self.highlights.as_ref() != Some(&highlights) {
            // Any row's highlights may have changed: they're rebuilt and
            // compared, and their tiles drawn meanwhile.
            self.rows.invalidate(0..u32::MAX);
            self.highlights = Some(highlights);
        }
        let scale_factor = window.scale_factor();
        let sampling = Sampling::new(geometry.row_height, scale_factor, display.row_count());
        let key = TileKey {
            scale_factor,
            width: geometry.x.end - geometry.x.start,
            column_width: options.column_width(),
            row_height: geometry.row_height,
            stride: sampling.stride,
        };
        let shown = sampling.tiles(geometry.first_row..geometry.end_row);
        // Room for the rows drawn, as many as the minimap's pixels at most.
        let drawn_rows = shown.len() * TILE_ROWS as usize;
        self.rows.set_capacity(CACHED_ROWS.max(2 * drawn_rows));
        let builder = Builder {
            view,
            syntax,
            unnecessary,
            max_column,
        };

        // The tiles nearest the rows the editor shows first: those that
        // can't be drawn, from the rows without runs; then those drawn from
        // runs that may be out of date, or at another size; then the sweep
        // begins again, a look at decorations, which say nothing when they
        // change. Until then, a tile is drawn from its rows' runs as they
        // are. Rows are built a chunk at a time, at least one a frame, until
        // the budget runs out; the rest wait for the next frame, and the
        // sweep for the steps between frames.
        let slider = geometry.slider_top + geometry.slider_height / 2.;
        let centre = geometry.scroll_for_click(slider) / sampling.span();
        let mut order: Vec<u32> = shown.clone().collect();
        order.sort_by_key(|index| index.abs_diff(centre));
        let budget = self.budget;
        let mut worked = false;
        let mut out_of_time = |work: bool| {
            let out = worked && started.elapsed() >= budget;
            worked |= work && !out;
            out
        };
        self.sweep = order.iter().copied().collect();
        self.sweep_sampling = Some(sampling);
        'passes: for pass in [Pass::Build, Pass::Verify, Pass::Sweep] {
            if pass == Pass::Sweep {
                while let Some(&index) = self.sweep.front() {
                    if out_of_time(true) {
                        break 'passes;
                    }
                    builder.sweep(&sampling.rows_of(index), &mut self.rows);
                    self.sweep.pop_front();
                    #[cfg(test)]
                    self.looked.push(index);
                }
                break;
            }
            for &index in &order {
                let rows_of = sampling.rows_of(index);
                let drawable = self.drawable(index, &rows_of);
                let todo = match pass {
                    Pass::Build => !drawable,
                    _ => drawable && !self.current(index, &rows_of, key),
                };
                if !todo {
                    continue;
                }
                let stale: Vec<u32> = (rows_of.iter().copied())
                    .filter(|&row| match pass {
                        Pass::Build => self.rows.row(row).is_none(),
                        _ => !self.fresh(row),
                    })
                    .collect();
                for chunk in stale.chunks(CHUNK_ROWS) {
                    if out_of_time(true) {
                        break 'passes;
                    }
                    builder.build(chunk, &mut self.rows);
                }
                if !self.drawable(index, &rows_of) || !self.drawn_at(index, key) {
                    // A tile whose rows were just built is drawn with them.
                    if stale.is_empty() && out_of_time(true) {
                        break 'passes;
                    }
                    self.rebuild(index, &rows_of, key, sampling, window);
                }
            }
        }
        let mut drawn = Drawn {
            tiles: Vec::new(),
            complete: true,
            sweep_left: !self.sweep.is_empty(),
            #[cfg(test)]
            runs: Vec::new(),
            #[cfg(test)]
            indices: Vec::new(),
            #[cfg(test)]
            images: Vec::new(),
        };
        for index in shown.clone() {
            let rows_of = sampling.rows_of(index);
            // A tile drawn before is drawn as it was until it's drawn
            // again; one never drawn leaves the background showing.
            let Some(tile) = self.tiles.get_mut(&index) else {
                drawn.complete = false;
                continue;
            };
            tile.used = self.frame;
            let image = tile.image.clone();
            let drawable = self.drawable(index, &rows_of);
            drawn.complete &= drawable && self.current(index, &rows_of, key);
            let rect = Rect {
                x: geometry.x.start,
                y: geometry.row_y(index * sampling.span()),
                width: key.width,
                height: sampling.span() as f32 * key.row_height,
            };
            drawn.tiles.push((rect, image));
            #[cfg(test)]
            drawn.images.push(index);
            #[cfg(test)]
            if drawable {
                drawn.indices.push(index);
                let visible = geometry.first_row..geometry.end_row;
                for &row in rows_of.iter().filter(|row| visible.contains(row)) {
                    let runs = &self.rows.row(row).expect("drawable").runs;
                    drawn.runs.extend(runs.iter().map(|run| {
                        let rect = Rect {
                            x: geometry.x.start + run.columns.start as f32 * key.column_width,
                            y: geometry.row_y(row),
                            width: run.columns.len() as f32 * key.column_width,
                            height: sampling.stride as f32 * geometry.row_height,
                        };
                        (rect, run.color.opacity(RUN_ALPHA))
                    }));
                }
            }
        }
        self.evict(&shown, window);
        let first = shown.start.saturating_mul(sampling.span());
        let end = shown.end.saturating_mul(sampling.span()).min(sampling.rows);
        self.rows.retain_around(first..end);
        drawn
    }

    /// Looks at the decorations of the sweep's next tiles, within the
    /// budget, at least one; `unnecessary` is the code shown faded. Returns
    /// whether rows' decorations changed, now stale, and whether tiles are
    /// left. A sweep of rows the text no longer has stops: the frame the
    /// change draws begins another.
    fn sweep_step(&mut self, view: &EditorView, unnecessary: &[Range<usize>]) -> (bool, bool) {
        let (Some(sampling), Some(key)) = (self.sweep_sampling, &self.runs_key) else {
            return (false, false);
        };
        if !self.rows.is_synced(view.model.display()) {
            self.sweep.clear();
            return (false, false);
        }
        let builder = Builder {
            view,
            syntax: None,
            unnecessary,
            max_column: key.max_column,
        };
        let started = Instant::now();
        let mut changed = false;
        let mut worked = false;
        while let Some(&index) = self.sweep.front() {
            if worked && started.elapsed() >= self.budget {
                break;
            }
            worked = true;
            changed |= builder.sweep(&sampling.rows_of(index), &mut self.rows);
            self.sweep.pop_front();
            #[cfg(test)]
            self.looked.push(index);
        }
        (changed, !self.sweep.is_empty())
    }

    /// Whether display row `row`'s runs are cached and up to date.
    fn fresh(&self, row: u32) -> bool {
        self.rows.row(row).is_some_and(|cached| !cached.stale)
    }

    /// Whether tile `index`, of rows `rows`, is drawn at `key` from its
    /// rows' runs, up to date.
    fn current(&self, index: u32, rows: &[u32], key: TileKey) -> bool {
        self.drawn_at(index, key) && rows.iter().all(|&row| self.fresh(row))
    }

    /// Whether tile `index` was drawn at `key`.
    fn drawn_at(&self, index: u32, key: TileKey) -> bool {
        self.tiles.get(&index).is_some_and(|tile| tile.key == key)
    }

    /// Whether tile `index`, of rows `rows`, has an image of its rows' runs
    /// as cached.
    fn drawable(&self, index: u32, rows: &[u32]) -> bool {
        let Some(tile) = self.tiles.get(&index) else {
            return false;
        };
        tile.ids.len() == rows.len()
            && (rows.iter().zip(&tile.ids))
                .all(|(&row, &id)| self.rows.row(row).is_some_and(|cached| cached.id == id))
    }

    /// Draws tile `index` from the runs of its rows, `rows`, replacing its
    /// image.
    fn rebuild(
        &mut self,
        index: u32,
        rows: &[u32],
        key: TileKey,
        sampling: Sampling,
        window: &mut Window,
    ) {
        let image = rasterize(&self.rows, rows, key, sampling);
        let ids = (rows.iter())
            .map(|&row| self.rows.row(row).map_or(0, |cached| cached.id))
            .collect();
        let tile = Tile {
            ids,
            key,
            image,
            used: self.frame,
        };
        if let Some(old) = self.tiles.insert(index, tile) {
            self.drop_image(old.image, window);
        }
        #[cfg(test)]
        self.built.push(index);
    }

    /// Drops the tiles not `shown` beyond the [`SPARE_TILES`] last shown.
    fn evict(&mut self, shown: &Range<u32>, window: &mut Window) {
        let mut spare: Vec<(u64, u32)> = (self.tiles.iter())
            .filter(|(index, _)| !shown.contains(index))
            .map(|(&index, tile)| (tile.used, index))
            .collect();
        if spare.len() <= SPARE_TILES {
            return;
        }
        spare.sort_unstable_by(|a, b| b.cmp(a));
        for (_, index) in spare.split_off(SPARE_TILES) {
            if let Some(tile) = self.tiles.remove(&index) {
                self.drop_image(tile.image, window);
            }
        }
    }

    /// Drops `image` from `window`'s atlas.
    fn drop_image(&mut self, image: Arc<RenderImage>, window: &mut Window) {
        #[cfg(test)]
        self.released.push(image.clone());
        if let Err(err) = window.drop_image(image) {
            log::error!("dropping a minimap tile: {err}");
        }
    }
}

/// A step of `entity`'s minimap's sweep, between frames: draws nothing
/// unless rows' decorations changed, and asks for the next step while
/// tiles are left.
fn sweep_step(entity: &WeakEntity<EditorView>, window: &mut Window, cx: &mut App) {
    let again = entity.update(cx, |view, cx| {
        let unnecessary = super::unnecessary_ranges(&view.model.diagnostics());
        let mut cache = view.minimap_cache.borrow_mut();
        let (changed, left) = cache.sweep_step(view, &unnecessary);
        cache.sweep_asked = left;
        drop(cache);
        if changed {
            cx.notify();
        }
        left
    });
    if again.unwrap_or(false) {
        let entity = entity.clone();
        window.on_next_frame(move |window, cx| sweep_step(&entity, window, cx));
    }
}

/// A tile's image: rows `rows`' runs as cached, each `sampling.stride` rows
/// tall, at device pixels, on a transparent background.
fn rasterize(
    cache: &RowRunsCache<Hsla>,
    rows: &[u32],
    key: TileKey,
    sampling: Sampling,
) -> Arc<RenderImage> {
    let scale = key.scale_factor;
    let device = |logical: f32| (logical * scale).round().max(0.) as usize;
    let height_of = key.row_height * sampling.stride as f32;
    let width = device(key.width).max(1);
    let height = device(TILE_ROWS as f32 * height_of).max(1);
    let mut pixels = vec![0u8; width * height * 4];
    for (n, &row) in rows.iter().enumerate() {
        let Some(cached) = cache.row(row) else {
            continue;
        };
        let top = device(n as f32 * height_of).min(height);
        let bottom = device((n + 1) as f32 * height_of).min(height);
        for run in &cached.runs {
            let left = device(run.columns.start as f32 * key.column_width).min(width);
            let right = device(run.columns.end as f32 * key.column_width).min(width);
            let color = bgra(run.color.opacity(RUN_ALPHA));
            for y in top..bottom {
                let line = &mut pixels[(y * width + left) * 4..(y * width + right) * 4];
                for pixel in line.as_chunks_mut::<4>().0 {
                    *pixel = color;
                }
            }
        }
    }
    let buffer = image::RgbaImage::from_raw(width as u32, height as u32, pixels)
        .expect("sized to the image");
    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
}

/// `color`'s bytes as a [`RenderImage`] holds them: blue, green, red and
/// alpha, not premultiplied.
fn bgra(color: Hsla) -> [u8; 4] {
    let Rgba { r, g, b, a } = color.into();
    [b, g, r, a].map(|channel| (channel.clamp(0., 1.) * 255.).round() as u8)
}

/// Builds rows' runs, and looks at the decorations over rows, a chunk of
/// rows at a time.
struct Builder<'a> {
    view: &'a EditorView,
    syntax: Option<&'a Syntax>,
    unnecessary: &'a [Range<usize>],
    max_column: u32,
}

/// The decorations colouring some rows, and each row's stamp from them and
/// the unnecessary code over it; a row with neither has none.
struct Extras {
    foreground: Vec<StyledSpan<Hsla>>,
    stamps: HashMap<u32, u64>,
}

impl Extras {
    fn stamp(&self, row: u32) -> u64 {
        self.stamps.get(&row).copied().unwrap_or(0)
    }
}

impl Builder<'_> {
    /// Display rows `rows`' text, ascending, no further than their drawn
    /// columns can reach: a char is a column at least and 4 bytes at most.
    fn ranges(&self, rows: &[u32]) -> Vec<(u32, Range<usize>)> {
        let (display, text) = (self.view.model.display(), self.view.model.text());
        let reach = 4 * self.max_column as usize;
        (rows.iter())
            .map(|&row| {
                let range = display.row(row).range;
                let end = text.clip_offset(range.end.min(range.start + reach), Bias::Left);
                (row, range.start..end)
            })
            .collect()
    }

    /// The decorations over `rows`, from the text each row draws, queried
    /// in chunks of rows near one another (as [`Builder::build`] queries
    /// highlights), and the rows' stamps.
    fn extras(&self, rows: &[(u32, Range<usize>)]) -> Extras {
        let theme = &self.view.theme;
        let mut foreground = Vec::new();
        for chunk in rows.chunk_by(|(_, above), (_, below)| below.start <= above.end + CHUNK_GAP) {
            let range = chunk[0].1.start..chunk[chunk.len() - 1].1.end;
            let decorations = self.view.model.decorations(range.clone());
            // Clipped to the chunk: a span over a gap between two chunks
            // isn't counted twice, and the spans stay sorted and apart, so
            // a row's stamp doesn't depend on how its rows were chunked.
            foreground.extend(
                (foreground_spans(&decorations, theme).into_iter()).filter_map(|span| {
                    let clipped = span.range.start.max(range.start)..span.range.end.min(range.end);
                    (!clipped.is_empty()).then_some(StyledSpan {
                        range: clipped,
                        style: span.style,
                    })
                }),
            );
        }
        let mut stamps = HashMap::new();
        for (row, range) in rows {
            // A row with no text draws nothing whatever is over it, and how
            // the spans were clipped around it depends on how its rows were
            // chunked: it keeps no stamp.
            if range.is_empty() {
                continue;
            }
            let mut hasher: Option<DefaultHasher> = None;
            let over = |span: &Range<usize>| span.end > range.start && span.start < range.end;
            let first = foreground.partition_point(|span| span.range.end <= range.start);
            let colored = (foreground[first..].iter())
                .take_while(|span| span.range.start <= range.end)
                .filter(|span| over(&span.range))
                .map(|span| (span.range.clone(), Some(span.style)));
            let first = self
                .unnecessary
                .partition_point(|fade| fade.end <= range.start);
            let fades = (self.unnecessary[first..].iter())
                .take_while(|fade| fade.start <= range.end)
                .filter(|fade| over(fade))
                .map(|fade| (fade.clone(), None));
            for (span, color) in colored.chain(fades) {
                let hasher = hasher.get_or_insert_default();
                let start = span.start.max(range.start) - range.start;
                let end = span.end.min(range.end).max(range.start) - range.start;
                (start, end).hash(hasher);
                color
                    .map(|color| [color.h, color.s, color.l, color.a].map(f32::to_bits))
                    .hash(hasher);
            }
            if let Some(hasher) = hasher {
                stamps.insert(*row, hasher.finish() | 1);
            }
        }
        Extras { foreground, stamps }
    }

    /// Looks at the decorations over display rows `rows`, ascending, and
    /// marks the rows whose stamps changed stale; returns whether any did.
    fn sweep(&self, rows: &[u32], cache: &mut RowRunsCache<Hsla>) -> bool {
        let ranges = self.ranges(rows);
        let extras = self.extras(&ranges);
        let mut changed = false;
        for &row in rows {
            let cached = cache.row(row);
            if cached.is_some_and(|cached| !cached.stale && cached.stamp != extras.stamp(row)) {
                cache.invalidate(row..row + 1);
                changed = true;
            }
        }
        changed
    }

    /// Builds the runs of display rows `rows`, ascending, in the colours the
    /// text's spans give them, or the text colour, into `cache`.
    fn build(&self, rows: &[u32], cache: &mut RowRunsCache<Hsla>) {
        let view = self.view;
        let theme = &view.theme;
        let (display, text) = (view.model.display(), view.model.text());
        let rows = self.ranges(rows);
        let extras = self.extras(&rows);
        // Rows near one another take their spans in one go, which the
        // syntax caches as one range: what a fold hides or a long row's
        // rest is queried with them, unless that's far more than a row.
        for chunk in rows.chunk_by(|(_, above), (_, below)| below.start <= above.end + CHUNK_GAP) {
            let range = chunk[0].1.start..chunk[chunk.len() - 1].1.end;
            let spans = styled_highlights(
                self.syntax,
                theme,
                self.unnecessary,
                &extras.foreground,
                range,
            );
            let mut next = 0;
            for (row, range) in chunk {
                next += spans[next..].partition_point(|span| span.range.end <= range.start);
                let row_spans = row_spans(&spans[next..], range);
                let row_text = text.slice_to_string(range.clone());
                let runs: Vec<MinimapRun<Hsla>> = fe_display::minimap::row_runs(
                    &row_text,
                    view.model.tab_size(),
                    &row_spans,
                    theme.foreground,
                    self.max_column,
                );
                cache.insert(display, *row, runs, extras.stamp(*row));
            }
        }
    }
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
    /// The selections, find matches and diagnostics under the tiles, in
    /// paint order.
    pub(crate) marks: Vec<(Bounds<Pixels>, Hsla)>,
    /// The git lane's marks, over the tiles.
    pub(crate) lane: Vec<(Bounds<Pixels>, Hsla)>,
    /// The tiles drawn from their rows' runs as cached, by index; others
    /// may be drawn as they were.
    pub(crate) tiles: Vec<u32>,
    /// Every tile drawn, by index.
    pub(crate) images: Vec<u32>,
    pub(crate) slider: Option<(Bounds<Pixels>, Hsla)>,
}

/// Paints the minimap `layout` placed: its marks under its tiles, the git
/// lane over them, then the slider, with the pointer `hover` and the slider
/// `dragged` or not, with an arrow over it.
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
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        for &(rect, color) in &placed.marks.under {
            window.paint_quad(fill(layout.bounds(rect), color));
        }
        for (rect, image) in &placed.tiles {
            let tile = layout.bounds(*rect);
            let painted =
                window.paint_image(tile, tile, Corners::default(), image.clone(), 0, false);
            if let Err(err) = painted {
                log::error!("painting a minimap tile: {err}");
            }
        }
        for &(rect, color) in &placed.marks.lane {
            window.paint_quad(fill(layout.bounds(rect), color));
        }
    });
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
        marks: (placed.marks.under.iter())
            .map(|&(mark, color)| (layout.bounds(mark), color))
            .collect(),
        lane: (placed.marks.lane.iter())
            .map(|&(mark, color)| (layout.bounds(mark), color))
            .collect(),
        tiles: placed.tile_indices.clone(),
        images: placed.image_indices.clone(),
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

#[cfg(test)]
#[path = "minimap_tile_tests.rs"]
mod tile_tests;
