//! Non-Latin text in the editor view (T1431): what works, locked in, and
//! the gaps docs/decisions/yzegn08wk2-non-latin-text-audit.md writes up,
//! each test pinning one naming its id (G<n>).
//!
//! `#[gpui::test]` lays text out with GPUI's `NoopTextSystem`: every char
//! one em wide, no shaping, reordering or fallback. Tests of caret and
//! selection logic use it, so they run anywhere. Tests of what shaping
//! decides (glyph order, widths, fallback fonts, hit-testing) use macOS's
//! CoreText shaper instead ([`shaped`]), and run on macOS only: it ships the
//! fonts they need (PingFang, Apple Color Emoji, Arial Hebrew, Geeza Pro),
//! and GPUI builds its text system only with the Mac platform.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::ops::Range;

use fe_buffer::{Buffer, ResolvedSelection};
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, init, set_vim_enabled};
use gpui::{
    Bounds, Entity, EntityInputHandler as _, Focusable as _, Modifiers, MouseButton,
    MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext, VisualTestContext, point, px,
    size,
};

/// A line of Rust with a Hebrew string and an Arabic comment. Hebrew
/// letters start at bytes 9, 11, 13 and 15, the closing quote is at 17,
/// the Arabic letters at 23, 25, 27, 29 and 31, and the row ends at 33.
const RUST_RTL: &str = "let s = \"שלום\"; // مرحبا";
const HEBREW: [usize; 4] = [9, 11, 13, 15];
const ARABIC: [usize; 5] = [23, 25, 27, 29, 31];

/// One of each kind of cluster between ASCII letters: a ZWJ family, a
/// flag, a keycap, `e` with a combining acute, Hangul jamo making one
/// syllable, and a Devanagari conjunct.
const CLUSTERS: &str =
    "a👨\u{200d}👩\u{200d}👧b🇨🇦c1\u{fe0f}\u{20e3}de\u{301}f\u{1112}\u{1161}\u{11ab}gक\u{94d}षh";
/// Where each cluster of [`CLUSTERS`] ends, from the first `a` on.
const CLUSTER_ENDS: [usize; 13] = [1, 19, 20, 28, 29, 36, 37, 40, 41, 50, 51, 60, 61];

/// The CoreText shaper, which GPUI only builds with its Mac platform, and
/// that only on the main thread. libtest runs tests on threads of their
/// own, so the platform is built before `main`, from the binary's
/// initialiser list, and kept for the process.
#[cfg(target_os = "macos")]
mod shaped {
    use std::sync::{Arc, OnceLock};

    use gpui::{PlatformTextSystem, TestAppContext, TestDispatcher};

    static TEXT_SYSTEM: OnceLock<Arc<dyn PlatformTextSystem>> = OnceLock::new();

    extern "C" fn build_text_system() {
        let platform = gpui_platform::current_platform(true);
        TEXT_SYSTEM.get_or_init(|| platform.text_system());
        // Kept: its text system holds on to what the platform made.
        std::mem::forget(platform);
    }

    #[used]
    #[unsafe(link_section = "__DATA,__mod_init_func")]
    static BUILD_TEXT_SYSTEM: extern "C" fn() = build_text_system;

    /// Runs `test` in an app shaping text with CoreText, and shuts it down.
    pub(super) fn with_app(name: &'static str, test: impl FnOnce(&mut TestAppContext)) {
        let text_system = TEXT_SYSTEM.get().expect("built before main").clone();
        let mut cx =
            TestAppContext::build_with_text_system(TestDispatcher::new(0), Some(name), text_system);
        test(&mut cx);
        cx.run_until_parked();
        cx.update(|cx| {
            cx.background_executor().forbid_parking();
            cx.quit();
        });
        cx.run_until_parked();
    }
}

