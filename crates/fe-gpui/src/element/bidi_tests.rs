//! Carets, hit-testing and visual ranges in rows with right-to-left text,
//! over lines shaped as CoreText shapes them: graphemes in the bidi
//! algorithm's visual order, each one em wide, bidi controls and
//! zero-width chars with no glyph, a combining mark drawn over its base.
//! The view tests use that shaper as the window's text system, so they run
//! on every platform.

use std::{borrow::Cow, sync::Arc};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_display::layout::{MeasuredLine as _, Rect, Vec2};
use fe_editor::EditorModel;
use gpui::{
    Bounds, DevicePixels, Entity, Focusable as _, Font, FontId, FontMetrics, FontRun, GlyphId,
    LineLayout, NoopTextSystem, Pixels, PlatformTextSystem, RenderGlyphParams, ShapedGlyph,
    ShapedRun, Size, TestAppContext, TestDispatcher, TextRenderingMode, VisualTestContext, point,
    px, size,
};
use unicode_bidi::{BidiClass, BidiInfo, bidi_class};
use unicode_segmentation::UnicodeSegmentation as _;

use super::{Bidi, has_rtl};
use crate::element::{FrameLayout, Line, block_rect};
use crate::{EditorTheme, EditorView, init};

/// Each grapheme's advance in [`line`]s.
const EM: f32 = 10.;

/// A line of Rust with a Hebrew string and an Arabic comment. Hebrew
/// letters start at bytes 9, 11, 13 and 15, the closing quote is at 17,
/// the Arabic letters at 23, 25, 27, 29 and 31, and the row ends at 33.
/// The neutrals between the two strings resolve right to left, so bytes 9
/// to 33 are one right-to-left run, painted after `let s = "`.
const RUST_RTL: &str = "let s = \"שלום\"; // مرحبا";
/// A row ending in Hebrew, its first letter ש painted rightmost.
const ENDS_HEBREW: &str = "let s = \"שלום";

/// `text` shaped as CoreText shapes it, a grapheme `em` wide.
fn shaped(text: &str, em: f32) -> (Vec<ShapedGlyph>, f32) {
    let info = BidiInfo::new(text, None);
    let mut glyphs = Vec::new();
    let mut x = 0.;
    for paragraph in &info.paragraphs {
        let (levels, runs) = info.visual_runs(paragraph, paragraph.range.clone());
        for run in runs {
            let mut graphemes: Vec<(usize, &str)> = (text[run.clone()].grapheme_indices(true))
                .map(|(at, grapheme)| (run.start + at, grapheme))
                .collect();
            if levels[run.start].is_rtl() {
                graphemes.reverse();
            }
            for (at, grapheme) in graphemes {
                let mut drawn = false;
                for (ix, ch) in grapheme.char_indices() {
                    let invisible = matches!(
                        bidi_class(ch),
                        BidiClass::BN
                            | BidiClass::LRE
                            | BidiClass::RLE
                            | BidiClass::LRO
                            | BidiClass::RLO
                            | BidiClass::PDF
                            | BidiClass::LRI
                            | BidiClass::RLI
                            | BidiClass::FSI
                            | BidiClass::PDI
                    );
                    if invisible {
                        continue;
                    }
                    glyphs.push(ShapedGlyph {
                        id: GlyphId(1),
                        position: point(px(if drawn { x + em * 0.4 } else { x }), px(0.)),
                        index: at + ix,
                        is_emoji: false,
                    });
                    drawn = true;
                }
                if drawn {
                    x += em;
                }
            }
        }
    }
    (glyphs, x)
}

fn layout(text: &str) -> LineLayout {
    let (glyphs, width) = shaped(text, EM);
    LineLayout {
        font_size: px(EM),
        width: px(width),
        ascent: px(8.),
        descent: px(2.),
        runs: vec![ShapedRun {
            font_id: FontId(0),
            glyphs,
        }],
        len: text.len(),
    }
}

