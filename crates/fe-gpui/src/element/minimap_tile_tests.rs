//! The minimap's tiles in a headless window: rebuilt only where their rows'
//! runs changed, moved when scrolling, all rebuilt for a new theme, bounded
//! and dropped from the atlas, and built over frames past the budget.

use std::time::Duration;

use fe_buffer::{Buffer, ResolvedSelection};
use fe_display::minimap::MinimapOptions;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext, px, size};

use super::{SPARE_TILES, TILE_ROWS};
use crate::{EditorTheme, EditorView, init};

/// A Rust editor on `text`, `height` tall, with the minimap on, drawn.
fn editor<'a>(
    text: &str,
    height: f32,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    editor_on(text, "main.rs", height, cx)
}

/// An editor on `text` as the file at `path`.
fn editor_on<'a>(
    text: &str,
    path: &str,
    height: f32,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let (text, path) = (text.to_owned(), path.to_owned());
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), Some(path.into()));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.simulate_resize(size(px(800.), px(height)));
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    view.update(cx, |view, cx| {
        view.set_minimap(Some(MinimapOptions::default()), cx);
        // A loaded debug frame can pass the wall-clock default and stop short;
        // tests of the budget set their own.
        view.minimap_cache.borrow_mut().budget = Duration::from_secs(3600);
    });
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// `rows` numbered rows of Rust.
fn numbered(rows: usize) -> String {
    (0..rows)
        .map(|n| format!("let value_{n} = {n};\n"))
        .collect()
}

/// The tiles built since the last call, by index.
fn built(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    view.read_with(cx, |view, _| {
        let mut built = std::mem::take(&mut view.minimap_cache.borrow_mut().built);
        // A tile may be built again as a reparse lands.
        built.sort();
        built.dedup();
        built
    })
}

/// The tiles the last frame drew, by index.
fn drawn(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    view.read_with(cx, |view, _| {
        view.painted_minimap
            .as_ref()
            .expect("painted")
            .tiles
            .clone()
    })
}

/// Every tile the last frame drew, as it was or not, by index.
fn images(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    view.read_with(cx, |view, _| {
        view.painted_minimap
            .as_ref()
            .expect("painted")
            .images
            .clone()
    })
}

/// Puts the caret at `column` of row `row`.
fn caret_at(view: &Entity<EditorView>, row: u32, column: usize, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let offset = view.model.text().row_start(row) + column;
        view.model.set_selections([ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }]);
        cx.notify();
    });
    draw(cx);
}

/// Scrolls so display row `row` is at the top, and draws.
fn scroll_to(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let line_height = view.layout.as_ref().expect("drawn").frame.line_height;
        view.scroll.y = px(row as f32 * line_height);
        view.autoscroll = false;
        cx.notify();
    });
    draw(cx);
}

#[gpui::test]
fn typing_and_inserting_a_row_rebuild_only_their_tile_and_those_below(cx: &mut TestAppContext) {
    // Tall enough to show every row, so the minimap doesn't scroll: five
    // tiles, the last part full.
    let (view, cx) = editor(&numbered(300), 1400., cx);
    assert_eq!(drawn(&view, cx), [0, 1, 2, 3, 4]);
    // Row 66 is in tile 1: `let va|lue_66 = 66;`.
    caret_at(&view, 66, 6, cx);
    built(&view, cx);
    cx.simulate_keystrokes("x");
    draw(cx);
    assert_eq!(built(&view, cx), [1], "only the edited row's tile");
    assert_eq!(drawn(&view, cx), [0, 1, 2, 3, 4]);

    // A new row moves the rows below it down: their tiles change.
    caret_at(&view, 66, "let vaxlue_66 = 66;".len(), cx);
    built(&view, cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(built(&view, cx), [1, 2, 3, 4], "none above");
    assert_eq!(drawn(&view, cx), [0, 1, 2, 3, 4]);
    // The rows' runs drawn are the text's, as if drawn afresh.
    let painted = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.painted_minimap.as_ref().expect("painted").runs.clone()
        })
    };
    let runs = painted(&view, cx);
    view.update(cx, |view, cx| {
        view.minimap_cache.replace(Default::default());
        view.minimap_cache.borrow_mut().budget = Duration::from_secs(3600);
        cx.notify();
    });
    draw(cx);
    assert_eq!(painted(&view, cx), runs);
}

