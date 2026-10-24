//! [`ImageViewer`] in a headless window: decoding, zoom, panning, the
//! checkerboard, the status bar items and reloading, over images written
//! to a tempdir.

use std::{
    io::Cursor,
    path::{Path, PathBuf},
};

use fe_command::Value;
use gpui::{
    AppContext as _, Entity, Modifiers, MouseButton, ScrollDelta, ScrollWheelEvent, TestAppContext,
    TouchPhase, VisualTestContext, point, px,
};
use kiln_shell::PaneItem as _;
use tempfile::TempDir;

use super::*;

/// `name` in a new tempdir: a `width` × `height` PNG of `rgba`.
fn png(width: u32, height: u32, rgba: [u8; 4]) -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("picture.png");
    write_png(&path, width, height, rgba);
    (dir, path)
}

fn write_png(path: &Path, width: u32, height: u32, rgba: [u8; 4]) {
    image::RgbaImage::from_pixel(width, height, image::Rgba(rgba))
        .save(path)
        .unwrap();
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A focused viewer of `path` in a headless window, loaded and drawn.
fn open_viewer<'a>(
    cx: &'a mut TestAppContext,
    path: &Path,
) -> (Entity<ImageViewer>, &'a mut VisualTestContext) {
    cx.update(crate::init);
    let path = path.to_owned();
    let (viewer, cx) = cx.add_window_view(move |_, cx| {
        let theme = EditorTheme::default_dark(cx);
        ImageViewer::new(path, Zoom::Fit, theme, cx)
    });
    let focus = viewer.read_with(cx, |viewer, _| viewer.focus.clone());
    cx.update(|window, cx| window.focus(&focus, cx));
    draw(cx);
    draw(cx);
    (viewer, cx)
}

fn geometry(viewer: &Entity<ImageViewer>, cx: &mut VisualTestContext) -> Geometry {
    viewer.read_with(cx, |viewer, _| {
        viewer.geometry().expect("a picture is shown")
    })
}

fn viewport(viewer: &Entity<ImageViewer>, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    viewer.read_with(cx, |viewer, _| viewer.viewport)
}

/// The pixel of the picture under `at`.
fn pixel_under(geometry: Geometry, at: Point<Pixels>) -> (f32, f32) {
    (
        f32::from(at.x - geometry.bounds.origin.x) / geometry.scale,
        f32::from(at.y - geometry.bounds.origin.y) / geometry.scale,
    )
}

#[gpui::test]
fn png_decodes_and_reports_its_dimensions(cx: &mut TestAppContext) {
    let (_dir, path) = png(40, 30, [10, 20, 30, 255]);
    let (viewer, cx) = open_viewer(cx, &path);
    viewer.read_with(cx, |viewer, _| {
        assert!(!viewer.is_loading());
        assert_eq!(viewer.pixel_size(), Some((40, 30)));
        assert_eq!(
            viewer.file_len(),
            Some(std::fs::metadata(&path).unwrap().len())
        );
        assert_eq!(viewer.message(), None);
    });
}

#[gpui::test]
fn fit_scales_a_large_image_down_and_never_a_small_one_up(cx: &mut TestAppContext) {
    let (_big_dir, big) = png(4000, 2000, [1, 2, 3, 255]);
    let (viewer_big, cx) = open_viewer(cx, &big);
    let view = viewport(&viewer_big, cx);
    let geometry_big = geometry(&viewer_big, cx);
    assert!(geometry_big.scale < 1.);
    assert!(geometry_big.bounds.size.width <= view.size.width + px(0.5));
    assert!(geometry_big.bounds.size.height <= view.size.height + px(0.5));
    // One axis fills the viewport, and the aspect ratio holds.
    let fills_width = (geometry_big.bounds.size.width - view.size.width).abs() < px(0.5);
    let fills_height = (geometry_big.bounds.size.height - view.size.height).abs() < px(0.5);
    assert!(fills_width || fills_height);
    let ratio = geometry_big.bounds.size.width / geometry_big.bounds.size.height;
    assert!((ratio - 2.).abs() < 0.01, "{ratio}");

    let (_small_dir, small) = png(20, 10, [1, 2, 3, 255]);
    let (viewer_small, cx) = open_viewer(cx, &small);
    let view = viewport(&viewer_small, cx);
    let geometry_small = geometry(&viewer_small, cx);
    assert_eq!(geometry_small.scale, 1.);
    assert_eq!(geometry_small.bounds.size, size(px(20.), px(10.)));
    // Centred.
    assert_eq!(geometry_small.bounds.center(), view.center());
}

