//! [`ImageViewer`] on SVG files in a headless window: the intrinsic size,
//! svgz, the link rule, drawing again once a zoom settles, the size limits
//! and the checkerboard, over files written to a tempdir.

use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln_shell::PaneItem as _;
use tempfile::TempDir;

use super::*;

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A focused viewer of `path`, with `root` as the workspace, loaded and
/// drawn.
fn open_viewer<'a>(
    cx: &'a mut TestAppContext,
    path: &Path,
    root: Option<&Path>,
) -> (Entity<ImageViewer>, &'a mut VisualTestContext) {
    cx.update(crate::init);
    let (path, root) = (path.to_owned(), root.map(Path::to_owned));
    let (viewer, cx) = cx.add_window_view(move |_, cx| {
        let theme = EditorTheme::default_dark(cx);
        ImageViewer::in_workspace(path, Zoom::Fit, root, theme, cx)
    });
    let focus = viewer.read_with(cx, |viewer, _| viewer.focus.clone());
    cx.update(|window, cx| window.focus(&focus, cx));
    draw(cx);
    draw(cx);
    (viewer, cx)
}

/// `body` as the SVG `name` in a new tempdir.
fn svg(name: &str, body: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, body).unwrap();
    (dir, path)
}

const RED_40_BY_30: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect width="40" height="30" fill="red"/></svg>"#;

#[gpui::test]
fn svg_renders_at_its_intrinsic_size(cx: &mut TestAppContext) {
    let (_dir, path) = svg("a.svg", RED_40_BY_30);
    let (viewer, cx) = open_viewer(cx, &path, None);
    viewer.read_with(cx, |viewer, cx| {
        assert_eq!(viewer.message(), None);
        assert_eq!(viewer.pixel_size(), Some((40, 30)));
        assert_eq!(viewer.file_len(), Some(RED_40_BY_30.len() as u64));
        // Drawn at twice the size, as the Markdown preview does.
        assert_eq!(viewer.rendered_size(), Some((80, 60)));
        let geometry = viewer.geometry().unwrap();
        assert_eq!(geometry.scale, 1.);
        assert_eq!(geometry.bounds.size, size(px(40.), px(30.)));
        let items = viewer.status_items(cx);
        let dimensions = items
            .iter()
            .find(|item| item.id == crate::image_status::DIMENSIONS)
            .expect("the size is in the status bar");
        assert_eq!(dimensions.text.as_ref(), "40 × 30");
        assert!(viewer.is_svg_source());
    });
}

#[gpui::test]
fn viewbox_only_svg_sizes_from_the_viewbox(cx: &mut TestAppContext) {
    let (_dir, path) = svg(
        "a.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 32"><rect width="64" height="32"/></svg>"#,
    );
    let (viewer, cx) = open_viewer(cx, &path, None);
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.message(), None);
        assert_eq!(viewer.pixel_size(), Some((64, 32)));
    });
}

/// `data` as a gzip file of stored (uncompressed) blocks.
fn gzip(data: &[u8]) -> Vec<u8> {
    let mut crc = !0u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (!(crc & 1)).wrapping_add(1));
        }
    }
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    let mut chunks = data.chunks(0xffff).peekable();
    if chunks.peek().is_none() {
        out.extend([1, 0, 0, 0xff, 0xff]);
    }
    while let Some(chunk) = chunks.next() {
        let len = chunk.len() as u16;
        out.push(u8::from(chunks.peek().is_none()));
        out.extend(len.to_le_bytes());
        out.extend((!len).to_le_bytes());
        out.extend(chunk);
    }
    out.extend((!crc).to_le_bytes());
    out.extend((data.len() as u32).to_le_bytes());
    out
}

#[gpui::test]
fn svgz_is_decompressed(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("a.svgz");
    let packed = gzip(RED_40_BY_30.as_bytes());
    std::fs::write(&path, &packed).unwrap();
    let (viewer, cx) = open_viewer(cx, &path, None);
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.message(), None);
        assert_eq!(viewer.pixel_size(), Some((40, 30)));
        assert_eq!(viewer.file_len(), Some(packed.len() as u64));
        // Compressed: no text to show.
        assert!(!viewer.is_svg_source());
    });
}