fn line(text: &str) -> Line {
    Line::new(text, Arc::new(layout(text)), Vec::new())
}

/// The x where the glyph for byte `index` of `text` is painted.
fn glyph_x(text: &str, index: usize) -> f32 {
    let (glyphs, _) = shaped(text, EM);
    let glyph = glyphs.iter().find(|glyph| glyph.index == index).unwrap();
    glyph.position.x.into()
}

#[test]
fn only_rows_with_right_to_left_text_get_bidi_maths() {
    for text in [
        "",
        "let s = 1;",
        "e\u{301} 一二 👨\u{200d}👩",
        "a\u{202a}b\u{202c}",
    ] {
        assert!(!has_rtl(text), "{text:?}");
        assert!(line(text).bidi.is_none());
    }
    for text in [RUST_RTL, "a\u{202e}bc\u{202c}", "x ܐ", "a\u{2067}b\u{2069}"] {
        assert!(has_rtl(text), "{text:?}");
        assert!(line(text).bidi.is_some());
    }
}

/// An ASCII row keeps GPUI's maths. The bidi maths over it, forced, agree
/// with GPUI's at every boundary and every x but over the last glyph,
/// whose left half GPUI's `closest_index_for_x` takes for the row's end.
#[test]
fn an_ascii_row_measures_as_gpui_does() {
    let text = "let s = 1; // ok";
    let line = line(text);
    assert!(line.bidi.is_none());
    let layout = layout(text);
    let bidi = Bidi::of(text, &layout);
    for index in 0..=text.len() {
        let gpui: f32 = layout.x_for_index(index).into();
        assert_eq!(line.x_for_index(index), gpui);
        assert_eq!(bidi.x_for_index(index), gpui);
    }
    let last = glyph_x(text, text.len() - 1);
    let mut x = -5.;
    while x < f32::from(layout.width) + 5. {
        let gpui = layout.closest_index_for_x(px(x));
        assert_eq!(line.index_for_x(x), gpui, "{x}");
        if !(last..=last + EM / 2.).contains(&x) {
            assert_eq!(bidi.index_for_x(x), gpui, "{x}");
        }
        assert_eq!(line.char_index_at(x), layout.index_for_x(px(x)), "{x}");
        assert_eq!(bidi.char_at(x), layout.index_for_x(px(x)), "{x}");
        x += 0.5;
    }
    assert_eq!(bidi.index_for_x(last + 2.), text.len() - 1);
}

/// G1: Right steps logically through the Hebrew, then the Arabic, and the
/// caret moves a letter leftwards each time, on the letter's logical edge.
#[test]
fn carets_move_right_to_left_through_an_rtl_run() {
    let line = line(RUST_RTL);
    let x = |index| line.x_for_index(index);
    // After the opening quote, on its right edge: the run's left edge.
    assert_eq!(x(9), 9. * EM);
    // After ש, on its left edge.
    assert_eq!(x(11), glyph_x(RUST_RTL, 9));
    assert!(x(9) < x(11));
    assert!(x(11) > x(13) && x(13) > x(15) && x(15) > x(17));
    let arabic: Vec<f32> = [23, 25, 27, 29, 31, 33].map(x).into();
    assert!(
        arabic.windows(2).all(|pair| pair[0] > pair[1]),
        "{arabic:?}"
    );
    // The LTR text before the run is as GPUI places it.
    for index in 0..=8 {
        assert_eq!(x(index), index as f32 * EM);
    }
}

