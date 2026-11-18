//! The byte inspector: its values, the strip under the rows, its commands
//! and its saved state.

use fe_command::Value;
use gpui::{Modifiers, MouseButton, Pixels, TestAppContext, px};
use kiln_shell::{ItemHandle, PaneItem as _};

use super::inspector::{Endian, inspect};
use super::tests::{counting, draw, file_of, run, viewer};
use super::*;

fn shown(
    viewer: &Entity<HexViewer>,
    cx: &mut gpui::VisualTestContext,
) -> Vec<(&'static str, String)> {
    viewer.update(cx, |viewer, _| {
        viewer
            .inspection()
            .into_iter()
            .map(|(name, value)| (name, value.unwrap_or_else(|| "—".into())))
            .collect()
    })
}

fn value<'a>(rows: &'a [(&'static str, String)], name: &str) -> &'a str {
    &rows.iter().find(|(n, _)| *n == name).unwrap().1
}

#[test]
fn reads_each_type_little_and_big_endian() {
    let bytes = [0xFE, 0xFF, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04];
    let le = inspect(&bytes, Endian::Little);
    assert_eq!(le.u8, Some(0xFE));
    assert_eq!(le.i8, Some(-2));
    assert_eq!(le.u16, Some(0xFFFE));
    assert_eq!(le.i16, Some(-2));
    assert_eq!(le.u32, Some(0x0000_FFFE));
    assert_eq!(le.i32, Some(0xFFFE));
    assert_eq!(le.u64, Some(0x0403_0201_0000_FFFE));
    assert_eq!(le.i64, Some(0x0403_0201_0000_FFFE));
    assert_eq!(le.f32, Some(f32::from_bits(0x0000_FFFE)));
    assert_eq!(le.f64, Some(f64::from_bits(0x0403_0201_0000_FFFE)));
    let be = inspect(&bytes, Endian::Big);
    assert_eq!(be.u8, Some(0xFE));
    assert_eq!(be.u16, Some(0xFEFF));
    assert_eq!(be.i16, Some(-257));
    assert_eq!(be.u32, Some(0xFEFF_0000));
    assert_eq!(be.i32, Some(-0x0101_0000));
    assert_eq!(be.u64, Some(0xFEFF_0000_0102_0304));
    assert_eq!(be.f32, Some(f32::from_bits(0xFEFF_0000)));
    assert_eq!(be.f64, Some(f64::from_bits(0xFEFF_0000_0102_0304)));
    assert_eq!(
        inspect(&[0x3F, 0x80, 0, 0], Endian::Big).f32,
        Some(1.0),
        "1.0f32"
    );
}

#[test]
fn a_short_tail_leaves_wide_types_empty() {
    let three = inspect(&[1, 2, 3], Endian::Little);
    assert_eq!((three.u8, three.u16), (Some(1), Some(0x0201)));
    assert_eq!((three.u32, three.i32, three.f32), (None, None, None));
    assert_eq!((three.u64, three.i64, three.f64), (None, None, None));
    assert_eq!(inspect(&[], Endian::Big), Default::default());
    assert_eq!(inspect(&[], Endian::Big).u8, None);
}

#[test]
fn floats_show_nan_infinity_and_negative_zero() {
    let text = |bytes: &[u8], name: &str| {
        let rows = inspect(bytes, Endian::Big).rows();
        rows.into_iter()
            .find(|(n, _)| *n == name)
            .unwrap()
            .1
            .unwrap()
    };
    assert_eq!(text(&f32::NAN.to_be_bytes(), "f32"), "NaN");
    assert_eq!(text(&f32::INFINITY.to_be_bytes(), "f32"), "inf");
    assert_eq!(text(&f32::NEG_INFINITY.to_be_bytes(), "f32"), "-inf");
    assert_eq!(text(&(-0.0f32).to_be_bytes(), "f32"), "-0");
    assert_eq!(text(&f64::NAN.to_be_bytes(), "f64"), "NaN");
    assert_eq!(text(&f64::NEG_INFINITY.to_be_bytes(), "f64"), "-inf");
    assert_eq!(text(&(-0.0f64).to_be_bytes(), "f64"), "-0");
    assert_eq!(text(&0.1f64.to_be_bytes(), "f64"), "0.1");
    assert_eq!(text(&1.5f32.to_be_bytes(), "f32"), "1.5");
}

#[gpui::test]
fn strip_follows_the_selection_start(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&counting(64));
    let (viewer, cx, _) = viewer(cx, &path);
    assert_eq!(value(&shown(&viewer, cx), "u8"), "0");
    assert_eq!(value(&shown(&viewer, cx), "u16"), "256");
    // A selection reads from its start, however it was dragged.
    viewer.update(cx, |viewer, cx| {
        viewer.select(10, 5);
        cx.notify();
    });
    draw(cx);
    let rows = shown(&viewer, cx);
    assert_eq!(value(&rows, "u8"), "5");
    assert_eq!(value(&rows, "u16"), (5 + 6 * 256).to_string());
    // The last byte: wider types have too few bytes.
    viewer.update(cx, |viewer, cx| {
        viewer.select(63, 63);
        cx.notify();
    });
    let rows = shown(&viewer, cx);
    assert_eq!(value(&rows, "u8"), "63");
    assert_eq!(value(&rows, "u16"), "—");
    assert_eq!(value(&rows, "f64"), "—");
}

#[gpui::test]
fn an_empty_file_has_no_values(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&[]);
    let (viewer, cx, _) = viewer(cx, &path);
    assert!(shown(&viewer, cx).iter().all(|(_, text)| text == "—"));
}