/// A workspace `root` and a folder `outside` it, each in `dir`, with a file
/// in each: `root/inside.png` and `outside/secret.png`.
fn two_folders() -> (TempDir, PathBuf, PathBuf) {
    let dir = TempDir::new().unwrap();
    let base = dunce::canonicalize(dir.path()).unwrap();
    let (root, outside) = (base.join("root"), base.join("outside"));
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    for file in [root.join("inside.png"), outside.join("secret.png")] {
        image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 255, 255]))
            .save(file)
            .unwrap();
    }
    (dir, root, outside)
}

fn linking(href: &Path) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="40" height="30"><image width="40" height="30" xlink:href="{}"/></svg>"#,
        href.display()
    )
}

#[gpui::test]
fn a_link_outside_the_workspace_is_blocked(cx: &mut TestAppContext) {
    let (_dir, root, outside) = two_folders();
    let path = root.join("sub").join("a.svg");
    std::fs::write(&path, linking(&outside.join("secret.png"))).unwrap();
    let (viewer, cx) = open_viewer(cx, &path, Some(&root));
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(
            viewer.message().map(|m| m.as_ref()),
            Some("Blocked: links a file outside the workspace")
        );
        assert_eq!(viewer.pixel_size(), None);
    });
}

#[gpui::test]
fn a_link_inside_the_folder_is_allowed(cx: &mut TestAppContext) {
    let (_dir, root, _) = two_folders();
    let path = root.join("sub").join("a.svg");
    // In the workspace, but not the SVG's own folder.
    std::fs::write(&path, linking(&root.join("inside.png"))).unwrap();
    let (viewer, cx) = open_viewer(cx, &path, Some(&root));
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.message(), None);
        assert_eq!(viewer.pixel_size(), Some((40, 30)));
    });
    // In the SVG's own folder, with no workspace.
    let sibling = root.join("sub").join("near.png");
    std::fs::copy(root.join("inside.png"), &sibling).unwrap();
    let path = root.join("sub").join("b.svg");
    std::fs::write(&path, linking(&sibling)).unwrap();
    let (viewer, cx) = open_viewer(cx, &path, None);
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.message(), None);
        assert_eq!(viewer.pixel_size(), Some((40, 30)));
    });
}

#[gpui::test]
fn zoom_in_rerenders_after_settling(cx: &mut TestAppContext) {
    let (_dir, path) = svg("a.svg", RED_40_BY_30);
    let (viewer, cx) = open_viewer(cx, &path, None);
    let scale_factor = cx.update(|window, _| window.scale_factor());
    let drawn = viewer.read_with(cx, |viewer, _| viewer.rendered_size().unwrap());
    assert_eq!(drawn, (80, 60));
    let at = viewer.read_with(cx, |viewer, _| viewer.viewport_centre());
    viewer.update(cx, |viewer, cx| viewer.zoom_to(8., at, cx));
    draw(cx);
    // The bitmap scales until the zoom has settled.
    cx.executor().advance_clock(Duration::from_millis(100));
    draw(cx);
    assert_eq!(
        viewer.read_with(cx, |viewer, _| viewer.rendered_size().unwrap()),
        drawn
    );
    cx.executor().advance_clock(Duration::from_millis(100));
    draw(cx);
    draw(cx);
    let wanted = (
        (40. * 8. * scale_factor).ceil() as u32,
        (30. * 8. * scale_factor).ceil() as u32,
    );
    assert_eq!(
        viewer.read_with(cx, |viewer, _| viewer.rendered_size().unwrap()),
        wanted
    );
    // The size the document says is the same.
    assert_eq!(
        viewer.read_with(cx, |viewer, _| viewer.pixel_size()),
        Some((40, 30))
    );
    // Zooming back out leaves the larger picture be.
    viewer.update(cx, |viewer, cx| viewer.zoom_to(1., at, cx));
    draw(cx);
    cx.executor().advance_clock(Duration::from_millis(300));
    draw(cx);
    assert_eq!(
        viewer.read_with(cx, |viewer, _| viewer.rendered_size().unwrap()),
        wanted
    );
}