/// A caret at a direction change goes with its logical predecessor's run:
/// at the end of a row ending in Hebrew, on the left edge of its last
/// letter, ם; at the start of a right-to-left paragraph, on the right edge
/// of its first letter.
#[test]
fn a_caret_at_a_direction_change_goes_with_its_predecessor() {
    let ends = line(ENDS_HEBREW);
    assert_eq!(
        ends.x_for_index(ENDS_HEBREW.len()),
        glyph_x(ENDS_HEBREW, 15)
    );
    assert_eq!(ends.x_for_index(9), 9. * EM);
    // A paragraph starting with Hebrew is right to left: `abc` is painted
    // first, then the Hebrew, its first letter rightmost.
    let text = "שלום abc";
    let rtl = line(text);
    assert_eq!(rtl.x_for_index(0), rtl.width());
    assert_eq!(rtl.x_for_index(text.len()), 3. * EM);
    assert_eq!(rtl.x_for_index(text.len() - 1), 2. * EM);
    // A bidi control has no glyph: the caret after it is the one before.
    let text = "a\u{202e}bc\u{202c}d";
    let controls = line(text);
    assert_eq!(controls.x_for_index(1), controls.x_for_index(4));
    // In the override `c` is painted left of `b`, so the caret after `c`
    // is left of the one after `b`, and is the one after `a`.
    assert!(controls.x_for_index(6) < controls.x_for_index(5));
    assert_eq!(controls.x_for_index(6), controls.x_for_index(1));
}

/// G2: a click on a right-to-left glyph lands on its nearer logical edge:
/// the right half of ש, painted last, gives its start, 9, and the left
/// half its end, 11. Past the row's width is its end; left of it, its
/// start.
#[test]
fn a_click_lands_on_the_nearer_logical_edge() {
    let line = line(ENDS_HEBREW);
    let shin = glyph_x(ENDS_HEBREW, 9);
    assert_eq!(line.index_for_x(shin + 0.7 * EM), 9);
    assert_eq!(line.index_for_x(shin + 0.3 * EM), 11);
    let mem = glyph_x(ENDS_HEBREW, 15);
    assert_eq!(line.index_for_x(mem + 0.3 * EM), ENDS_HEBREW.len());
    assert_eq!(line.index_for_x(mem + 0.7 * EM), 15);
    assert_eq!(line.index_for_x(line.width()), ENDS_HEBREW.len());
    assert_eq!(line.index_for_x(line.width() + 20.), ENDS_HEBREW.len());
    assert_eq!(line.index_for_x(-3.), 0);
    assert_eq!(line.index_for_x(8.6 * EM), 9);
}

/// Every x over a row of Arabic with vowel marks hits a grapheme
/// boundary, never between a letter and its mark.
#[test]
fn clicks_over_marked_letters_land_on_grapheme_boundaries() {
    // Each letter but the last carries a fatha or kasra.
    let text = "x \u{645}\u{64e}\u{631}\u{650}\u{62d}\u{64e}\u{628}\u{627}";
    let boundaries: Vec<usize> = (text.grapheme_indices(true))
        .map(|(at, _)| at)
        .chain([text.len()])
        .collect();
    let line = line(text);
    let mut x = -2.;
    while x < line.width() + 2. {
        let index = line.index_for_x(x);
        assert!(boundaries.contains(&index), "{x}: {index}");
        x += 1.;
    }
}

/// The glyph under a point, as hovering and links read it: the grapheme
/// painted there, wherever its logical position.
#[test]
fn the_char_under_a_point_is_the_glyph_painted_there() {
    let line = line(RUST_RTL);
    for letter in [9, 11, 13, 15, 23, 31] {
        let x = glyph_x(RUST_RTL, letter) + EM / 2.;
        assert_eq!(line.char_index_at(x), Some(letter));
    }
    assert_eq!(line.char_index_at(line.width()), None);
    assert_eq!(line.char_index_at(4.5 * EM), Some(4));
}