/// A focused editor 800 by 400 showing `text`, with carets at `carets`
/// (the last is the primary one) and vim on or off, drawn once.
fn editor<'a>(
    text: &str,
    carets: &[usize],
    vim: bool,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_owned();
    let carets = carets.to_vec();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_selections(carets.into_iter().map(|at| ResolvedSelection {
            range: at..at,
            reversed: false,
        }));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.simulate_resize(size(px(800.), px(400.)));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
        set_vim_enabled(cx, vim);
    });
    cx.run_until_parked();
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
    draw(cx);
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn selections(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Range<usize>> {
    view.read_with(cx, |view, _| {
        (view.model().selections().iter())
            .map(|selection| selection.range.clone())
            .collect()
    })
}

/// The one selection.
fn selection(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Range<usize> {
    let selections = selections(view, cx);
    assert_eq!(selections.len(), 1, "{selections:?}");
    selections[0].clone()
}

fn head(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |view, _| view.model().primary_selection().head())
}

fn set_caret(view: &Entity<EditorView>, at: usize, cx: &mut VisualTestContext) {
    view.update(cx, |view, _| {
        view.model_mut().set_selections([ResolvedSelection {
            range: at..at,
            reversed: false,
        }])
    });
    draw(cx);
}

/// Runs `f` on the view as the platform's input handler does.
fn input<R>(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    f: impl FnOnce(&mut EditorView, &mut gpui::Window, &mut gpui::Context<EditorView>) -> R,
) -> R {
    cx.update(|window, cx| view.update(cx, |view, cx| f(view, window, cx)))
}

fn utf16(text: &str, offset: usize) -> usize {
    text[..offset].chars().map(char::len_utf16).sum()
}

fn byte_at_utf16(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (at, ch) in text.char_indices() {
        if units >= utf16 {
            return at;
        }
        units += ch.len_utf16();
    }
    text.len()
}

/// The window bounds the input method is given for `range`, buffer bytes.
fn bounds(
    view: &Entity<EditorView>,
    range: Range<usize>,
    cx: &mut VisualTestContext,
) -> Bounds<Pixels> {
    let text = text(view, cx);
    let range = utf16(&text, range.start)..utf16(&text, range.end);
    input(view, cx, |view, window, cx| {
        view.bounds_for_range(range, Bounds::default(), window, cx)
    })
    .expect("the row is drawn")
}

/// The caret's left edge at `offset`, in the window.
fn caret_x(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> f32 {
    bounds(view, offset..offset, cx).origin.x.into()
}

/// The point at `x` halfway down `offset`'s row.
fn on_row(
    view: &Entity<EditorView>,
    offset: usize,
    x: f32,
    cx: &mut VisualTestContext,
) -> Point<Pixels> {
    let caret = bounds(view, offset..offset, cx);
    point(px(x), caret.origin.y + caret.size.height / 2.)
}

/// The buffer offset the input method is told is under `at`.
fn hit(view: &Entity<EditorView>, at: Point<Pixels>, cx: &mut VisualTestContext) -> usize {
    let text = text(view, cx);
    let utf16 = input(view, cx, |view, window, cx| {
        view.character_index_for_point(at, window, cx)
    })
    .expect("an index");
    byte_at_utf16(&text, utf16)
}

fn click(at: Point<Pixels>, count: usize, modifiers: Modifiers, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseDownEvent {
        position: at,
        button: MouseButton::Left,
        modifiers,
        click_count: count,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: at,
        button: MouseButton::Left,
        modifiers,
        click_count: count,
    });
    cx.run_until_parked();
    draw(cx);
}

/// `text` shaped in the editor's font as one run: each glyph's byte index,
/// x from the line's start and font, in the order they're painted.
#[cfg(target_os = "macos")]
fn glyphs(text: &str, cx: &mut VisualTestContext) -> Vec<(usize, f32, gpui::FontId)> {
    let line = cx.update(|window, cx| {
        let theme = EditorTheme::default_dark(cx);
        let run = gpui::TextRun {
            len: text.len(),
            font: theme.font(),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window
            .text_system()
            .layout_line(text, theme.font_size, &[run], None)
    });
    (line.runs.iter())
        .flat_map(|run| {
            (run.glyphs.iter()).map(|glyph| (glyph.index, glyph.position.x.into(), run.font_id))
        })
        .collect()
}

/// Where the glyph for byte `index` comes in paint order.
#[cfg(target_os = "macos")]
fn paint_order(glyphs: &[(usize, f32, gpui::FontId)], index: usize) -> usize {
    (glyphs.iter())
        .position(|glyph| glyph.0 == index)
        .expect("a glyph at the index")
}

/// The window x where the glyph for byte `index` of `row`, the editor's
/// first row, is painted: the row's text starts at offset 0's caret.
#[cfg(target_os = "macos")]
fn glyph_x(view: &Entity<EditorView>, row: &str, index: usize, cx: &mut VisualTestContext) -> f32 {
    let origin = caret_x(view, 0, cx);
    let glyphs = glyphs(row, cx);
    origin + glyphs[paint_order(&glyphs, index)].1
}

/// Editor settings soft wrapping at column 20.
fn wrap_at_20(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let settings = EditorSettings {
            wrap_column: 20,
            wrap_mode: fe_config::settings::WrapMode::Column,
            ..EditorSettings::default()
        };
        view.apply_settings(&settings, cx);
        view.model_mut().set_soft_wrap(true);
    });
    cx.run_until_parked();
    draw(cx);
}

// Right-to-left and mixed direction.