#[gpui::test]
fn scrolling_by_a_row_moves_the_tiles_and_rebuilds_none(cx: &mut TestAppContext) {
    let (view, cx) = editor(&numbered(1000), 400., cx);
    scroll_to(&view, 100, cx);
    let tiles = drawn(&view, cx);
    let top = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let layout = view.layout.as_ref().expect("drawn");
            layout.minimap.as_ref().expect("shown").geometry.top
        })
    };
    let before = top(cx);
    built(&view, cx);
    scroll_to(&view, 101, cx);
    assert_eq!(drawn(&view, cx), tiles, "the same tiles show");
    assert!(top(cx) > before, "and moved up");
    assert_eq!(built(&view, cx), [] as [u32; 0]);
}

#[gpui::test]
fn a_theme_change_rebuilds_every_tile(cx: &mut TestAppContext) {
    let (view, cx) = editor(&numbered(1000), 400., cx);
    let tiles = drawn(&view, cx);
    built(&view, cx);
    view.update(cx, |view, cx| {
        view.theme.foreground = gpui::red();
        cx.notify();
    });
    draw(cx);
    assert_eq!(built(&view, cx), tiles);
}

#[gpui::test]
fn live_tiles_stay_bounded_and_dropped_ones_leave_the_atlas(cx: &mut TestAppContext) {
    let text: String = (0..1_000_000).map(|n| format!("{n}\n")).collect();
    let (view, cx) = editor_on(&text, "numbers.txt", 400., cx);
    let rows = view.read_with(cx, |view, _| view.model.display().row_count());
    let mut most = 0;
    let mut row = 0;
    while row < rows {
        scroll_to(&view, row, cx);
        let (live, shown) = view.read_with(cx, |view, _| {
            let cache = view.minimap_cache.borrow();
            let painted = view.painted_minimap.as_ref().expect("painted");
            (cache.live_tiles(), painted.tiles.len())
        });
        assert!(live <= shown + SPARE_TILES, "{live} tiles at row {row}");
        most = most.max(live);
        row += 23_456;
    }
    let (released, live) = view.read_with(cx, |view, _| {
        let cache = view.minimap_cache.borrow();
        (cache.released.clone(), cache.images())
    });
    assert!(released.len() > 100, "{} dropped", released.len());
    cx.update(|window, _| {
        for image in &released {
            assert!(!window.has_image_atlas_entry(image), "a dropped tile");
        }
        for image in &live {
            assert!(window.has_image_atlas_entry(image), "a live tile");
        }
    });
    // Turned off, the minimap drops its tiles.
    view.update(cx, |view, cx| view.set_minimap(None, cx));
    draw(cx);
    let left = view.read_with(cx, |view, _| view.minimap_cache.borrow().live_tiles());
    assert_eq!(left, 0, "of {most}");
    cx.update(|window, _| {
        assert!(
            live.iter()
                .all(|image| !window.has_image_atlas_entry(image))
        );
    });
}