/// Inside a right-to-left run a range is one piece, its glyphs' extent;
/// across a direction change it's one piece per run, left to right.
#[test]
fn visual_ranges_have_one_piece_per_run() {
    let line = line(RUST_RTL);
    let lamed = glyph_x(RUST_RTL, 11);
    // של, painted ל then ש.
    assert_eq!(line.visual_ranges(9..13), [lamed..lamed + 2. * EM]);
    // `s = "של`: the LTR text up to the run, then the letters at its right
    // end.
    assert_eq!(
        line.visual_ranges(4..13),
        [4. * EM..9. * EM, lamed..lamed + 2. * EM]
    );
    // The whole run is one piece.
    assert_eq!(line.visual_ranges(9..33), [9. * EM..line.width()]);
    assert_eq!(line.visual_ranges(0..33), [0. ..line.width()]);
    // Empty, at its caret.
    let x = line.x_for_index(11);
    assert_eq!(line.visual_ranges(11..11), [x..x]);
    // An LTR row is one piece from x to x, as by default.
    let ltr = self::line("let s = 1;");
    assert!(ltr.bidi.is_none());
    assert_eq!(ltr.visual_ranges(4..7), [4. * EM..7. * EM]);
}

/// A line of `text` painted as `glyphs`, each a byte index and an x.
fn painted(text: &str, glyphs: &[(usize, f32)], width: f32) -> Line {
    let glyphs = (glyphs.iter())
        .map(|&(index, x)| ShapedGlyph {
            id: GlyphId(1),
            position: point(px(x), px(0.)),
            index,
            is_emoji: false,
        })
        .collect();
    let layout = LineLayout {
        font_size: px(EM),
        width: px(width),
        ascent: px(8.),
        descent: px(2.),
        runs: vec![ShapedRun {
            font_id: FontId(0),
            glyphs,
        }],
        len: text.len(),
    };
    Line::new(text, Arc::new(layout), Vec::new())
}

/// A lam-alef ligature is one glyph, at the lam's index: lam and alef
/// share it, the lam on its right half, the alef on its left, so each of
/// the ligature's three boundaries has a caret and a click on either half
/// lands on that letter's nearer edge.
#[test]
fn a_ligature_is_shared_between_its_graphemes() {
    // `x `, then lam (2..4) and alef (4..6) drawn as one glyph.
    let text = "x \u{644}\u{627}";
    let line = painted(text, &[(0, 0.), (1, EM), (2, 2. * EM)], 4. * EM);
    let (left, right) = (2. * EM, 4. * EM);
    let middle = (left + right) / 2.;
    assert_eq!(line.x_for_index(2), left);
    assert_eq!(line.x_for_index(4), middle);
    assert_eq!(line.x_for_index(6), left);
    // The lam's right half gives its start, its left half its end.
    assert_eq!(line.index_for_x(right - 1.), 2);
    assert_eq!(line.index_for_x(middle + 1.), 4);
    // The alef's left half gives its end.
    assert_eq!(line.index_for_x(left + 1.), 6);
    assert_eq!(line.char_index_at(left + 1.), Some(4));
    assert_eq!(line.visual_ranges(4..6), [left..middle]);
    assert_eq!(line.visual_ranges(2..4), [middle..right]);
    assert_eq!(line.visual_ranges(2..6), [left..right]);
    // An alef with a fatha (6..8) of its own still shares the ligature.
    let text = "x \u{644}\u{627}\u{64e}";
    let line = painted(
        text,
        &[(0, 0.), (1, EM), (6, 2. * EM - 2.), (2, 2. * EM)],
        4. * EM,
    );
    assert_eq!(line.x_for_index(4), middle);
    assert_eq!(line.x_for_index(8), left);
    assert_eq!(line.visual_ranges(4..8), [left..middle]);
    assert_eq!(line.visual_ranges(1..2), [EM..left]);
    assert_eq!(line.index_for_x(left + 1.), 8);
    assert_eq!(line.index_for_x(middle - 1.), 4);
    // A bidi control with no glyph is no share of a ligature.
    let text = "x \u{644}\u{200f}";
    let line = painted(text, &[(0, 0.), (1, EM), (2, 2. * EM)], 3. * EM);
    assert_eq!(line.visual_ranges(2..4), [2. * EM..3. * EM]);
    assert_eq!(line.x_for_index(text.len()), 2. * EM);
}