#[gpui::test]
fn oversize_svg_is_refused(cx: &mut TestAppContext) {
    // Over 8 MiB.
    let padding = " ".repeat(MAX_SVG_BYTES as usize);
    let (_dir, path) = svg(
        "big.svg",
        &format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">{padding}</svg>"#),
    );
    let (viewer, cx) = open_viewer(cx, &path, None);
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(
            viewer.message().map(|m| m.as_ref()),
            Some("Too large to preview")
        );
    });
    // Over 100 megapixels.
    let (_dir, path) = svg(
        "wide.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="20000" height="10000"/>"#,
    );
    let (viewer, cx) = open_viewer(cx, &path, None);
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(
            viewer.message().map(|m| m.as_ref()),
            Some("Too large to preview")
        );
        assert_eq!(viewer.pixel_size(), None);
    });
}

#[gpui::test]
fn transparent_regions_show_the_checkerboard(cx: &mut TestAppContext) {
    let (_dir, path) = svg(
        "half.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect width="20" height="30" fill="red"/></svg>"#,
    );
    let (viewer, cx) = open_viewer(cx, &path, None);
    viewer.read_with(cx, |viewer, _| {
        let painted = viewer.painted.expect("painted");
        assert!(painted.checkerboard.is_some());
    });
    let (_dir, path) = svg("solid.svg", RED_40_BY_30);
    let (viewer, cx) = open_viewer(cx, &path, None);
    viewer.read_with(cx, |viewer, _| {
        let painted = viewer.painted.expect("painted");
        assert_eq!(painted.checkerboard, None);
    });
}

