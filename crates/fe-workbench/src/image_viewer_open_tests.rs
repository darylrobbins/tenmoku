//! Image files open in an [`fe_gpui::ImageViewer`] tab, never an editor:
//! from the project panel, quick open and [`Pane::open`]. Other files open
//! as they did.

use std::path::{Path, PathBuf};

use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::{Opened, is_image, open};
use crate::{Launch, Pane, Workbench};

/// A valid 1 × 1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x38, 0x21, 0x27, 0xf7,
    0x1f, 0x00, 0x04, 0xba, 0x02, 0x04, 0x3e, 0x57, 0x1f, 0xc7, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// A project of `images` (PNG bytes) and `texts`.
fn project(images: &[&str], texts: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    for name in images {
        std::fs::write(root.join(name), PNG).unwrap();
    }
    for name in texts {
        std::fs::write(root.join(name), format!("{name}\n")).unwrap();
    }
    (dir, root)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn open_root<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx)
}

fn pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

/// Each tab of `pane`: its path, and whether it's an image viewer. A tab
/// is one or the other, or an editor.
fn tabs(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Vec<(PathBuf, bool)> {
    pane.read_with(cx, |pane, _| {
        (pane.tabs().iter())
            .map(|tab| {
                assert!(
                    tab.image_viewer().is_some() != tab.editor().is_some(),
                    "{:?} is an image viewer or an editor",
                    tab.path
                );
                (tab.path.clone().unwrap(), tab.image_viewer().is_some())
            })
            .collect()
    })
}

fn click_row(workbench: &Entity<Workbench>, name: &str, cx: &mut VisualTestContext) {
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    let ix = panel.read_with(cx, |panel, _| {
        let tree = panel.tree().expect("the root is open");
        (tree.rows(0..tree.row_count()))
            .position(|row| row.name.to_string_lossy() == name)
            .unwrap_or_else(|| panic!("no row {name}"))
    });
    let bounds = panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

#[test]
fn the_extension_picks_the_viewer_in_any_case() {
    for name in [
        "a.png",
        "a.jpg",
        "a.jpeg",
        "a.gif",
        "a.webp",
        "a.bmp",
        "a.ico",
        "A.PNG",
        "a.JpG",
        "dir.d/a.Webp",
        "a.svg",
        "a.SVGZ",
    ] {
        assert!(is_image(Path::new(name)), "{name}");
    }
    for name in ["a.txt", "a.svg.txt", "png", "a.png.txt", "a", "a.pn"] {
        assert!(!is_image(Path::new(name)), "{name}");
    }
    // Not read: even a missing file is an image to view.
    assert!(matches!(
        open(PathBuf::from("/no/such/dir/missing.png")),
        Ok(Opened::Image)
    ));
}

#[gpui::test]
fn opening_a_png_makes_an_image_tab_not_an_editor(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["logo.png"], &[]);
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);
    click_row(&workbench, "logo.png", cx);
    assert_eq!(tabs(&pane, cx), [(root.join("logo.png"), true)]);
    assert!(workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).is_none()));
    pane.read_with(cx, |pane, cx| {
        let viewer = pane.tabs()[0].image_viewer().unwrap().read(cx);
        assert_eq!(viewer.path(), root.join("logo.png"));
        // Never dirty.
        assert!(!pane.tabs()[0].item().is_dirty(cx));
    });
    // Decoded off the main thread.
    draw(cx);
    pane.read_with(cx, |pane, cx| {
        let viewer = pane.tabs()[0].image_viewer().unwrap().read(cx);
        assert_eq!(viewer.pixel_size(), Some((1, 1)));
    });
}

#[gpui::test]
fn an_uppercase_extension_opens_the_viewer(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["PHOTO.PNG", "scan.JpG"], &[]);
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);
    click_row(&workbench, "PHOTO.PNG", cx);
    click_row(&workbench, "scan.JpG", cx);
    assert_eq!(
        tabs(&pane, cx),
        [
            (root.join("PHOTO.PNG"), true),
            (root.join("scan.JpG"), true)
        ]
    );
}

#[gpui::test]
fn a_txt_file_still_opens_in_an_editor(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[], &["notes.txt"]);
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);
    click_row(&workbench, "notes.txt", cx);
    assert_eq!(tabs(&pane, cx), [(root.join("notes.txt"), false)]);
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    editor.expect("an editor shows").read_with(cx, |editor, _| {
        assert_eq!(editor.model().text().to_string(), "notes.txt\n");
    });
}

#[gpui::test]
fn reopening_activates_the_same_tab(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["logo.png"], &["notes.txt"]);
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);
    click_row(&workbench, "logo.png", cx);
    click_row(&workbench, "notes.txt", cx);
    assert_eq!(pane.read_with(cx, |pane, _| pane.active()), Some(1));

    let path = root.join("logo.png");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.clone(), window, cx)));
    draw(cx);
    assert_eq!(
        tabs(&pane, cx),
        [(path, true), (root.join("notes.txt"), false)]
    );
    assert_eq!(pane.read_with(cx, |pane, _| pane.active()), Some(0));
}

#[gpui::test]
fn quick_open_and_the_tree_open_images_in_the_viewer(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["tree.png", "quick.png"], &[]);
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);
    click_row(&workbench, "tree.png", cx);

    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("quick.png");
    let picker = crate::tests::quick_open::picker(&workbench, cx);
    assert_eq!(crate::tests::quick_open::paths(&picker, cx)[0], "quick.png");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        tabs(&pane, cx),
        [
            (root.join("tree.png"), true),
            (root.join("quick.png"), true)
        ]
    );
    assert!(workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).is_none()));
}