#[gpui::test]
fn actual_size_is_one_pixel_per_point(cx: &mut TestAppContext) {
    let (_dir, path) = png(3000, 2000, [1, 2, 3, 255]);
    let (viewer, cx) = open_viewer(cx, &path);
    // `cmd-1` is `image.zoom_actual` in the viewer's key context.
    cx.simulate_keystrokes("cmd-1");
    draw(cx);
    let actual = geometry(&viewer, cx);
    assert_eq!(actual.scale, 1.);
    assert_eq!(actual.bounds.size, size(px(3000.), px(2000.)));
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.zoom(), Zoom::Scale(1.));
        assert_eq!(viewer.zoom_text(), "100%");
    });

    cx.simulate_keystrokes("cmd-0");
    draw(cx);
    viewer.read_with(cx, |viewer, _| assert_eq!(viewer.zoom(), Zoom::Fit));
}

#[gpui::test]
fn zoom_steps_stop_at_the_ends(cx: &mut TestAppContext) {
    let (_dir, path) = png(40, 30, [1, 2, 3, 255]);
    let (viewer, cx) = open_viewer(cx, &path);
    let scale = |cx: &mut VisualTestContext| match viewer.read_with(cx, |v, _| v.zoom()) {
        Zoom::Scale(scale) => scale,
        Zoom::Fit => 1.,
    };
    let mut seen = Vec::new();
    for _ in 0..14 {
        cx.simulate_keystrokes("cmd-=");
        draw(cx);
        seen.push(scale(cx));
    }
    assert_eq!(
        seen,
        [
            1.5, 2., 3., 4., 8., 12., 16., 20., 20., 20., 20., 20., 20., 20.
        ]
    );
    let mut seen = Vec::new();
    for _ in 0..14 {
        cx.simulate_keystrokes("cmd--");
        draw(cx);
        seen.push(scale(cx));
    }
    assert_eq!(
        seen,
        [
            16., 12., 8., 4., 3., 2., 1.5, 1., 0.75, 0.5, 0.25, 0.1, 0.1, 0.1
        ]
    );
}

fn cmd_wheel(at: Point<Pixels>, dy: f32) -> ScrollWheelEvent {
    ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
        modifiers: Modifiers::secondary_key(),
        touch_phase: TouchPhase::Moved,
    }
}

#[gpui::test]
fn wheel_zoom_keeps_the_pointer_point_fixed(cx: &mut TestAppContext) {
    let (_dir, path) = png(3000, 2000, [1, 2, 3, 255]);
    let (viewer, cx) = open_viewer(cx, &path);
    cx.simulate_keystrokes("cmd-1");
    draw(cx);
    let view = viewport(&viewer, cx);
    let at = view.origin + point(px(300.), px(200.));
    let before = pixel_under(geometry(&viewer, cx), at);
    for dy in [-40., -40., 60.] {
        cx.simulate_event(cmd_wheel(at, dy));
        draw(cx);
        let geometry = geometry(&viewer, cx);
        let after = pixel_under(geometry, at);
        assert!((after.0 - before.0).abs() < 0.5, "{before:?} {after:?}");
        assert!((after.1 - before.1).abs() < 0.5, "{before:?} {after:?}");
    }
    // Zoomed in on net.
    assert!(geometry(&viewer, cx).scale > 1.);

    // A pinch too.
    let before = pixel_under(geometry(&viewer, cx), at);
    cx.simulate_event(PinchEvent {
        position: at,
        delta: -0.3,
        modifiers: Modifiers::default(),
        phase: TouchPhase::Moved,
    });
    draw(cx);
    let after = pixel_under(geometry(&viewer, cx), at);
    assert!((after.0 - before.0).abs() < 0.5 && (after.1 - before.1).abs() < 0.5);
}