#[gpui::test]
fn a_frame_past_its_budget_leaves_tiles_for_the_next(cx: &mut TestAppContext) {
    let (view, cx) = editor(&numbered(5000), 400., cx);
    // No time at all: a frame builds a chunk of rows. A jump to rows no
    // tile has been built for shows the background until they are.
    view.update(cx, |view, _| {
        view.minimap_cache.borrow_mut().budget = Duration::ZERO
    });
    built(&view, cx);
    scroll_to(&view, 4500, cx);
    let shown: Vec<u32> = view.read_with(cx, |view, _| {
        let geometry = &view
            .layout
            .as_ref()
            .expect("drawn")
            .minimap
            .as_ref()
            .expect("shown")
            .geometry;
        (geometry.first_row / TILE_ROWS..geometry.end_row.div_ceil(TILE_ROWS)).collect()
    });
    assert!(shown.len() >= 4, "{shown:?}");
    // That update drew the first frame; each frame asked for draws the next,
    // and builds a chunk of rows, so a tile every few frames.
    let mut frames = 0;
    let mut done: Vec<u32> = Vec::new();
    loop {
        frames += 1;
        let (built, drawn) = (built(&view, cx), drawn(&view, cx));
        assert!(built.len() <= 1, "frame {frames}: {built:?}");
        assert_eq!(images(&view, cx), drawn, "nothing drawn as it was");
        done.extend(built);
        done.sort();
        assert_eq!(drawn, done, "the rest show the background");
        // Only the tiles built draw runs.
        let rows = view.read_with(cx, |view, _| {
            let layout = view.layout.as_ref().expect("drawn");
            let geometry = &layout.minimap.as_ref().expect("shown").geometry;
            let painted = view.painted_minimap.as_ref().expect("painted");
            (painted.runs.iter())
                .map(|(bounds, _)| {
                    let y = f32::from(bounds.top() - layout.origin.y) + geometry.top;
                    (y / geometry.row_height).round() as u32 / TILE_ROWS
                })
                .collect::<Vec<u32>>()
        });
        assert!(
            rows.iter().all(|tile| drawn.contains(tile)),
            "{rows:?} {drawn:?}"
        );
        // Once built, the tiles' decorations are looked at too.
        let requested = cx.update(|window, cx| window.simulate_next_frame(cx));
        if requested == 0 {
            break;
        }
    }
    assert!(frames > shown.len(), "{frames} frames");
    assert_eq!(drawn(&view, cx), shown);
}

#[gpui::test]
fn with_no_time_an_edit_keeps_every_tile_drawn(cx: &mut TestAppContext) {
    let (view, cx) = editor(&numbered(300), 1400., cx);
    let shown = drawn(&view, cx);
    view.update(cx, |view, _| {
        view.minimap_cache.borrow_mut().budget = Duration::ZERO
    });
    built(&view, cx);
    // An edit's frame builds the edited row and draws its tile from the
    // other rows as they were; a new row's tiles below are drawn the same.
    caret_at(&view, 66, 6, cx);
    cx.simulate_keystrokes("x");
    assert_eq!(built(&view, cx), [1], "in the edit's frame");
    assert_eq!(drawn(&view, cx), shown, "typed");
    // Every row's highlights are checked, a chunk a frame; they're the
    // same, so no tile is built again.
    while cx.update(|window, cx| window.simulate_next_frame(cx)) > 0 {
        assert_eq!(drawn(&view, cx), shown);
    }
    assert_eq!(built(&view, cx), [] as [u32; 0]);
    caret_at(&view, 66, "let vaxlue_66 = 66;".len(), cx);
    cx.simulate_keystrokes("enter");
    // The tiles below are drawn as they were until they're built again.
    assert_eq!(images(&view, cx), shown, "a new row");
    while cx.update(|window, cx| window.simulate_next_frame(cx)) > 0 {
        assert_eq!(images(&view, cx), shown);
    }
    assert_eq!(drawn(&view, cx), shown);
    assert_eq!(built(&view, cx), [1, 2, 3, 4]);
}