/// A gzip file of 48 rects, 64 × 48, with a dynamic Huffman block (made by
/// gzip).
const DYNAMIC_SVGZ: &[u8] = &[
    0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xff, 0x8d, 0xd6, 0xc9, 0x6e, 0xdb, 0x30,
    0x14, 0x85, 0xe1, 0x57, 0x31, 0xd8, 0x7d, 0xc4, 0x79, 0x28, 0xa2, 0xbc, 0x0b, 0x87, 0x4b, 0xdb,
    0x80, 0x3b, 0x20, 0x11, 0xaa, 0xe4, 0xed, 0x43, 0xa5, 0x05, 0xa2, 0x22, 0x07, 0xbe, 0xca, 0xc2,
    0xf0, 0x42, 0x1f, 0x6c, 0xf3, 0xf8, 0xb7, 0xf2, 0xf8, 0xf2, 0xe7, 0x7c, 0x7a, 0xfd, 0x71, 0xfb,
    0xf9, 0x32, 0x8b, 0xcb, 0xb2, 0xfc, 0xfe, 0x3e, 0x4d, 0xeb, 0xba, 0x3e, 0xac, 0xe6, 0xe1, 0xd7,
    0xf3, 0x79, 0xd2, 0x52, 0xca, 0x69, 0x5c, 0x21, 0x4e, 0xeb, 0xb5, 0x2d, 0x97, 0x59, 0x78, 0x2b,
    0x4e, 0x17, 0xba, 0x9e, 0x2f, 0xcb, 0x2c, 0x6c, 0x14, 0x4f, 0x8f, 0xcf, 0x54, 0x97, 0xd3, 0xeb,
    0x2c, 0xa4, 0x38, 0xbd, 0x7d, 0x3c, 0xfe, 0xbb, 0x30, 0x7c, 0x5e, 0x37, 0x9e, 0xf6, 0xeb, 0xed,
    0x36, 0x8b, 0x6f, 0xf2, 0xe3, 0x4f, 0x4c, 0x9f, 0x2c, 0x1e, 0x61, 0xda, 0xb9, 0x62, 0xdc, 0x9e,
    0x29, 0x7f, 0xc4, 0xd9, 0x5c, 0xbc, 0xcf, 0x7b, 0xa7, 0xed, 0x11, 0xe7, 0xbb, 0x52, 0xa9, 0xef,
    0x9d, 0xd1, 0x47, 0x5c, 0xb2, 0xbe, 0x36, 0xbb, 0x77, 0xf6, 0xd0, 0xb1, 0x94, 0x54, 0x83, 0x4c,
    0xff, 0xb9, 0x43, 0xe7, 0xd2, 0x48, 0x6b, 0x43, 0x7b, 0xe7, 0x0e, 0x9d, 0x8b, 0x34, 0xa1, 0x05,
    0xb3, 0x77, 0x7f, 0xdf, 0x66, 0x64, 0x66, 0x88, 0x2d, 0xe6, 0xf8, 0x75, 0x3d, 0x86, 0xd9, 0x66,
    0x4c, 0x6b, 0x60, 0x3d, 0xc6, 0x05, 0x1d, 0x49, 0x69, 0xb0, 0x1e, 0xe3, 0x52, 0xa0, 0x64, 0x03,
    0x58, 0x8f, 0x71, 0xa5, 0x5a, 0x1b, 0x2a, 0x58, 0x8f, 0x71, 0x34, 0xbe, 0x2b, 0x45, 0x81, 0xf5,
    0x18, 0x27, 0x7d, 0xcf, 0xe4, 0xc1, 0x7a, 0xdc, 0x0c, 0xc5, 0x39, 0x55, 0xbe, 0xae, 0xb7, 0x9d,
    0xea, 0x3d, 0xe7, 0x64, 0x91, 0x0e, 0xc4, 0xc7, 0xb9, 0x30, 0x60, 0x44, 0xf5, 0x71, 0x30, 0x65,
    0xef, 0x0b, 0xca, 0x8f, 0x83, 0xa5, 0x57, 0x45, 0xa8, 0x3f, 0x0e, 0x92, 0x55, 0x55, 0xa3, 0x00,
    0x39, 0x28, 0x53, 0x08, 0x0e, 0x15, 0xc8, 0x41, 0x4d, 0x6d, 0x7c, 0x4d, 0xc1, 0x88, 0xec, 0x1a,
    0x46, 0xb7, 0x0a, 0x1a, 0xdc, 0x8e, 0xe8, 0xee, 0x1a, 0x31, 0xc6, 0x0e, 0x22, 0xe4, 0x5c, 0x6a,
    0x34, 0x5e, 0x11, 0xac, 0xc8, 0xc1, 0x3a, 0x7e, 0x62, 0x3c, 0xca, 0x90, 0x83, 0x14, 0x52, 0x4a,
    0xa8, 0x43, 0x0e, 0xca, 0xda, 0x6d, 0x45, 0x21, 0x72, 0xd0, 0x28, 0xdb, 0x25, 0x2a, 0x91, 0x83,
    0xce, 0xe7, 0x6c, 0x50, 0x8a, 0xec, 0x1a, 0x23, 0x29, 0x0f, 0x5a, 0xdc, 0x3e, 0xe9, 0x3d, 0x97,
    0xa5, 0x97, 0x19, 0xb4, 0xc8, 0xb9, 0xea, 0x4a, 0x69, 0xa8, 0x45, 0x0e, 0x52, 0x56, 0xe3, 0x15,
    0xc1, 0x8a, 0x1c, 0x94, 0x3d, 0x28, 0x83, 0x5a, 0xe4, 0xa0, 0x19, 0x1b, 0x06, 0xd4, 0x22, 0x07,
    0x5d, 0xd2, 0x21, 0xa3, 0x16, 0x39, 0x18, 0x28, 0xea, 0x86, 0x5a, 0x64, 0xd7, 0x18, 0xf7, 0x27,
    0x05, 0x5a, 0xb4, 0xcc, 0x7d, 0xb4, 0x46, 0x13, 0x2d, 0x68, 0x91, 0x73, 0xd4, 0xd2, 0xb8, 0x03,
    0x83, 0x15, 0x39, 0xa8, 0x34, 0x51, 0x41, 0x2d, 0x72, 0xd0, 0x04, 0x9b, 0x08, 0xb5, 0xc8, 0x41,
    0x57, 0xf3, 0xf8, 0x4d, 0x05, 0x2b, 0x72, 0x30, 0xaa, 0xde, 0x1d, 0x6a, 0x91, 0x83, 0xd9, 0xbb,
    0x1c, 0x51, 0x8b, 0xec, 0x1a, 0xa5, 0x8c, 0x3a, 0x36, 0xb8, 0xfd, 0xc7, 0xfa, 0xf4, 0x0e, 0xe9,
    0x05, 0x25, 0xfa, 0xd9, 0x0a, 0x00, 0x00,
];