/// Arrows move logically through a mixed-direction row, as VS Code's do:
/// Right steps one grapheme in byte order through a Hebrew string,
/// shift-Right selects in byte order, and Home and End go to the row's
/// logical start and end. The text is unchanged.
#[gpui::test]
fn rtl_logical_caret_steps_one_grapheme(cx: &mut TestAppContext) {
    let (view, cx) = editor(RUST_RTL, &[8], false, cx);
    let mut steps = Vec::new();
    for _ in 0..6 {
        keys("right", cx);
        steps.push(head(&view, cx));
    }
    assert_eq!(steps, [9, 11, 13, 15, 17, 18]);
    set_caret(&view, 9, cx);
    keys("shift-right shift-right", cx);
    assert_eq!(selection(&view, cx), 9..13);
    keys("end", cx);
    assert_eq!(head(&view, cx), RUST_RTL.len());
    keys("home", cx);
    assert_eq!(head(&view, cx), 0);
    assert_eq!(text(&view, cx), RUST_RTL);
}

/// CoreText reorders the row by the Unicode bidi algorithm before the
/// editor paints it: in paint order each Hebrew and Arabic letter comes
/// after the one following it in the buffer, and the neutrals between the
/// two RTL strings (`"; // `) resolve right to left too, so the comment's
/// Arabic is drawn left of the string's Hebrew, as VS Code draws it.
#[cfg(target_os = "macos")]
#[test]
fn rtl_shaper_paints_rtl_runs_right_to_left() {
    shaped::with_app("rtl_shaper_paints_rtl_runs_right_to_left", |cx| {
        let (_, cx) = editor(RUST_RTL, &[0], false, cx);
        let glyphs = glyphs(RUST_RTL, cx);
        assert!(
            glyphs.windows(2).all(|pair| pair[0].1 <= pair[1].1),
            "{glyphs:?}"
        );
        let order = |indices: &[usize]| -> Vec<usize> {
            (indices.iter())
                .map(|&index| paint_order(&glyphs, index))
                .collect()
        };
        let descending = |order: Vec<usize>| order.windows(2).all(|pair| pair[0] > pair[1]);
        assert!(descending(order(&HEBREW)), "{glyphs:?}");
        assert!(descending(order(&ARABIC)), "{glyphs:?}");
        assert!(order(&ARABIC)[0] < order(&HEBREW)[3], "{glyphs:?}");
        assert_eq!(order(&[0, 1, 2, 3]), [0, 1, 2, 3]);
    });
}

/// Limitation G1: every caret position inside a right-to-left span gets
/// the same x, the span's left edge, because GPUI's
/// `LineLayout::x_for_index` takes the first glyph in paint order whose
/// byte index is at or past the caret's. So the caret doesn't move on
/// screen while Right steps through the Hebrew and Arabic, and the block
/// caret, selection rects and input method window all sit there too.
#[cfg(target_os = "macos")]
#[test]
fn rtl_carets_inside_an_rtl_span_share_one_x() {
    shaped::with_app("rtl_carets_inside_an_rtl_span_share_one_x", |cx| {
        let (view, cx) = editor(RUST_RTL, &[0], false, cx);
        let inside: Vec<f32> = (RUST_RTL.char_indices())
            .map(|(at, _)| at)
            .filter(|at| (HEBREW[0]..=ARABIC[4]).contains(at))
            .map(|at| caret_x(&view, at, cx))
            .collect();
        assert!(inside.iter().all(|&x| x == inside[0]), "{inside:?}");
        assert!(caret_x(&view, HEBREW[0] - 1, cx) < inside[0]);
        assert!(caret_x(&view, RUST_RTL.len(), cx) > inside[0]);
    });
}

/// Hit-testing a mixed-direction row never panics and always gives a char
/// boundary on the row, from left of the text to past its end; a click
/// anywhere along it puts the caret on such a boundary.
#[cfg(target_os = "macos")]
#[test]
fn rtl_hit_testing_a_mixed_row_lands_on_char_boundaries() {
    shaped::with_app(
        "rtl_hit_testing_a_mixed_row_lands_on_char_boundaries",
        |cx| {
            let full = format!("{RUST_RTL}\nnext");
            let (view, cx) = editor(&full, &[0], false, cx);
            let left = caret_x(&view, 0, cx);
            let right = caret_x(&view, RUST_RTL.len(), cx);
            let mut x = left - 10.;
            let mut step = 0;
            while x < right + 20. {
                let at = on_row(&view, 0, x, cx);
                let offset = hit(&view, at, cx);
                let on_row = offset <= RUST_RTL.len() && full.is_char_boundary(offset);
                assert!(on_row, "{x}: {offset}");
                if step % 8 == 0 {
                    click(at, 1, Modifiers::none(), cx);
                    let caret = head(&view, cx);
                    let on_row = caret <= RUST_RTL.len() && full.is_char_boundary(caret);
                    assert!(on_row, "{x}: {caret}");
                }
                x += 2.;
                step += 1;
            }
            assert_eq!(text(&view, cx), full);
        },
    );
}