#[gpui::test]
fn click_toggles_fit_and_actual(cx: &mut TestAppContext) {
    let (_dir, path) = png(3000, 2000, [1, 2, 3, 255]);
    let (viewer, cx) = open_viewer(cx, &path);
    let at = geometry(&viewer, cx).bounds.center();
    cx.simulate_click(at, Modifiers::default());
    draw(cx);
    viewer.read_with(cx, |viewer, _| assert_eq!(viewer.zoom(), Zoom::Scale(1.)));
    cx.simulate_click(at, Modifiers::default());
    draw(cx);
    viewer.read_with(cx, |viewer, _| assert_eq!(viewer.zoom(), Zoom::Fit));
}

#[gpui::test]
fn drag_pans_a_zoomed_image(cx: &mut TestAppContext) {
    let (_dir, path) = png(3000, 2000, [1, 2, 3, 255]);
    let (viewer, cx) = open_viewer(cx, &path);
    cx.simulate_keystrokes("cmd-1");
    draw(cx);
    let scroll = |cx: &mut VisualTestContext| viewer.read_with(cx, |viewer, _| viewer.scroll);
    let before = scroll(cx);
    let start = point(px(500.), px(400.));
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(
        start - point(px(100.), px(50.)),
        MouseButton::Left,
        Modifiers::default(),
    );
    cx.simulate_mouse_up(
        start - point(px(100.), px(50.)),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    assert_eq!(scroll(cx), before + point(px(100.), px(50.)));
    // A drag isn't a click: the zoom stays.
    viewer.read_with(cx, |viewer, _| assert_eq!(viewer.zoom(), Zoom::Scale(1.)));

    // The wheel alone scrolls too, and stops at the edge.
    cx.simulate_event(ScrollWheelEvent {
        position: start,
        delta: ScrollDelta::Pixels(point(px(0.), px(-30.))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
    });
    draw(cx);
    assert_eq!(scroll(cx).y, before.y + px(80.));
    cx.simulate_event(ScrollWheelEvent {
        position: start,
        delta: ScrollDelta::Pixels(point(px(10_000.), px(0.))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
    });
    draw(cx);
    assert_eq!(scroll(cx).x, px(0.));
}

#[gpui::test]
fn transparent_png_paints_the_checkerboard(cx: &mut TestAppContext) {
    let (_dir, path) = png(100, 80, [200, 0, 0, 128]);
    let (viewer, cx) = open_viewer(cx, &path);
    let painted = viewer.read_with(cx, |viewer, _| viewer.painted.expect("painted"));
    assert_eq!(painted.checkerboard, Some(painted.image));
    assert_eq!(painted.image.size, size(px(100.), px(80.)));

    // An opaque picture needs none.
    let (_dir, path) = png(100, 80, [200, 0, 0, 255]);
    let (viewer, cx) = open_viewer(cx, &path);
    let painted = viewer.read_with(cx, |viewer, _| viewer.painted.expect("painted"));
    assert_eq!(painted.checkerboard, None);
    assert_eq!(painted.image.size, size(px(100.), px(80.)));

    // Two greys, lighter and darker steps from the background's.
    let (light, dark) = checker_colors(gpui::hsla(0., 0., 0.1, 1.));
    assert!(light.l > 0.1 && dark.l > light.l);
    let (light, dark) = checker_colors(gpui::hsla(0., 0., 1., 1.));
    assert!(light.l < 1. && dark.l < light.l);
}

#[gpui::test]
fn undecodable_file_shows_the_reason(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("broken.png");
    std::fs::write(&path, "this is not an image").unwrap();
    let (viewer, cx) = open_viewer(cx, &path);
    viewer.read_with(cx, |viewer, _| {
        let message = viewer.message().expect("a message").to_string();
        assert!(
            message.starts_with("Can't display this image: "),
            "{message}"
        );
        assert!(message.len() > "Can't display this image: ".len());
        assert_eq!(viewer.pixel_size(), None);
    });

    // A truncated PNG fails in the decoder, with its reason.
    let (_dir, good) = png(30, 30, [1, 2, 3, 255]);
    let bytes = std::fs::read(&good).unwrap();
    let cut = dir.path().join("cut.png");
    std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
    let (viewer, cx) = open_viewer(cx, &cut);
    viewer.read_with(cx, |viewer, _| {
        let message = viewer.message().expect("a message").to_string();
        assert!(
            message.starts_with("Can't display this image: "),
            "{message}"
        );
    });
}

#[gpui::test]
fn status_items_show_dimensions_size_and_zoom(cx: &mut TestAppContext) {
    let (_dir, path) = png(40, 30, [1, 2, 3, 255]);
    let len = std::fs::metadata(&path).unwrap().len();
    let (viewer, cx) = open_viewer(cx, &path);
    let items = |cx: &mut VisualTestContext| {
        viewer.read_with(cx, |viewer, cx| {
            viewer
                .status_items(cx)
                .into_iter()
                .map(|item| {
                    let command = item.command.as_ref().map(|c| c.command.as_str().to_owned());
                    (item.id.to_string(), item.text.to_string(), command)
                })
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(
        items(cx),
        [
            ("image_dimensions".to_owned(), "40 × 30".to_owned(), None),
            ("image_file_size".to_owned(), size_text(len), None),
            (
                "image_zoom".to_owned(),
                "Fit".to_owned(),
                Some("image.zoom_actual".to_owned())
            ),
        ]
    );
    cx.simulate_keystrokes("cmd-=");
    draw(cx);
    let items = items(cx);
    assert_eq!(items[2].1, "150%");
    assert_eq!(items[2].2.as_deref(), Some("image.zoom_fit"));
}

#[gpui::test]
fn reload_on_activation_after_the_file_changed(cx: &mut TestAppContext) {
    let (_dir, path) = png(10, 10, [1, 2, 3, 255]);
    let (viewer, cx) = open_viewer(cx, &path);
    cx.simulate_keystrokes("cmd-=");
    draw(cx);
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.pixel_size(), Some((10, 10)))
    });

    // Changed while the window was in the background.
    cx.deactivate_window();
    write_png(&path, 64, 48, [9, 9, 9, 255]);
    draw(cx);
    // Nothing is checked while it's away.
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.pixel_size(), Some((10, 10)))
    });
    cx.update(|window, _| window.activate_window());
    draw(cx);
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.pixel_size(), Some((64, 48)));
        // The zoom is kept.
        assert_eq!(viewer.zoom(), Zoom::Scale(1.5));
    });

    // Gone.
    std::fs::remove_file(&path).unwrap();
    cx.deactivate_window();
    draw(cx);
    cx.update(|window, _| window.activate_window());
    draw(cx);
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.message().map(|m| m.as_ref()), Some("File not found"));
    });
}