#[gpui::test]
fn fill_settles_with_every_row_shown_and_redraws_tiles_in_place(cx: &mut TestAppContext) {
    // More rows than the cache keeps by default, all shown.
    let (view, cx) = editor(&numbered(6000), 400., cx);
    let fill = MinimapOptions {
        size: fe_display::minimap::MinimapSize::Fill,
        ..MinimapOptions::default()
    };
    view.update(cx, |view, cx| view.set_minimap(Some(fill), cx));
    draw(cx);
    let shown = drawn(&view, cx);
    // Rows less than a device pixel tall are drawn every so many.
    let rows = view.read_with(cx, |view, _| {
        let painted = view.painted_minimap.as_ref().expect("painted");
        painted
            .runs
            .iter()
            .map(|(bounds, _)| bounds.top())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    });
    assert!(rows < 6001 && shown.len() >= 2, "{rows} rows, {shown:?}");
    assert_eq!(cx.update(|window, cx| window.simulate_next_frame(cx)), 0);
    // A new row shrinks every row: the tiles are drawn again, stretched
    // until then rather than dropped.
    view.update(cx, |view, cx| {
        view.minimap_cache.borrow_mut().budget = Duration::ZERO;
        view.model.set_selections([ResolvedSelection {
            range: 0..0,
            reversed: false,
        }]);
        cx.notify();
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(images(&view, cx), shown);
    while cx.update(|window, cx| window.simulate_next_frame(cx)) > 0 {
        assert_eq!(images(&view, cx), shown);
    }
    assert_eq!(drawn(&view, cx), shown);
}

#[gpui::test]
fn fill_and_fit_draw_a_long_file_at_the_minimaps_resolution(cx: &mut TestAppContext) {
    let text: String = (0..1_000_000).map(|n| format!("{n}\n")).collect();
    let (view, cx) = editor_on(&text, "numbers.txt", 400., cx);
    for size in [
        fe_display::minimap::MinimapSize::Fill,
        fe_display::minimap::MinimapSize::Fit,
    ] {
        let options = MinimapOptions {
            size,
            ..MinimapOptions::default()
        };
        view.update(cx, |view, cx| view.set_minimap(Some(options), cx));
        draw(cx);
        assert_eq!(cx.update(|window, cx| window.simulate_next_frame(cx)), 0);
        let (live, cached, images, height) = view.read_with(cx, |view, _| {
            let cache = view.minimap_cache.borrow();
            let painted = view.painted_minimap.as_ref().expect("painted");
            (
                cache.live_tiles(),
                cache.cached_rows(),
                cache.images(),
                painted.bounds.size.height,
            )
        });
        // A row drawn is a device pixel at least: as many rows as the
        // minimap has pixels, not as the file has rows.
        let pixels = (f32::from(height) * 2.) as usize;
        assert!(
            live * TILE_ROWS as usize <= pixels + 2 * TILE_ROWS as usize,
            "{live} tiles"
        );
        assert!(cached <= 4096, "{cached} rows");
        // And they're drawn: every image has opaque pixels.
        for image in &images {
            let bytes = image.as_bytes(0).expect("a frame");
            assert!(
                bytes.chunks(4).any(|pixel| pixel[3] > 0),
                "{size:?}: a blank tile"
            );
        }
    }
}

/// Colours every row's first three bytes as a keyword, or a string, and
/// logs the length of each range it's asked for.
#[derive(Default)]
struct Keywords {
    strings: bool,
    /// Strings only on the rows above this one.
    strings_below: Option<u32>,
}

thread_local! {
    static ASKED: std::cell::RefCell<Vec<usize>> = Default::default();
}

impl fe_editor::decorations::DecorationProvider for Keywords {
    fn decorations(
        &self,
        snapshot: &fe_buffer::BufferSnapshot,
        range: std::ops::Range<usize>,
        out: &mut Vec<fe_editor::decorations::Decoration>,
    ) {
        ASKED.with(|asked| asked.borrow_mut().push(range.len()));
        let text = snapshot.text();
        let (first, last) = (
            text.offset_to_point(range.start).row,
            text.offset_to_point(range.end).row,
        );
        for row in first..=last {
            let strings = self.strings || self.strings_below.is_some_and(|below| row < below);
            let name: std::sync::Arc<str> = if strings { "string" } else { "keyword" }.into();
            let start = text.row_start(row);
            out.push(fe_editor::decorations::Decoration::new(
                start..start + 3,
                fe_editor::decorations::DecorationKind::Foreground(name.clone()),
                fe_editor::decorations::Tint::Muted,
            ));
        }
    }
}

/// The tiles whose decorations were looked at since the last call.
fn looked(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    view.read_with(cx, |view, _| {
        std::mem::take(&mut view.minimap_cache.borrow_mut().looked)
    })
}

#[gpui::test]
fn decorations_are_asked_for_by_chunk_within_the_budget_and_their_changes_drawn(
    cx: &mut TestAppContext,
) {
    // A long row the minimap shows but the text doesn't: its rest isn't
    // asked for.
    let mut text = numbered(300);
    text.insert_str(
        text.find("let value_150 ").unwrap(),
        &"long ".repeat(100_000),
    );
    let (view, cx) = editor(&text, 400., cx);
    let provider = std::rc::Rc::new(std::cell::RefCell::new(Keywords::default()));
    view.update(cx, |view, cx| {
        view.model.add_decoration_provider(provider.clone());
        cx.notify();
    });
    draw(cx);
    let asked = ASKED.with(|asked| std::mem::take(&mut *asked.borrow_mut()));
    assert!(!asked.is_empty());
    // At most a tile of rows of what their columns can reach, and the gaps
    // between them.
    let reach = 4 * MinimapOptions::default().max_column as usize;
    assert!(
        asked
            .iter()
            .all(|&len| len <= TILE_ROWS as usize * (reach + 16 * 1024)),
        "{asked:?}"
    );
    let shown = drawn(&view, cx);
    assert!(shown.len() >= 3, "{shown:?}");

    // With no time and every tile current, a frame looks at one tile.
    view.update(cx, |view, _| {
        view.minimap_cache.borrow_mut().budget = Duration::ZERO
    });
    looked(&view, cx);
    // The update's end draws a frame.
    view.update(cx, |_, cx| cx.notify());
    assert_eq!(looked(&view, cx).len(), 1);
    assert!(cx.update(|window, cx| window.simulate_next_frame(cx)) > 0);

    // The provider recolours: each tile is looked at in turn, its rows
    // built again in the new colour, and drawn again.
    built(&view, cx);
    provider.borrow_mut().strings = true;
    view.update(cx, |_, cx| cx.notify());
    draw(cx);
    while cx.update(|window, cx| window.simulate_next_frame(cx)) > 0 {}
    assert_eq!(built(&view, cx), shown);
    assert_eq!(drawn(&view, cx), shown);
}

/// One foreground decoration over the whole text.
struct Everything;

impl fe_editor::decorations::DecorationProvider for Everything {
    fn decorations(
        &self,
        snapshot: &fe_buffer::BufferSnapshot,
        _: std::ops::Range<usize>,
        out: &mut Vec<fe_editor::decorations::Decoration>,
    ) {
        out.push(fe_editor::decorations::Decoration::new(
            0..snapshot.text().len(),
            fe_editor::decorations::DecorationKind::Foreground("string".into()),
            fe_editor::decorations::Tint::Muted,
        ));
    }
}

/// Draws frames as asked until none is, at most `limit`; returns how many.
fn settle(limit: usize, cx: &mut VisualTestContext) -> usize {
    (0..limit)
        .find(|_| cx.update(|window, cx| window.simulate_next_frame(cx)) == 0)
        .unwrap_or(limit)
}

#[gpui::test]
fn a_decoration_over_a_gap_between_chunks_stamps_its_rows_the_same_each_time(
    cx: &mut TestAppContext,
) {
    // A long row splits a tile's rows into chunks queried apart, in a
    // sweep's chunk and a build's alike; the decoration spans the gap.
    let mut text = numbered(300);
    text.insert_str(text.find("let value_40 ").unwrap(), &"long ".repeat(10_000));
    let (view, cx) = editor(&text, 400., cx);
    view.update(cx, |view, cx| {
        view.minimap_cache.borrow_mut().budget = Duration::ZERO;
        let everything: std::rc::Rc<
            std::cell::RefCell<dyn fe_editor::decorations::DecorationProvider>,
        > = std::rc::Rc::new(std::cell::RefCell::new(Everything));
        view.model.add_decoration_provider(everything);
        cx.notify();
    });
    assert!(settle(1000, cx) < 1000, "frames never stop");
    built(&view, cx);
    // Looked at again, nothing has changed.
    view.update(cx, |_, cx| cx.notify());
    assert!(settle(1000, cx) < 1000, "frames never stop");
    assert_eq!(built(&view, cx), [] as [u32; 0]);
}

#[gpui::test]
fn an_unchanged_empty_row_keeps_its_tile_across_different_chunk_splits(cx: &mut TestAppContext) {
    // Blank rows where build chunks split: row 15, the last of the first
    // 16-row chunk, and row 42, after a long row whose rest makes a gap
    // between the chunks queried. The decoration spans them all.
    let mut text = numbered(300);
    text.insert(text.find("let value_15 ").unwrap(), '\n');
    text.insert_str(text.find("let value_40 ").unwrap(), &"long ".repeat(10_000));
    text.insert(text.find("let value_41 ").unwrap(), '\n');
    let (view, cx) = editor(&text, 400., cx);
    let blank = |row: u32, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.model.display().row(row).range.is_empty())
    };
    assert!(blank(15, cx) && blank(42, cx));
    view.update(cx, |view, cx| {
        let everything: std::rc::Rc<
            std::cell::RefCell<dyn fe_editor::decorations::DecorationProvider>,
        > = std::rc::Rc::new(std::cell::RefCell::new(Everything));
        view.model.add_decoration_provider(everything);
        cx.notify();
    });
    assert!(settle(1000, cx) < 1000, "frames never stop");
    // Built again with no time: rows 0 to 15 first, then the rest of the
    // tile, swept whole, and any row found stale built alone.
    view.update(cx, |view, cx| {
        view.minimap_cache.replace(super::Cache {
            budget: Duration::ZERO,
            ..Default::default()
        });
        cx.notify();
    });
    assert!(settle(5000, cx) < 5000, "frames never stop");
    built(&view, cx);
    // Looked at again, through other splits, nothing has changed.
    view.update(cx, |_, cx| cx.notify());
    assert!(settle(5000, cx) < 5000, "frames never stop");
    assert_eq!(built(&view, cx), [] as [u32; 0]);
}