/// A mark positioned left of its base doesn't move the grapheme's edges,
/// nor its neighbour's: the base's glyph places it.
#[test]
fn a_mark_left_of_its_base_does_not_move_its_edges() {
    // `x `, then shin (2..4) with a qamats (4..6) offset 2 left of it.
    let text = "x \u{5e9}\u{5b8}";
    let line = painted(
        text,
        &[(0, 0.), (1, EM), (4, 2. * EM - 2.), (2, 2. * EM)],
        3. * EM,
    );
    assert_eq!(line.x_for_index(6), 2. * EM);
    assert_eq!(line.x_for_index(2), 2. * EM);
    assert_eq!(line.visual_ranges(2..6), [2. * EM..3. * EM]);
    assert_eq!(line.visual_ranges(1..2), [EM..2. * EM]);
    assert_eq!(line.index_for_x(2. * EM - 1.), 2);
    assert_eq!(line.index_for_x(3. * EM - 1.), 2);
    assert_eq!(line.index_for_x(2. * EM + 1.), 6);
}

/// A mark that shares its base's cluster index, offset left of it, is told
/// from the base by where the base's advance reaches, so the grapheme keeps
/// the base's edges, as the same grapheme with no mark does.
#[test]
fn a_mark_sharing_its_bases_index_does_not_move_its_edges() {
    // `x `, then shin (2..4) at 2 em with a qamats offset 2 left of it,
    // both at byte 2, in either glyph order.
    let text = "x \u{5e9}\u{5b8}";
    let plain = painted("x \u{5e9}", &[(0, 0.), (1, EM), (2, 2. * EM)], 3. * EM);
    let (base, mark) = ((2, 2. * EM), (2, 2. * EM - 2.));
    for glyphs in [
        [(0, 0.), (1, EM), base, mark],
        [(0, 0.), (1, EM), mark, base],
    ] {
        let line = painted(text, &glyphs, 3. * EM);
        for index in [0, 1, 2] {
            assert_eq!(line.x_for_index(index), plain.x_for_index(index), "{index}");
        }
        // The trailing caret, and the block over the grapheme.
        assert_eq!(line.x_for_index(6), 2. * EM);
        assert_eq!(line.visual_ranges(2..6), [2. * EM..3. * EM]);
        // The neighbour on the left keeps its full width.
        assert_eq!(line.visual_ranges(1..2), [EM..2. * EM]);
        for x in [EM + 1., 2. * EM - 1., 2. * EM + 1., 3. * EM - 1.] {
            // The shin's end is byte 4 plain and 6 with its mark.
            let end = |index| if index == 4 { 6 } else { index };
            assert_eq!(line.index_for_x(x), end(plain.index_for_x(x)), "{x}");
            assert_eq!(line.char_index_at(x), plain.char_index_at(x), "{x}");
        }
    }
}

/// Marked graphemes side by side: each mark lands on the next one's x, so
/// a base is told by the chain from the line's end, not by any glyph's x.
#[test]
fn adjacent_marked_graphemes_keep_their_bases_edges() {
    // `x `, then two shins (2..4, 6..8), each with a qamats 2 left of it.
    let text = "x \u{5e9}\u{5b8}\u{5e9}\u{5b8}";
    let (a, b) = (2. * EM, 3. * EM);
    for glyphs in [
        [(0, 0.), (1, EM), (2, a), (2, a - 2.), (6, b), (6, b - 2.)],
        [(0, 0.), (1, EM), (2, a - 2.), (2, a), (6, b - 2.), (6, b)],
    ] {
        let line = painted(text, &glyphs, 4. * EM);
        assert_eq!(line.x_for_index(6), a);
        assert_eq!(line.x_for_index(10), b);
        assert_eq!(line.visual_ranges(2..6), [a..b]);
        assert_eq!(line.visual_ranges(6..10), [b..4. * EM]);
        assert_eq!(line.visual_ranges(1..2), [EM..a]);
        assert_eq!(line.index_for_x(a + 1.), 6);
        assert_eq!(line.index_for_x(b + 1.), 10);
        assert_eq!(line.char_index_at(a - 1.), Some(1));
    }
}