#[gpui::test]
fn endian_toggle_flips_the_values(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let (viewer, cx, _) = viewer(cx, &path);
    assert_eq!(value(&shown(&viewer, cx), "u16"), "513");
    run(cx, "hex.toggle_endian");
    assert_eq!(value(&shown(&viewer, cx), "u16"), "258");
    // The button does the same.
    draw(cx);
    let button = cx.debug_bounds("hex-viewer-endian").expect("the button");
    cx.simulate_click(button.center(), Modifiers::none());
    assert_eq!(value(&shown(&viewer, cx), "u16"), "513");
}

#[gpui::test]
fn the_strip_reads_once_per_selection_change(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&counting(256));
    let (viewer, cx, _) = viewer(cx, &path);
    let reads = |cx: &mut gpui::VisualTestContext| viewer.read_with(cx, |v, _| v.inspect_reads);
    draw(cx);
    let first = reads(cx);
    assert_eq!(first, 1);
    for _ in 0..5 {
        viewer.update(cx, |_, cx| cx.notify());
        draw(cx);
    }
    run(cx, "hex.toggle_endian");
    draw(cx);
    assert_eq!(reads(cx), first, "frames and byte order don't read");
    viewer.update(cx, |viewer, cx| {
        viewer.select(40, 40);
        cx.notify();
    });
    draw(cx);
    assert_eq!(reads(cx), first + 1);
}

#[gpui::test]
fn hiding_the_strip_gives_rows_back(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&counting(0x4000));
    let (viewer, cx, _) = viewer(cx, &path);
    assert!(cx.debug_bounds("hex-viewer-inspector").is_some());
    let height = |cx: &mut gpui::VisualTestContext| {
        viewer.read_with(cx, |v, _| v.rows_bounds().unwrap().size.height)
    };
    let with = height(cx);
    let strip = cx.debug_bounds("hex-viewer-inspector").unwrap().size.height;
    run(cx, "hex.toggle_inspector");
    draw(cx);
    assert!(cx.debug_bounds("hex-viewer-inspector").is_none());
    assert_eq!(height(cx), with + strip);
    run(cx, "hex.toggle_inspector");
    draw(cx);
    assert_eq!(height(cx), with);
}

#[gpui::test]
fn inspector_state_survives_a_saved_layout(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&counting(64));
    let (viewer, cx, _) = viewer(cx, &path);
    run(cx, "hex.toggle_endian");
    run(cx, "hex.toggle_inspector");
    let item: Box<dyn ItemHandle> = Box::new(viewer.clone());
    let state = cx.update(|_, cx| item.serialize(cx)).expect("saved");
    let task = cx.update(|window, cx| HexViewer::restore(state, window, cx));
    let restored = std::rc::Rc::new(std::cell::RefCell::new(None));
    cx.update(|_, cx| {
        let restored = restored.clone();
        cx.spawn(async move |_| *restored.borrow_mut() = Some(task.await))
            .detach();
    });
    cx.run_until_parked();
    let restored = restored.take().unwrap().expect("restored");
    restored.read_with(cx, |v, _| {
        assert!(!v.inspector_shown);
        assert_eq!(v.endian, Endian::Big);
    });
}

#[gpui::test]
fn an_older_saved_state_restores_the_defaults(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&counting(64));
    let (viewer, cx, _) = viewer(cx, &path);
    run(cx, "hex.toggle_endian");
    run(cx, "hex.toggle_inspector");
    let old = Value::from_iter([("head", Value::from(3u64))]);
    viewer.update(cx, |viewer, cx| {
        let window = cx.windows().first().copied();
        let _ = window;
        super::pane_item::place(viewer, &old, cx);
    });
    viewer.read_with(cx, |v, _| {
        assert!(v.inspector_shown);
        assert_eq!(v.endian, Endian::Little);
    });
    let _ = MouseButton::Left;
}

fn strip_height(cx: &mut gpui::VisualTestContext) -> Pixels {
    cx.debug_bounds("hex-viewer-inspector")
        .expect("the strip")
        .size
        .height
}

