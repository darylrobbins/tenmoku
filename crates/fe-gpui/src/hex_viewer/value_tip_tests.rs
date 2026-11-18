//! The byte inspector's value tooltip, drawn by hovering a cell of the
//! strip in a headless window: a long value's tooltip lies wholly inside
//! the window, ordinary, narrow or short down to the smallest window whose
//! strip still shows a cell to hover, its lines fit across it and, when
//! they're taller than the window allows, it scrolls to the last one.
//! [`TipFit`] is checked on its own below that, where the strip has gone.

use std::time::Duration;

use gpui::{
    Bounds, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent, Size, TestAppContext,
    VisualTestContext, point, px, size,
};

use super::tests::{draw, file_of, viewer};
use super::*;

/// The window sizes the tooltip is drawn in.
const ORDINARY: Size<Pixels> = size(px(1000.), px(600.));
const NARROW: Size<Pixels> = size(px(200.), px(600.));
const SHORT: Size<Pixels> = size(px(1000.), px(80.));
const NARROW_AND_SHORT: Size<Pixels> = size(px(220.), px(260.));
/// Too narrow for the margins.
const NARROWER: Size<Pixels> = size(px(30.), px(600.));
/// Too narrow for the margins, padding and border: the strip's cells are
/// 4 px wide here, and gone 4 px narrower.
const NARROWEST: Size<Pixels> = size(px(20.), px(600.));
/// Too short for the margins: the strip's first row shows 2 px of its cells.
const SHORTEST: Size<Pixels> = size(px(1000.), px(34.));

/// A file whose first bytes are `value`'s, little-endian, then zeros.
fn file_starting(value: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
    let mut bytes = value.to_vec();
    bytes.resize(64, 0);
    file_of(&bytes)
}

/// The inspector's text for `name`, as its tooltip shows it whole.
fn full_text(viewer: &Entity<HexViewer>, name: &str, cx: &mut VisualTestContext) -> String {
    viewer.update(cx, |viewer, _| {
        let (_, value) = (viewer.inspection().into_iter())
            .find(|(n, _)| *n == name)
            .unwrap();
        format!("{name} {}", value.unwrap())
    })
}

fn inside(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    let slack = px(0.5);
    inner.left() >= outer.left() - slack
        && inner.right() <= outer.right() + slack
        && inner.top() >= outer.top() - slack
        && inner.bottom() <= outer.bottom() + slack
}