/// Limitation G2: hit-testing reads glyphs as if they ran left to right.
/// A click on the rightmost glyph of a row that ends in right-to-left
/// text (the Hebrew string's first letter, painted last) lands at the
/// row's end, not beside that letter (9 or 11).
#[cfg(target_os = "macos")]
#[test]
fn rtl_click_on_the_last_painted_rtl_glyph_lands_at_the_row_end() {
    shaped::with_app(
        "rtl_click_on_the_last_painted_rtl_glyph_lands_at_the_row_end",
        |cx| {
            // Without the comment, the Hebrew is the row's last text.
            let row = "let s = \"שלום";
            let (view, cx) = editor(row, &[0], false, cx);
            let x = glyph_x(&view, row, HEBREW[0], cx) + 4.;
            click(on_row(&view, 0, x, cx), 1, Modifiers::none(), cx);
            assert_eq!(head(&view, cx), row.len());
        },
    );
}

/// A double-click on a Hebrew or Arabic glyph selects that whole word:
/// the glyph's byte index lies inside it, wherever it's painted.
#[cfg(target_os = "macos")]
#[test]
fn rtl_double_click_selects_the_rtl_word_under_the_pointer() {
    shaped::with_app(
        "rtl_double_click_selects_the_rtl_word_under_the_pointer",
        |cx| {
            let (view, cx) = editor(RUST_RTL, &[0], false, cx);
            for (letter, word) in [(HEBREW[1], 9..17), (ARABIC[2], 23..33)] {
                let x = glyph_x(&view, RUST_RTL, letter, cx) + 2.;
                let at = on_row(&view, 0, x, cx);
                click(at, 1, Modifiers::none(), cx);
                click(at, 2, Modifiers::none(), cx);
                assert_eq!(selection(&view, cx), word);
            }
        },
    );
}

/// Limitation G4: bidi controls and zero-width chars draw nothing. An
/// RLO (U+202E) up to a PDF (U+202C) reverses `bc` on screen to `cb`, and
/// neither control, nor a zero-width space, gets a glyph, so the row reads
/// `acbd ef` with nothing to show why (Trojan Source; VS Code's
/// `editor.unicodeHighlight` marks them).
#[cfg(target_os = "macos")]
#[test]
fn rtl_bidi_controls_reorder_text_and_draw_nothing() {
    shaped::with_app("rtl_bidi_controls_reorder_text_and_draw_nothing", |cx| {
        let row = "a\u{202e}bc\u{202c}d e\u{200b}f";
        let (_, cx) = editor(row, &[0], false, cx);
        let glyphs = glyphs(row, cx);
        let (b, c) = (row.find('b').unwrap(), row.find('c').unwrap());
        assert!(
            paint_order(&glyphs, c) < paint_order(&glyphs, b),
            "{glyphs:?}"
        );
        for invisible in ['\u{202e}', '\u{202c}', '\u{200b}'] {
            let at = row.find(invisible).unwrap();
            let drawn = glyphs.iter().any(|glyph| glyph.0 == at);
            assert!(!drawn, "{invisible:?}: {glyphs:?}");
        }
    });
}

// Full-width CJK.

/// How many CJK chars fit `wrap_column` 20 in the editor's font, from the
/// shaped advance of `一` (`layout_width`, which the wrap code measures a
/// char with) and of one ASCII column (`em_advance`, the wrap column's
/// unit). A row breaks where `x + width > wrap width` (`fe-display`
/// `break_points`), so an exact fit stays.
#[cfg(target_os = "macos")]
fn cjk_chars_in_20_columns(cx: &mut VisualTestContext) -> usize {
    let (cjk, column): (f32, f32) = cx.update(|window, cx| {
        let theme = EditorTheme::default_dark(cx);
        let system = window.text_system();
        let font = system.resolve_font(&theme.font());
        let cjk = system.layout_width(font, theme.font_size, '一');
        let column = system
            .em_advance(font, theme.font_size)
            .expect("an advance");
        (cjk.into(), column.into())
    });
    // The wrap code's own sum and comparison, in f32 with no tolerance: on
    // macOS 27 `一` is 13.000001, so 12 of them (156.00001) overflow 20 * 7.8.
    let (mut fits, mut x) = (0, 0f32);
    while x + cjk <= 20. * column {
        x += cjk;
        fits += 1;
    }
    fits
}