/// A PNG header that claims `width` × `height`, with no pixels.
fn header_only_png(width: u32, height: u32) -> Vec<u8> {
    fn crc(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
    let mut ihdr = b"IHDR".to_vec();
    ihdr.extend(width.to_be_bytes());
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend(13u32.to_be_bytes());
    bytes.extend(&ihdr);
    bytes.extend(crc(&ihdr).to_be_bytes());
    // The decoder reads on to the first pixel data.
    bytes.extend(0u32.to_be_bytes());
    bytes.extend(b"IDAT");
    bytes.extend(crc(b"IDAT").to_be_bytes());
    bytes
}

#[gpui::test]
fn too_large_is_refused(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    // Over 64 MiB: a sparse file, never read.
    let heavy = dir.path().join("heavy.png");
    std::fs::File::create(&heavy)
        .unwrap()
        .set_len(MAX_BYTES + 1)
        .unwrap();
    let (viewer, cx) = open_viewer(cx, &heavy);
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(
            viewer.message().map(|m| m.as_ref()),
            Some("Too large to preview")
        );
    });

    // Over 100 megapixels: refused from the header, before decoding.
    let wide = dir.path().join("wide.png");
    std::fs::write(&wide, header_only_png(20_000, 20_000)).unwrap();
    let (viewer, cx) = open_viewer(cx, &wide);
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(
            viewer.message().map(|m| m.as_ref()),
            Some("Too large to preview")
        );
    });
}