/// Hovers `name`'s cell for a file starting with `value` in a window of
/// `window`, and checks its tooltip: inside the window, every char of the
/// value in its lines, each line whole across, and every line reachable,
/// scrolling there when the text is taller than the tooltip. Says whether
/// it scrolled.
fn check_tip(
    cx: &mut TestAppContext,
    value: &[u8],
    name: &'static str,
    window: Size<Pixels>,
) -> bool {
    let (_dir, path) = file_starting(value);
    let (viewer, cx, _) = viewer(cx, &path);
    cx.simulate_resize(window);
    draw(cx);
    cx.run_until_parked();
    draw(cx);
    let full = full_text(&viewer, name, cx);
    assert!(full.len() > 40, "a long value: {full}");
    let probe = viewer.read_with(cx, |viewer, _| viewer.tip_probe.clone());
    let viewport = Bounds::new(point(px(0.), px(0.)), window);
    assert_eq!(cx.update(|window, _| window.viewport_size()), window);

    let selector = if name == "f32" {
        "hex-inspect-f32"
    } else {
        "hex-inspect-f64"
    };
    let cell = cx.debug_bounds(selector).unwrap();
    let shown = cell.intersect(&viewport);
    assert!(
        shown.size.width > px(0.) && shown.size.height > px(0.),
        "{name} cell {cell:?} in {window:?}"
    );
    cx.simulate_mouse_move(shown.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(500));
    draw(cx);
    let tip = cx
        .debug_bounds("hex-value-tip")
        .unwrap_or_else(|| panic!("{name}'s tooltip shows in {window:?}"));
    assert!(probe.borrow().drawn > 0);
    assert!(inside(tip, viewport), "tooltip {tip:?} in {window:?}");
    let text = cx.debug_bounds("hex-value-tip-text").unwrap();
    assert!(inside(text, tip), "{text:?} in {tip:?}");
    assert!(
        text.size.width > px(0.) && text.size.height > px(0.),
        "room to read: {text:?}"
    );

    // Every char is in its lines, in order, and every line fits across, or
    // scrolls across in a window narrower than a glyph.
    let lines = probe.borrow().lines.clone();
    assert_eq!(lines.concat(), full, "nothing is cut");
    if window.width < ORDINARY.width {
        assert!(lines.len() > 1, "it wraps: {lines:?}");
    }
    let drawn = probe.borrow().line_bounds.clone();
    assert_eq!(drawn.len(), lines.len());
    let scroll = probe.borrow().scroll.clone().unwrap();
    let most = scroll.max_offset();
    for (line, at) in lines.iter().zip(&drawn) {
        assert!(
            at.left() >= text.left() - px(0.5)
                && at.size.width <= text.size.width + most.x + px(0.5),
            "{line:?} {at:?} across {text:?} in {window:?}"
        );
    }

    if most == point(px(0.), px(0.)) {
        for (line, at) in lines.iter().zip(&drawn) {
            assert!(inside(*at, text), "{line:?} {at:?} in {text:?}");
        }
        return false;
    }
    // More than the window allows: the first line starts in view, and the
    // mouse goes in and scrolls to the end, where the last line and the
    // widest's end are in view, the tooltip staying where it is.
    let first = drawn[0];
    assert!(
        first.left() >= text.left() - px(0.5) && first.top() >= text.top() - px(0.5),
        "{first:?} in {text:?}"
    );
    let extent = drawn.last().unwrap().bottom() - first.top();
    let wide = drawn
        .iter()
        .map(|at| at.size.width)
        .fold(px(0.), Pixels::max);
    assert!(
        extent > text.size.height || wide > text.size.width,
        "{extent:?} {wide:?} vs {text:?}"
    );
    cx.simulate_mouse_move(text.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(1000));
    draw(cx);
    assert_eq!(cx.debug_bounds("hex-value-tip"), Some(tip), "still shown");
    // GPUI takes one axis a wheel event, so down, then across.
    for delta in [
        point(px(0.), -(extent + px(100.))),
        point(-(wide + px(100.)), px(0.)),
    ] {
        cx.simulate_event(ScrollWheelEvent {
            position: text.center(),
            delta: ScrollDelta::Pixels(delta),
            ..Default::default()
        });
    }
    draw(cx);
    assert_eq!(scroll.offset(), -most, "scrolled to the end");
    assert_eq!(cx.debug_bounds("hex-value-tip"), Some(tip), "where it was");
    let drawn = probe.borrow().line_bounds.clone();
    let last = *drawn.last().unwrap();
    assert!(
        last.bottom() <= text.bottom() + px(0.5),
        "last line {last:?} in {text:?}"
    );
    let right = drawn
        .iter()
        .map(|at| at.right())
        .fold(px(f32::MIN), Pixels::max);
    assert!(right <= text.right() + px(0.5), "{right:?} in {text:?}");
    true
}

fn long_f64() -> [u8; 8] {
    let value = f64::MIN_POSITIVE;
    assert!(value.to_string().len() > 300);
    value.to_le_bytes()
}

fn long_f32() -> [u8; 4] {
    let value = f32::MIN_POSITIVE;
    assert!(value.to_string().len() > 40);
    value.to_le_bytes()
}

#[gpui::test]
fn a_long_f64_tip_in_an_ordinary_window(cx: &mut TestAppContext) {
    assert!(!check_tip(cx, &long_f64(), "f64", ORDINARY));
}

#[gpui::test]
fn a_long_f64_tip_in_a_narrow_window(cx: &mut TestAppContext) {
    check_tip(cx, &long_f64(), "f64", NARROW);
}

#[gpui::test]
fn a_long_f64_tip_in_a_short_window_scrolls(cx: &mut TestAppContext) {
    assert!(check_tip(cx, &long_f64(), "f64", SHORT));
}

#[gpui::test]
fn a_long_f64_tip_in_a_narrow_short_window_scrolls(cx: &mut TestAppContext) {
    assert!(check_tip(cx, &long_f64(), "f64", NARROW_AND_SHORT));
}

#[gpui::test]
fn a_long_f32_tip_in_an_ordinary_window(cx: &mut TestAppContext) {
    assert!(!check_tip(cx, &long_f32(), "f32", ORDINARY));
}

#[gpui::test]
fn a_long_f32_tip_in_a_narrow_window(cx: &mut TestAppContext) {
    check_tip(cx, &long_f32(), "f32", NARROW);
}

#[gpui::test]
fn a_long_f32_tip_in_a_short_window(cx: &mut TestAppContext) {
    check_tip(cx, &long_f32(), "f32", SHORT);
}