#[gpui::test]
fn a_change_to_a_tile_swept_already_is_drawn_before_frames_stop(cx: &mut TestAppContext) {
    let (view, cx) = editor(&numbered(300), 400., cx);
    let provider = std::rc::Rc::new(std::cell::RefCell::new(Keywords::default()));
    view.update(cx, |view, cx| {
        view.model.add_decoration_provider(provider.clone());
        cx.notify();
    });
    draw(cx);
    let shown = drawn(&view, cx);
    assert!(shown.len() >= 3, "{shown:?}");
    // No time: the frame looks at tile 0, the next step at tile 1.
    view.update(cx, |view, cx| {
        view.minimap_cache.borrow_mut().budget = Duration::ZERO;
        cx.notify();
    });
    looked(&view, cx);
    view.update(cx, |_, cx| cx.notify());
    assert!(cx.update(|window, cx| window.simulate_next_frame(cx)) > 0);
    // (The test window may draw again after the step, which begins the
    // sweep anew.)
    assert_eq!(looked(&view, cx)[..2], [0, 1]);
    built(&view, cx);
    // Only tile 0's rows change colour, then the host draws again.
    provider.borrow_mut().strings_below = Some(TILE_ROWS);
    view.update(cx, |_, cx| cx.notify());
    settle(1000, cx);
    assert_eq!(built(&view, cx), [0]);
    let (first, string) = view.read_with(cx, |view, _| {
        let painted = view.painted_minimap.as_ref().expect("painted");
        let string = fe_syntax::Highlight::from_capture("string")
            .and_then(|highlight| view.theme.syntax_color(highlight))
            .expect("a string colour");
        (painted.runs[0].1, string.opacity(super::RUN_ALPHA))
    });
    assert_eq!(first, string, "tile 0 is drawn in the new colour");
}