/// Limitation G8: soft wrap at a column counts a wide char by its shaped
/// width, a little over 1.67 columns on macOS 27 (11 chars fit
/// `wrap_column` 20) and under 1.67 on macOS 15 (12 fit), not 2: more than
/// the 10 VS Code and nvim fit. Rows only ever break between chars.
#[cfg(target_os = "macos")]
#[test]
fn cjk_soft_wrap_at_a_column_counts_the_shaped_width() {
    shaped::with_app("cjk_soft_wrap_at_a_column_counts_the_shaped_width", |cx| {
        let row = "一二三四五六七八九十".repeat(3);
        let (view, cx) = editor(&row, &[0], false, cx);
        wrap_at_20(&view, cx);
        let n = cjk_chars_in_20_columns(cx);
        assert!(n > 10, "{n}");
        let mut starts = vec![0];
        let mut top = bounds(&view, 0..0, cx).origin.y;
        for (at, _) in row.char_indices() {
            let y = bounds(&view, at..at, cx).origin.y;
            if y > top {
                starts.push(at);
                top = y;
            }
        }
        assert_eq!(starts, [0, 3 * n, 6 * n]);
    });
}

/// Down and Up keep the caret's visible column, a wide char counting two.
/// From column 6 of an ASCII row, Down lands after the third CJK char
/// (visible column 6, byte 20), as in VS Code; Down again is back at column
/// 6 of the last row, and Up Up returns to the start.
#[gpui::test]
fn cjk_up_down_keep_the_visible_column(cx: &mut TestAppContext) {
    let text = "abcdefghij\n一二三四五六七八\nabcdefghij";
    let (view, cx) = editor(text, &[6], false, cx);
    keys("down", cx);
    assert_eq!(head(&view, cx), 11 + 3 * 3);
    keys("down", cx);
    assert_eq!(head(&view, cx), 36 + 6);
    keys("up up", cx);
    assert_eq!(head(&view, cx), 6);
}

/// Limitation G7: keyboard column select counts chars, not visible
/// columns: two columns right of `a` on `ab一二cd` take `b一`, and the row
/// below takes `bc`, a narrower box; VS Code takes `bcd` there (a wide
/// char is two visible columns). The keys are bound on macOS only.
#[cfg(target_os = "macos")]
#[gpui::test]
fn cjk_keyboard_column_select_counts_chars(cx: &mut TestAppContext) {
    let text = "ab一二cd\nabcdefgh";
    let (view, cx) = editor(text, &[1], false, cx);
    keys(
        "cmd-shift-alt-right cmd-shift-alt-right cmd-shift-alt-down",
        cx,
    );
    let below = text.find('\n').unwrap() + 1;
    assert_eq!(selections(&view, cx), [1..5, below + 1..below + 3]);
}