#[gpui::test]
fn a_long_f32_tip_in_a_narrow_short_window(cx: &mut TestAppContext) {
    check_tip(cx, &long_f32(), "f32", NARROW_AND_SHORT);
}

#[gpui::test]
fn a_long_f64_tip_in_a_window_too_narrow_for_margins(cx: &mut TestAppContext) {
    check_tip(cx, &long_f64(), "f64", NARROWER);
}

#[gpui::test]
fn a_long_f32_tip_in_a_window_too_narrow_for_margins(cx: &mut TestAppContext) {
    check_tip(cx, &long_f32(), "f32", NARROWER);
}

#[gpui::test]
fn a_long_f64_tip_in_the_narrowest_window(cx: &mut TestAppContext) {
    check_tip(cx, &long_f64(), "f64", NARROWEST);
}

#[gpui::test]
fn a_long_f32_tip_in_the_narrowest_window(cx: &mut TestAppContext) {
    check_tip(cx, &long_f32(), "f32", NARROWEST);
}

#[gpui::test]
fn a_long_f64_tip_in_the_shortest_window_scrolls(cx: &mut TestAppContext) {
    assert!(check_tip(cx, &long_f64(), "f64", SHORTEST));
}

#[gpui::test]
fn a_long_f32_tip_in_the_shortest_window(cx: &mut TestAppContext) {
    check_tip(cx, &long_f32(), "f32", SHORTEST);
}

/// The text's widest glyph and its line height in the tests' text system.
const GLYPH: Pixels = px(7.2);
const LINE: Pixels = px(19.);

#[test]
fn the_margins_then_the_padding_give_way_in_a_small_window() {
    let fit = |w: f32, h: f32| TipFit::new(size(px(w), px(h)), GLYPH, LINE);
    // Room for everything: 8 px margins, padding and a border.
    let roomy = fit(1000., 600.);
    assert_eq!(roomy.margin, size(TIP_MARGIN, TIP_MARGIN));
    assert_eq!(roomy.pad, size(TIP_PAD_X, TIP_PAD_Y));
    assert_eq!(roomy.border, TIP_BORDER);
    assert_eq!(roomy.cap, size(px(984.), px(584.)));
    assert_eq!(roomy.room, px(984. - 18. - 1.));
    // Margins, padding, border and a glyph need 42.2 px across, a line 45
    // down.
    let edge = fit(42.2, 45.);
    assert_eq!(edge.margin, size(TIP_MARGIN, TIP_MARGIN));
    assert!(edge.room >= GLYPH, "{edge:?}");
    // Short of that, the margins go, and the glyph still fits.
    let no_margin = fit(42., 44.);
    assert_eq!(no_margin.margin, size(px(0.), px(0.)));
    assert_eq!(no_margin.pad, size(TIP_PAD_X, TIP_PAD_Y));
    assert_eq!(no_margin.cap, size(px(42.), px(44.)));
    assert!(no_margin.room >= GLYPH, "{no_margin:?}");
    let down = no_margin.cap.height - (no_margin.pad.height + no_margin.border) * 2.;
    assert!(down >= LINE, "a line down: {down:?}");
    // Short of a glyph and the padding, the padding and border go too.
    for (w, h) in [(26., 28.), (8.2, 19.)] {
        let bare = fit(w, h);
        assert_eq!(bare.margin, size(px(0.), px(0.)), "{w}x{h}");
        assert_eq!(bare.pad, size(px(0.), px(0.)), "{w}x{h}");
        assert_eq!(bare.border, px(0.), "{w}x{h}");
        assert_eq!(bare.cap, size(px(w), px(h)), "the whole window");
        assert!(bare.room >= GLYPH, "{bare:?}");
    }
}

#[test]
fn a_window_smaller_than_a_glyph_still_holds_the_box() {
    // The text scrolls both ways inside it; the box takes the window.
    for (w, h) in [(5., 10.), (1., 1.), (0., 0.)] {
        let fit = TipFit::new(size(px(w), px(h)), GLYPH, LINE);
        assert_eq!(fit.cap, size(px(w), px(h)));
        assert_eq!(fit.room, px((w - 1f32).max(0.)));
        assert_eq!(fit.width(GLYPH), px(w), "capped to the window");
        // A line a glyph wide each, nothing dropped.
        let text = "f64 2.2e-308";
        let lines = value_tip_lines(text, fit.room, |ix| GLYPH * ix as f32);
        assert_eq!(lines.len(), text.len());
        assert_eq!(
            lines.iter().map(|(l, _)| l.as_ref()).collect::<String>(),
            text
        );
    }
}