#[gpui::test]
async fn restore_round_trips_the_path_and_zoom(cx: &mut TestAppContext) {
    let (_dir, path) = png(3000, 2000, [1, 2, 3, 255]);
    let (viewer, cx) = open_viewer(cx, &path);
    cx.simulate_keystrokes("cmd-=");
    draw(cx);
    let state = viewer
        .read_with(cx, |viewer, cx| viewer.serialize(cx))
        .expect("a state");
    assert_eq!(state.get("path").and_then(Value::as_str), path.to_str());

    let restored = cx
        .update(|window, cx| ImageViewer::restore(state.clone(), window, cx))
        .await
        .expect("restored");
    cx.run_until_parked();
    restored.read_with(cx, |restored, _| {
        assert_eq!(restored.path(), path);
        assert_eq!(restored.zoom(), viewer.read_with(cx, |v, _| v.zoom()));
        assert_eq!(restored.pixel_size(), Some((3000, 2000)));
    });

    // Fit round-trips as Fit.
    let fit = Value::from_iter([
        ("path", Value::from(path.to_str().unwrap())),
        ("zoom", "fit".into()),
    ]);
    let restored = cx
        .update(|window, cx| ImageViewer::restore(fit, window, cx))
        .await
        .expect("restored");
    restored.read_with(cx, |restored, _| assert_eq!(restored.zoom(), Zoom::Fit));

    // A file that's gone isn't restored.
    std::fs::remove_file(&path).unwrap();
    let gone = cx
        .update(|window, cx| ImageViewer::restore(state, window, cx))
        .await;
    assert!(gone.is_none());
}

#[gpui::test]
fn only_the_first_frame_of_an_animation_is_kept(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("spin.gif");
    {
        let mut encoder =
            image::codecs::gif::GifEncoder::new(std::fs::File::create(&path).unwrap());
        for shade in [10u8, 120, 240] {
            let pixels = image::RgbaImage::from_pixel(20, 10, image::Rgba([shade, 0, 0, 255]));
            encoder.encode_frame(image::Frame::new(pixels)).unwrap();
        }
    }
    let (viewer, cx) = open_viewer(cx, &path);
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.pixel_size(), Some((20, 10)));
        let picture = viewer.picture().expect("a picture");
        assert_eq!(picture.image.frame_count(), 1);
        // The first frame's colour, as BGRA.
        assert_eq!(&picture.image.as_bytes(0).unwrap()[..4], [0, 0, 10, 255]);
    });
}

#[test]
fn a_read_stops_one_byte_past_the_limit() {
    assert_eq!(
        read_limited(Cursor::new(vec![7u8; 10]), 10).unwrap(),
        Some(vec![7u8; 10])
    );
    assert_eq!(read_limited(Cursor::new(vec![7u8; 11]), 10).unwrap(), None);
    // An endless source is cut off, not read whole.
    assert_eq!(read_limited(std::io::repeat(1), 1000).unwrap(), None);
    // A directory isn't a picture.
    let dir = TempDir::new().unwrap();
    assert!(matches!(decode(dir.path()), Decoded::Failed(_)));
}