#[test]
fn a_dynamic_huffman_svgz_unpacks() {
    let svg = gunzip(DYNAMIC_SVGZ, MAX_SVG_BYTES as usize).unwrap();
    assert_eq!(svg.len(), 2777);
    assert!(svg.starts_with(br#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48">"#));
    assert!(svg.ends_with(b"</svg>"));
    assert_eq!(gunzip(DYNAMIC_SVGZ, 100), Err(Gunzip::TooLarge));
    assert_eq!(gunzip(&DYNAMIC_SVGZ[..200], 10_000), Err(Gunzip::Malformed));
}

#[gpui::test]
fn a_dynamic_huffman_svgz_draws(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("rects.svgz");
    std::fs::write(&path, DYNAMIC_SVGZ).unwrap();
    let (viewer, cx) = open_viewer(cx, &path, None);
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.message(), None);
        assert_eq!(viewer.pixel_size(), Some((64, 48)));
    });
}

/// A gzip file of one fixed-Huffman block that unpacks to over 8 MiB of `a`,
/// with a trailer that says 0 bytes.
fn forged_bomb() -> Vec<u8> {
    struct Bits(Vec<u8>, u32);
    impl Bits {
        /// `count` bits of `value`, low bit first.
        fn put(&mut self, value: u32, count: u32) {
            for bit in 0..count {
                if self.1.is_multiple_of(8) {
                    self.0.push(0);
                }
                *self.0.last_mut().unwrap() |= (((value >> bit) & 1) as u8) << (self.1 % 8);
                self.1 += 1;
            }
        }
        /// A Huffman code, high bit first.
        fn code(&mut self, value: u32, count: u32) {
            for bit in (0..count).rev() {
                self.put((value >> bit) & 1, 1);
            }
        }
    }
    let mut bits = Bits(Vec::new(), 0);
    bits.put(1, 1);
    bits.put(1, 2);
    // A literal 'a' (0x61): 8 bits, 0x30 + 0x61.
    bits.code(0x30 + 0x61, 8);
    // Matches of 258 at distance 1: length code 285 (8 bits, 0xc0 + 5),
    // distance code 0 (5 bits).
    for _ in 0..(MAX_SVG_BYTES / 258 + 10) {
        bits.code(0xc0 + 5, 8);
        bits.code(0, 5);
    }
    // End of block: code 256 (7 bits).
    bits.code(0, 7);
    let mut gz = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    gz.extend(bits.0);
    gz.extend([0; 8]);
    gz
}

#[gpui::test]
fn a_gzip_bomb_with_a_forged_trailer_is_refused(cx: &mut TestAppContext) {
    let bomb = forged_bomb();
    assert!(bomb.len() < 100_000, "{} bytes", bomb.len());
    assert_eq!(gunzip(&bomb, MAX_SVG_BYTES as usize), Err(Gunzip::TooLarge));
    // Unpacked within a larger limit, it is all `a`.
    let unpacked = gunzip(&bomb, 64 * 1024 * 1024).unwrap();
    assert!(unpacked.len() as u64 > MAX_SVG_BYTES);
    assert!(unpacked.iter().all(|byte| *byte == b'a'));
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bomb.svgz");
    std::fs::write(&path, &bomb).unwrap();
    let (viewer, cx) = open_viewer(cx, &path, None);
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(
            viewer.message().map(|m| m.as_ref()),
            Some("Too large to preview")
        );
    });
}