#[gpui::test]
fn the_strip_is_one_row_wide_and_two_narrow_with_its_toggle_in_view(cx: &mut TestAppContext) {
    // The widest values: u64::MAX, i64::MIN and a long f64.
    let mut bytes = f64::MIN_POSITIVE.to_le_bytes().to_vec();
    bytes.extend(counting(64));
    let (_dir, path) = file_of(&bytes);
    let (viewer, cx, _) = viewer(cx, &path);
    cx.simulate_resize(gpui::size(px(1000.), px(400.)));
    draw(cx);
    let wide = strip_height(cx);
    let toggle = cx.debug_bounds("hex-viewer-endian").expect("toggle");
    assert!(toggle.right() <= px(1000.), "{toggle:?}");
    // Long values stay inside the strip and don't change its height.
    let strip = cx.debug_bounds("hex-viewer-inspector").unwrap();
    let f64_cell = cx.debug_bounds("hex-inspect-f64").expect("f64");
    assert!(f64_cell.right() <= strip.right());
    viewer.update(cx, |v, cx| {
        v.select(8, 8);
        cx.notify();
    });
    draw(cx);
    assert_eq!(strip_height(cx), wide, "content doesn't change the height");

    cx.simulate_resize(gpui::size(px(500.), px(400.)));
    draw(cx);
    let narrow = strip_height(cx);
    assert!(narrow > wide, "{narrow:?} vs {wide:?}");
    let strip = cx.debug_bounds("hex-viewer-inspector").unwrap();
    let toggle = cx.debug_bounds("hex-viewer-endian").expect("toggle");
    assert!(toggle.right() <= strip.right(), "{toggle:?} in {strip:?}");
    let f64_cell = cx.debug_bounds("hex-inspect-f64").expect("f64");
    let u8_cell = cx.debug_bounds("hex-inspect-u8").unwrap();
    assert!(f64_cell.top() > u8_cell.top(), "floats on the second row");
    assert!(f64_cell.right() <= strip.right());
}

#[test]
fn a_long_float_wraps_in_its_tooltip_whole() {
    let text = f64::MIN_POSITIVE.to_string();
    assert!(text.len() > 300);
    let full = format!("f64 {text}");
    // 7 px a char, 100 px a line: 14 chars, the last fewer.
    let x_at = |ix: usize| px(7. * ix as f32);
    let lines = value_tip_lines(&full, px(100.), x_at);
    assert!(lines.len() > 20);
    for (line, width) in &lines {
        assert!(line.len() <= 14 && *width <= px(100.), "{line} {width:?}");
        assert_eq!(*width, px(7. * line.len() as f32));
    }
    assert!(lines[..lines.len() - 1].iter().all(|(l, _)| l.len() == 14));
    let joined: String = lines.iter().map(|(line, _)| line.as_ref()).collect();
    assert_eq!(joined, full, "nothing is cut");
    // Narrower than a char, each line still takes one.
    let lines = value_tip_lines(&full, px(1.), x_at);
    assert_eq!(lines.len(), full.len());
    assert_eq!(value_tip_lines("", px(100.), x_at), [("".into(), px(0.))]);
}

#[gpui::test]
fn the_arrangement_follows_the_width_without_a_forced_draw(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&counting(64));
    let (_viewer, cx, _) = viewer(cx, &path);
    let one_row = |cx: &mut gpui::VisualTestContext| {
        let f = cx.debug_bounds("hex-inspect-f64").unwrap().top();
        let u = cx.debug_bounds("hex-inspect-u8").unwrap().top();
        f == u
    };
    // One frame, as a window gives; the viewer asks for the next itself.
    cx.simulate_resize(gpui::size(px(500.), px(400.)));
    draw(cx);
    cx.run_until_parked();
    assert!(!one_row(cx), "narrow from the first frames after a resize");
    cx.simulate_resize(gpui::size(px(1000.), px(400.)));
    draw(cx);
    cx.run_until_parked();
    assert!(one_row(cx), "and back");
}

#[gpui::test]
fn a_very_narrow_strip_wraps_and_keeps_every_type_and_the_toggle(cx: &mut TestAppContext) {
    let (_dir, path) = file_of(&counting(64));
    let (viewer, cx, _) = viewer(cx, &path);
    for width in [400., 250., 150.] {
        cx.simulate_resize(gpui::size(px(width), px(400.)));
        draw(cx);
        cx.run_until_parked();
        let strip = cx.debug_bounds("hex-viewer-inspector").unwrap();
        for id in [
            "hex-inspect-u8",
            "hex-inspect-i8",
            "hex-inspect-u16",
            "hex-inspect-i16",
            "hex-inspect-u32",
            "hex-inspect-i32",
            "hex-inspect-u64",
            "hex-inspect-i64",
            "hex-inspect-f32",
            "hex-inspect-f64",
            "hex-viewer-endian",
        ] {
            let b = cx
                .debug_bounds(id)
                .unwrap_or_else(|| panic!("{id} at {width}"));
            assert!(
                b.left() >= strip.left() && b.right() <= strip.right() + px(0.5),
                "{id} inside the strip at {width}: {b:?} in {strip:?}"
            );
        }
        // The toggle still works there.
        let before = viewer.read_with(cx, |v, _| v.endian);
        let toggle = cx.debug_bounds("hex-viewer-endian").unwrap();
        cx.simulate_click(toggle.center(), Modifiers::none());
        assert_ne!(viewer.read_with(cx, |v, _| v.endian), before, "at {width}");
    }
}