/// Limitation G7: an alt-shift-drag box is chars wide on every row, so
/// over a row with wide chars it isn't a rectangle on screen. Dragged
/// from column 1 of `abcdefgh` down to the x of its column 5, the head's
/// column is read on `ab一二cd` as char column 4: the box takes `bcd` above
/// and `b一二` below, whose right edges are a third of a column apart.
#[cfg(target_os = "macos")]
#[test]
fn cjk_alt_shift_drag_selects_a_box_of_chars() {
    shaped::with_app("cjk_alt_shift_drag_selects_a_box_of_chars", |cx| {
        let text = "abcdefgh\nab一二cd";
        let below = text.find('\n').unwrap() + 1;
        let (view, cx) = editor(text, &[0], false, cx);
        let from = on_row(&view, 0, caret_x(&view, 1, cx), cx);
        let to = on_row(&view, below, caret_x(&view, 5, cx), cx);
        let box_keys = Modifiers {
            alt: true,
            shift: true,
            ..Modifiers::default()
        };
        cx.simulate_event(MouseDownEvent {
            position: from,
            button: MouseButton::Left,
            modifiers: box_keys,
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_mouse_move(to, Some(MouseButton::Left), box_keys);
        cx.simulate_event(MouseUpEvent {
            position: to,
            button: MouseButton::Left,
            modifiers: box_keys,
            click_count: 1,
        });
        cx.run_until_parked();
        draw(cx);
        assert_eq!(selections(&view, cx), [1..4, below + 1..below + 8]);
        let (above, under) = (caret_x(&view, 4, cx), caret_x(&view, below + 8, cx));
        assert!(under - above > 2., "{above} {under}");
    });
}

/// Vim's `gj` and `gk` move by display row over a soft-wrapped row of
/// wide chars, keeping the caret's place along it. The offsets depend on
/// how many chars wrap to a row (limitation G8: 11 on macOS 27, 12 on
/// macOS 15), so they follow the measured count `n`.
#[cfg(target_os = "macos")]
#[test]
fn cjk_vim_gj_gk_move_by_display_row_over_wrapped_wide_text() {
    shaped::with_app(
        "cjk_vim_gj_gk_move_by_display_row_over_wrapped_wide_text",
        |cx| {
            let text = format!("{}\nabc", "一二三四五六七八九十".repeat(3));
            let (view, cx) = editor(&text, &[0], true, cx);
            wrap_at_20(&view, cx);
            // n chars, 3n bytes, to a display row (G8).
            let n = cjk_chars_in_20_columns(cx);
            assert!(n > 10, "{n}");
            let mut heads = Vec::new();
            for typed in ["l l l", "g j", "g j", "g k", "g k"] {
                keys(typed, cx);
                heads.push(head(&view, cx));
            }
            assert_eq!(heads, [9, 3 * n + 9, 6 * n + 9, 3 * n + 9, 9]);
        },
    );
}

/// Vim's `|` goes to a display column through the view: on `一二三abc`,
/// column 4 is the second half of `二`, so the caret goes to `二`, and
/// column 7 is `a`.
#[gpui::test]
fn cjk_vim_bar_goes_to_the_display_column(cx: &mut TestAppContext) {
    let (view, cx) = editor("一二三abc", &[0], true, cx);
    keys("4 |", cx);
    assert_eq!(head(&view, cx), 3);
    keys("7 |", cx);
    assert_eq!(head(&view, cx), 9);
}

/// Limitation G8: a vim visual block over wide chars is drawn ragged.
/// From `a` four columns right and down, the block is display columns 1
/// to 4 on both rows, `abcd` and `一二`, but a wide char is drawn narrower
/// than two columns, so their right edges are two thirds of a column apart
/// on screen, where nvim's cells line up.
#[cfg(target_os = "macos")]
#[test]
fn cjk_vim_visual_block_edges_over_wide_chars_do_not_line_up() {
    shaped::with_app(
        "cjk_vim_visual_block_edges_over_wide_chars_do_not_line_up",
        |cx| {
            let text = "abcd\n一二三";
            let (view, cx) = editor(text, &[0], true, cx);
            keys("ctrl-v l l l j", cx);
            assert_eq!(selections(&view, cx), [0..4, 5..11]);
            let (narrow, wide) = (caret_x(&view, 4, cx), caret_x(&view, 11, cx));
            assert!(narrow - wide > 4., "{narrow} {wide}");
        },
    );
}

// Grapheme clusters.

/// Right and Left step over each cluster whole: a ZWJ family, a flag, a
/// keycap, `e` with a combining accent, Hangul jamo and a Devanagari
/// conjunct each take one step.
#[gpui::test]
fn grapheme_left_right_step_over_whole_clusters(cx: &mut TestAppContext) {
    let (view, cx) = editor(CLUSTERS, &[0], false, cx);
    let mut rights = Vec::new();
    for _ in CLUSTER_ENDS {
        keys("right", cx);
        rights.push(head(&view, cx));
    }
    assert_eq!(rights, CLUSTER_ENDS);
    let mut lefts = Vec::new();
    for _ in CLUSTER_ENDS {
        keys("left", cx);
        lefts.push(head(&view, cx));
    }
    let mut back = CLUSTER_ENDS[..CLUSTER_ENDS.len() - 1].to_vec();
    back.reverse();
    back.push(0);
    assert_eq!(lefts, back);
}

/// Backspace after a cluster and Delete before it each take the whole
/// cluster, combining sequences too. VS Code takes only the accent of
/// `e` and a combining acute on Backspace: the difference is G9.
#[gpui::test]
fn grapheme_backspace_and_delete_remove_whole_clusters(cx: &mut TestAppContext) {
    let (view, cx) = editor(CLUSTERS, &[0], false, cx);
    let mut start = 0;
    for end in CLUSTER_ENDS {
        let without = format!("{}{}", &CLUSTERS[..start], &CLUSTERS[end..]);
        let cluster = &CLUSTERS[start..end];
        set_caret(&view, end, cx);
        keys("backspace", cx);
        assert_eq!(text(&view, cx), without, "backspace after {cluster:?}");
        keys("cmd-z", cx);
        set_caret(&view, start, cx);
        keys("delete", cx);
        assert_eq!(text(&view, cx), without, "delete before {cluster:?}");
        keys("cmd-z", cx);
        assert_eq!(text(&view, cx), CLUSTERS);
        start = end;
    }
}

/// A click inside a ZWJ family, painted as one emoji glyph, puts the caret
/// at its start or end, whichever is nearer, never between its chars, and
/// a double-click selects it whole. Its caret advance is that glyph's.
#[cfg(target_os = "macos")]
#[test]
fn grapheme_clicks_inside_a_cluster_land_on_its_edges() {
    shaped::with_app("grapheme_clicks_inside_a_cluster_land_on_its_edges", |cx| {
        let (view, cx) = editor(CLUSTERS, &[0], false, cx);
        let (start, end) = (CLUSTER_ENDS[0], CLUSTER_ENDS[1]);
        let (left, right) = (caret_x(&view, start, cx), caret_x(&view, end, cx));
        let glyphs = glyphs(CLUSTERS, cx);
        let family = paint_order(&glyphs, start);
        assert_eq!(glyphs[family + 1].0, end, "one glyph: {glyphs:?}");
        assert_eq!(right - left, glyphs[family + 1].1 - glyphs[family].1);
        let mut hits = Vec::new();
        for part in [0.1, 0.3, 0.45, 0.55, 0.9] {
            let at = on_row(&view, 0, left + (right - left) * part, cx);
            hits.push(hit(&view, at, cx));
        }
        assert_eq!(hits, [start, start, start, end, end]);
        let at = on_row(&view, 0, left + (right - left) * 0.3, cx);
        click(at, 1, Modifiers::none(), cx);
        assert_eq!(selection(&view, cx), start..start);
        click(at, 2, Modifiers::none(), cx);
        assert_eq!(selection(&view, cx), start..end);
    });
}

/// With vim on, `l` steps over clusters, and `x`, `r` and `~` act on the
/// whole cluster under the caret, as nvim does: `x` on `e` with a
/// combining acute takes both, `r` replaces both, `~` capitalises the
/// base and keeps the accent, and `x` on a ZWJ family takes all of it.
#[gpui::test]
fn grapheme_vim_keys_act_on_whole_clusters(cx: &mut TestAppContext) {
    let start = "ae\u{301}b👨\u{200d}👩\u{200d}👧c🇨🇦d";
    let (view, cx) = editor(start, &[0], true, cx);
    let mut heads = Vec::new();
    for _ in 0..6 {
        keys("l", cx);
        heads.push(head(&view, cx));
    }
    assert_eq!(heads, [1, 4, 5, 23, 24, 32]);
    keys("0 l x", cx);
    assert_eq!(text(&view, cx), "ab👨\u{200d}👩\u{200d}👧c🇨🇦d");
    keys("u 0 l r z", cx);
    assert_eq!(text(&view, cx), "azb👨\u{200d}👩\u{200d}👧c🇨🇦d");
    keys("u 0 l ~", cx);
    assert_eq!(text(&view, cx), "aE\u{301}b👨\u{200d}👩\u{200d}👧c🇨🇦d");
    assert_eq!(head(&view, cx), 4);
    keys("u 0 l l l x", cx);
    assert_eq!(text(&view, cx), "ae\u{301}bc🇨🇦d");
}

/// Down lands only on grapheme boundaries. From column 1 of `abcdef`, Down
/// lands after `e` and its combining acute (byte 10); from column 3, two
/// Downs land at byte 12 and then after the family (byte 33, one cluster
/// two columns wide), never inside a cluster.
#[gpui::test]
fn grapheme_up_down_land_on_cluster_boundaries(cx: &mut TestAppContext) {
    let text = "abcdef\ne\u{301}xyz\n👨\u{200d}👩\u{200d}👧xyz";
    let (view, cx) = editor(text, &[1], false, cx);
    keys("down", cx);
    assert_eq!(head(&view, cx), 10);
    set_caret(&view, 3, cx);
    keys("down down", cx);
    assert_eq!(head(&view, cx), 33);
}

// Input methods: what ime_tests.rs and the vim tests leave.

/// A composition in vim's visual-block insert (`<C-v>jjI`) stays out of
/// the buffer, its commit types on the first row, and `<Esc>` repeats it
/// on every row of the block, as typed text is.
#[gpui::test]
fn ime_commit_in_vim_block_insert_repeats_on_every_row(cx: &mut TestAppContext) {
    let (view, cx) = editor("abcd\nefgh\nijkl", &[0], true, cx);
    keys("ctrl-v j j shift-i", cx);
    input(&view, cx, |view, window, cx| {
        view.replace_and_mark_text_in_range(None, "にほ", None, window, cx)
    });
    draw(cx);
    assert_eq!(text(&view, cx), "abcd\nefgh\nijkl");
    input(&view, cx, |view, window, cx| {
        view.replace_text_in_range(None, "日本", window, cx)
    });
    draw(cx);
    assert_eq!(text(&view, cx), "日本abcd\nefgh\nijkl");
    keys("escape", cx);
    assert_eq!(text(&view, cx), "日本abcd\n日本efgh\n日本ijkl");
    keys("u", cx);
    assert_eq!(text(&view, cx), "abcd\nefgh\nijkl");
}

/// In vim's replace mode (`R`) over wide chars, a composition leaves the
/// buffer alone until it commits, then overwrites one grapheme per char
/// it commits, and `<BS>` puts them back.
#[gpui::test]
fn ime_composition_in_replace_mode_overwrites_wide_chars_on_commit(cx: &mut TestAppContext) {
    let (view, cx) = editor("一二三四", &[0], true, cx);
    keys("shift-r", cx);
    input(&view, cx, |view, window, cx| {
        view.replace_and_mark_text_in_range(None, "にほ", None, window, cx)
    });
    draw(cx);
    assert_eq!(text(&view, cx), "一二三四");
    input(&view, cx, |view, window, cx| {
        view.replace_text_in_range(None, "日本", window, cx)
    });
    draw(cx);
    assert_eq!(text(&view, cx), "日本三四");
    keys("backspace backspace", cx);
    assert_eq!(text(&view, cx), "一二三四");
}

/// With carets on rows of different widths, one after wide chars, the
/// input method's window goes at the primary caret, past the wide chars,
/// as wide as the composition is drawn; the commit types once at every
/// caret.
#[cfg(target_os = "macos")]
#[test]
fn ime_composition_at_carets_on_rows_of_different_widths() {
    shaped::with_app(
        "ime_composition_at_carets_on_rows_of_different_widths",
        |cx| {
            let start = "一二三\nab";
            let (view, cx) = editor(start, &[start.len(), 9], false, cx);
            let primary = head(&view, cx);
            let before = caret_x(&view, primary, cx);
            input(&view, cx, |view, window, cx| {
                view.replace_and_mark_text_in_range(None, "にほ", None, window, cx)
            });
            draw(cx);
            let marked = input(&view, cx, |view, window, cx| {
                view.marked_text_range(window, cx)
            })
            .expect("composing");
            let drawn = input(&view, cx, |view, window, cx| {
                view.bounds_for_range(marked, Bounds::default(), window, cx)
            })
            .expect("drawn");
            assert_eq!(f32::from(drawn.origin.x), before);
            let glyphs = glyphs("にほ", cx);
            let second = glyphs[paint_order(&glyphs, "に".len())].1;
            assert!(f32::from(drawn.size.width) > second, "{drawn:?}");
            input(&view, cx, |view, window, cx| {
                view.replace_text_in_range(None, "日本", window, cx)
            });
            assert_eq!(text(&view, cx), "一二三日本\nab日本");
        },
    );
}

// Font fallback.

/// The editor font (JetBrains Mono, or the first installed of kiln's
/// preferred list) has no CJK or emoji, so CoreText draws `一` and `😀`
/// from fallback fonts, each its own, and the editor places carets by
/// their shaped advances: as they're drawn.
#[cfg(target_os = "macos")]
#[test]
fn fallback_cjk_and_emoji_draw_from_other_fonts_and_measure_as_drawn() {
    shaped::with_app(
        "fallback_cjk_and_emoji_draw_from_other_fonts_and_measure_as_drawn",
        |cx| {
            let row = "a一😀b";
            let (view, cx) = editor(row, &[0], false, cx);
            let glyphs = glyphs(row, cx);
            let font = |index: usize| glyphs[paint_order(&glyphs, index)].2;
            let (latin, cjk, emoji) = (font(0), font(1), font(4));
            assert_ne!(cjk, latin);
            assert_ne!(emoji, latin);
            assert_ne!(emoji, cjk);
            assert_eq!(font(8), latin);
            let origin = caret_x(&view, 0, cx);
            for (index, x, _) in glyphs {
                let caret = caret_x(&view, index, cx) - origin;
                assert!((caret - x).abs() < 0.01, "{index}: {caret} {x}");
            }
        },
    );
}

/// Limitation G10: the editor names no fallback fonts. Its font carries
/// no fallback list, and `[editor]` has no `font_fallbacks` setting as
/// `[terminal]` has, so which font draws a char the editor font lacks is
/// CoreText's choice alone.
#[gpui::test]
fn fallback_the_editor_font_names_no_fallbacks(cx: &mut TestAppContext) {
    let font = cx.update(|cx| EditorTheme::default_dark(cx).font());
    assert_eq!(font.fallbacks, None);
    let settings = serde_json::to_value(fe_config::settings::Settings::default()).unwrap();
    let has = |table: &str| settings[table].get("font_fallbacks").is_some();
    assert!(has("terminal"), "{settings}");
    assert!(!has("editor"), "{settings}");
}