/// Moving a line's glyphs moves its graphemes with them.
#[test]
fn moved_glyphs_carry_their_directions() {
    let line = line(ENDS_HEBREW);
    let moved = line.with_glyphs_moved(line.width() + 5., |_| Some(5.));
    for index in [0, 9, 11, 15, ENDS_HEBREW.len()] {
        assert_eq!(moved.x_for_index(index), line.x_for_index(index) + 5.);
    }
    let shin = glyph_x(ENDS_HEBREW, 9) + 5.;
    assert_eq!(moved.index_for_x(shin + 0.7 * EM), 9);
}

/// The window's text system: GPUI's no-op one, but laying lines out with
/// [`shaped`], a grapheme as wide as its em.
struct BidiShaper(NoopTextSystem);

impl PlatformTextSystem for BidiShaper {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> gpui::Result<()> {
        self.0.add_fonts(fonts)
    }

    fn all_font_names(&self) -> Vec<String> {
        self.0.all_font_names()
    }

    fn font_id(&self, descriptor: &Font) -> gpui::Result<FontId> {
        self.0.font_id(descriptor)
    }

    fn font_metrics(&self, font_id: FontId) -> FontMetrics {
        self.0.font_metrics(font_id)
    }

    fn typographic_bounds(&self, font_id: FontId, glyph_id: GlyphId) -> gpui::Result<Bounds<f32>> {
        self.0.typographic_bounds(font_id, glyph_id)
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> gpui::Result<Size<f32>> {
        self.0.advance(font_id, glyph_id)
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        self.0.glyph_for_char(font_id, ch)
    }

    fn glyph_raster_bounds(
        &self,
        params: &RenderGlyphParams,
    ) -> gpui::Result<Bounds<DevicePixels>> {
        self.0.glyph_raster_bounds(params)
    }

    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        raster_bounds: Bounds<DevicePixels>,
    ) -> gpui::Result<(Size<DevicePixels>, Vec<u8>)> {
        self.0.rasterize_glyph(params, raster_bounds)
    }

    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> LineLayout {
        let noop = self.0.layout_line(text, font_size, runs);
        // The no-op system's em: 600 units of 1000.
        let (glyphs, width) = shaped(text, f32::from(font_size) * 0.6);
        let runs = match glyphs.is_empty() {
            true => Vec::new(),
            false => vec![ShapedRun {
                font_id: FontId(0),
                glyphs,
            }],
        };
        LineLayout {
            width: px(width),
            runs,
            ..noop
        }
    }

    fn recommended_rendering_mode(&self, font_id: FontId, font_size: Pixels) -> TextRenderingMode {
        self.0.recommended_rendering_mode(font_id, font_size)
    }
}

/// Runs `test` on an editor showing `text`, drawn with [`BidiShaper`].
fn with_editor(
    name: &'static str,
    text: &str,
    test: impl FnOnce(&Entity<EditorView>, &mut VisualTestContext),
) {
    let mut cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        Some(name),
        Arc::new(BidiShaper(NoopTextSystem::new())),
    );
    cx.update(init);
    let text = text.to_owned();
    let (view, window) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_selections([ResolvedSelection {
            range: 0..0,
            reversed: false,
        }]);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    window.simulate_resize(size(px(800.), px(400.)));
    window.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    window.run_until_parked();
    window.update(|window, cx| window.draw(cx).clear(cx));
    test(&view, window);
    cx.run_until_parked();
    cx.update(|cx| {
        cx.background_executor().forbid_parking();
        cx.quit();
    });
    cx.run_until_parked();
}