#[gpui::test]
fn an_svg_with_no_size_and_no_viewbox_is_300_by_150(cx: &mut TestAppContext) {
    // Drawn content that is smaller, content that is none, and one side given.
    for (body, expected) in [
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10"/></svg>"#,
            (300, 150),
        ),
        (r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#, (300, 150)),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="80"><rect width="10" height="10"/></svg>"#,
            (80, 150),
        ),
    ] {
        let (_dir, path) = svg("a.svg", body);
        let (viewer, cx) = open_viewer(cx, &path, None);
        viewer.read_with(cx, |viewer, _| {
            assert_eq!(viewer.message(), None, "{body}");
            assert_eq!(viewer.pixel_size(), Some(expected), "{body}");
            let (w, h) = viewer.rendered_size().unwrap();
            assert_eq!((w, h), (expected.0 * 2, expected.1 * 2), "{body}");
        });
    }
}

#[gpui::test]
fn a_restored_svg_still_links_files_in_the_workspace(cx: &mut TestAppContext) {
    let (_dir, root, _) = two_folders();
    let path = root.join("sub").join("a.svg");
    std::fs::write(&path, linking(&root.join("inside.png"))).unwrap();
    let (viewer, cx) = open_viewer(cx, &path, Some(&root));
    let state = viewer
        .read_with(cx, |viewer, cx| viewer.serialize(cx))
        .unwrap();
    let restoring = cx.update(|window, cx| ImageViewer::restore(state, window, cx));
    cx.run_until_parked();
    let restored = futures::FutureExt::now_or_never(restoring)
        .flatten()
        .expect("restored");
    draw(cx);
    restored.read_with(cx, |viewer, _| {
        assert_eq!(viewer.message(), None);
        assert_eq!(viewer.pixel_size(), Some((40, 30)));
        assert_eq!(viewer.root(), Some(root.as_path()));
    });
}

#[gpui::test]
fn thin_and_subpixel_svgs_draw(cx: &mut TestAppContext) {
    for (body, intrinsic, rendered) in [
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="20000"/>"#,
            (1, 20000),
            // The 2x render is cut to 8192 a side.
            (1, 8192),
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="0.4" height="0.4"/>"#,
            (1, 1),
            (1, 1),
        ),
    ] {
        let (_dir, path) = svg("a.svg", body);
        let (viewer, cx) = open_viewer(cx, &path, None);
        viewer.read_with(cx, |viewer, _| {
            assert_eq!(viewer.message(), None, "{body}");
            assert_eq!(viewer.pixel_size(), Some(intrinsic), "{body}");
            assert_eq!(viewer.rendered_size(), Some(rendered), "{body}");
        });
    }
}

#[gpui::test]
fn sharpening_compares_both_sides(cx: &mut TestAppContext) {
    let (_dir, path) = svg(
        "tall.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="100"><rect width="1" height="100"/></svg>"#,
    );
    let (viewer, cx) = open_viewer(cx, &path, None);
    let scale_factor = cx.update(|window, _| window.scale_factor());
    let at = viewer.read_with(cx, |viewer, _| viewer.viewport_centre());
    for zoom in [2.1, 2.9] {
        viewer.update(cx, |viewer, cx| viewer.zoom_to(zoom, at, cx));
        draw(cx);
        cx.executor().advance_clock(Duration::from_millis(300));
        draw(cx);
        draw(cx);
        let wanted = rendered_size_at((1., 100.), zoom * scale_factor);
        let drawn = viewer.read_with(cx, |viewer, _| viewer.rendered_size().unwrap());
        assert!(
            drawn.0 >= wanted.0 && drawn.1 >= wanted.1,
            "{zoom}: {drawn:?} < {wanted:?}"
        );
    }
}