#[gpui::test]
fn a_split_made_while_decoding_loads_for_itself(cx: &mut TestAppContext) {
    let (_dir, path) = png(40, 30, [1, 2, 3, 255]);
    cx.update(crate::init);
    let theme = cx.update(|cx| EditorTheme::default_dark(cx));
    let viewer = cx.new(|cx| ImageViewer::new(path.clone(), Zoom::Scale(2.), theme, cx));
    // Nothing has run yet: still loading.
    assert!(viewer.read_with(cx, |viewer, _| viewer.is_loading()));
    let snapshot = viewer.read_with(cx, |viewer, _| viewer.snapshot());
    let split = cx.new(|cx| ImageViewer::from_snapshot(snapshot, cx));
    assert!(split.read_with(cx, |split, _| split.is_loading()));
    cx.run_until_parked();
    for viewer in [&viewer, &split] {
        viewer.read_with(cx, |viewer, _| {
            assert!(!viewer.is_loading());
            assert_eq!(viewer.pixel_size(), Some((40, 30)));
            assert_eq!(viewer.zoom(), Zoom::Scale(2.));
        });
    }

    // Split after the picture is there: no second read.
    let snapshot = viewer.read_with(cx, |viewer, _| viewer.snapshot());
    let later = cx.new(|cx| ImageViewer::from_snapshot(snapshot, cx));
    cx.run_until_parked();
    later.read_with(cx, |later, _| {
        assert_eq!(later.pixel_size(), Some((40, 30)));
        assert_eq!(later.loads, 0);
    });
}

#[gpui::test]
fn a_saved_zoom_is_applied_to_a_split(cx: &mut TestAppContext) {
    let (_dir, path) = png(40, 30, [1, 2, 3, 255]);
    let (viewer, cx) = open_viewer(cx, &path);
    let state = |zoom: Value| {
        Value::from_iter([
            ("path", Value::from(path.to_str().unwrap())),
            ("zoom", zoom),
        ])
    };
    cx.update(|window, cx| {
        viewer.update(cx, |viewer, cx| {
            viewer.apply_state(&state(Value::from(3.0)), window, cx);
            assert_eq!(viewer.zoom(), Zoom::Scale(3.));
            viewer.apply_state(&state("fit".into()), window, cx);
            assert_eq!(viewer.zoom(), Zoom::Fit);
        })
    });
}

#[gpui::test]
fn a_change_while_decoding_is_read_again(cx: &mut TestAppContext) {
    let (_dir, path) = png(10, 10, [1, 2, 3, 255]);
    cx.update(crate::init);
    let theme = cx.update(|cx| EditorTheme::default_dark(cx));
    let viewer = cx.new(|cx| ImageViewer::new(path.clone(), Zoom::Fit, theme, cx));
    // The file changes after the stamp was taken, before it was read.
    write_png(&path, 64, 48, [9, 9, 9, 255]);
    cx.run_until_parked();
    viewer.read_with(cx, |viewer, _| {
        assert!(!viewer.is_loading());
        assert_eq!(viewer.pixel_size(), Some((64, 48)));
        // Read again, since the stamp it was read under was the old one.
        assert_eq!(viewer.loads, 2);
    });
}

#[gpui::test]
fn a_split_made_during_a_reload_gets_the_changed_image(cx: &mut TestAppContext) {
    let (_dir, path) = png(10, 10, [1, 2, 3, 255]);
    cx.update(crate::init);
    let theme = cx.update(|cx| EditorTheme::default_dark(cx));
    let viewer = cx.new(|cx| ImageViewer::new(path.clone(), Zoom::Fit, theme, cx));
    cx.run_until_parked();
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.pixel_size(), Some((10, 10)))
    });

    // A reload starts, and the viewer is split before it finishes.
    write_png(&path, 64, 48, [9, 9, 9, 255]);
    viewer.update(cx, |viewer, cx| viewer.check_disk(cx));
    assert!(viewer.read_with(cx, |viewer, _| viewer.is_loading()));
    let snapshot = viewer.read_with(cx, |viewer, _| viewer.snapshot());
    let split = cx.new(|cx| ImageViewer::from_snapshot(snapshot, cx));
    cx.run_until_parked();
    for viewer in [&viewer, &split] {
        viewer.read_with(cx, |viewer, _| {
            assert!(!viewer.is_loading());
            assert_eq!(viewer.pixel_size(), Some((64, 48)));
        });
    }
}