/// The frame's caret rect at `offset`, and the width of a space.
fn caret(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> (Rect, f32) {
    view.update(cx, |view, _| {
        let layout = view.layout.as_mut().expect("drawn");
        let space = layout.space_width().into();
        let FrameLayout {
            frame,
            long_rows,
            glyph_widths,
            ..
        } = layout;
        let caret = frame.caret_bounds(offset, long_rows, glyph_widths).unwrap();
        (caret, space)
    })
}

/// The vim block caret on a Hebrew letter covers that letter's glyph, one
/// em wide, not the gap from its start's caret to its end's.
#[test]
fn the_block_caret_on_a_right_to_left_letter_is_its_glyph_wide() {
    with_editor(
        "the_block_caret_on_a_right_to_left_letter_is_its_glyph_wide",
        RUST_RTL,
        |view, cx| {
            for (letter, next) in [(9, 11), (11, 13), (15, 17), (23, 25)] {
                let (after, em) = caret(view, next, cx);
                let block = view.update(cx, |view, _| {
                    let text = view.model.text().clone();
                    block_rect(view.layout.as_mut().unwrap(), &text, letter).unwrap()
                });
                // Its left edge is where the caret after it is drawn.
                assert_eq!(block.x, after.x, "{letter}");
                assert!((block.width - em).abs() < 0.01, "{letter}: {block:?}");
            }
            // Left-to-right text keeps its block.
            let (at, em) = caret(view, 4, cx);
            let block = view.update(cx, |view, _| {
                let text = view.model.text().clone();
                block_rect(view.layout.as_mut().unwrap(), &text, 4).unwrap()
            });
            assert_eq!(block.x, at.x);
            assert!((block.width - em).abs() < 0.01, "{block:?}");
        },
    );
}

/// The char under a point in a right-to-left run is the letter painted
/// there, and a click on a letter's right half puts the caret at its start.
#[test]
fn the_char_at_a_point_in_a_right_to_left_run_is_the_glyph_under_it() {
    with_editor(
        "the_char_at_a_point_in_a_right_to_left_run_is_the_glyph_under_it",
        RUST_RTL,
        |view, cx| {
            for (letter, next) in [(9, 11), (13, 15), (27, 29)] {
                // The letter's left edge is the caret after it.
                let (after, em) = caret(view, next, cx);
                let (char_at, offset_at) = view.update(cx, |view, _| {
                    let text = view.model.text().clone();
                    let layout = view.layout.as_mut().unwrap();
                    let at = |part: f32| {
                        layout.to_window(Vec2 {
                            x: after.x + em * part,
                            y: after.y + after.height / 2.,
                        })
                    };
                    let (middle, right) = (at(0.5), at(0.8));
                    (
                        layout.char_at(&text, middle),
                        layout.offset_at(&text, right),
                    )
                });
                assert_eq!(char_at, Some(letter));
                assert_eq!(offset_at, letter);
            }
        },
    );
}

/// A selection inside a right-to-left run, whose end's caret is left of
/// its start's, is still a rect from the left one to the right one.
#[test]
fn a_selection_inside_a_right_to_left_run_is_not_inside_out() {
    with_editor(
        "a_selection_inside_a_right_to_left_run_is_not_inside_out",
        RUST_RTL,
        |view, cx| {
            view.update(cx, |view, _| {
                view.model.set_selections([ResolvedSelection {
                    range: 11..15,
                    reversed: false,
                }])
            });
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let (start, _) = caret(view, 11, cx);
            let (end, _) = caret(view, 15, cx);
            assert!(end.x < start.x);
            let rects = view.read_with(cx, |view, _| {
                view.layout.as_ref().unwrap().frame.selections.clone()
            });
            assert_eq!(rects.len(), 1);
            assert_eq!((rects[0].x, rects[0].x + rects[0].width), (end.x, start.x));
        },
    );
}
