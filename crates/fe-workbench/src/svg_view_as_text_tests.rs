//! An `.svg` opens in the image viewer, and `image.open_as_text` and
//! `image.open_as_image` swap its tab for an editor on the file and back,
//! in place, keeping the zoom.

use std::path::{Path, PathBuf};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{ImageZoom, commands::RunCommand};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use crate::{Launch, Pane, Workbench};

const SVG_40_BY_30: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect width="40" height="30" fill="red"/></svg>"#;
const SVG_20_BY_10: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="blue"/></svg>"#;

/// A valid 1 × 1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x38, 0x21, 0x27, 0xf7,
    0x1f, 0x00, 0x04, 0xba, 0x02, 0x04, 0x3e, 0x57, 0x1f, 0xc7, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// A project of `files` (name, contents).
fn project(files: &[(&str, &[u8])]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    for (name, bytes) in files {
        std::fs::write(root.join(name), bytes).unwrap();
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

/// Each tab of `pane`: its path, and whether it's an image viewer (else an
/// editor).
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

/// Runs `command` as a key would in `frame` within the workbench.
fn run_in(command: &str, frame: &str, cx: &mut VisualTestContext) {
    // The viewer of an SVG is `svg` too.
    let svg = |frame: &str| match frame {
        "ImageViewer" => ContextFrame::new(frame).flag("svg"),
        _ => ContextFrame::new(frame),
    };
    let context: ContextStack = ["workbench", frame].into_iter().map(svg).collect();
    let run = RunCommand(Invocation::of(command).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn active(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Option<usize> {
    pane.read_with(cx, |pane, _| pane.active())
}

#[gpui::test]
fn svg_opens_in_the_viewer(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("logo.svg", SVG_40_BY_30.as_bytes())]);
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);
    click_row(&workbench, "logo.svg", cx);
    draw(cx);
    assert_eq!(tabs(&pane, cx), [(root.join("logo.svg"), true)]);
    pane.read_with(cx, |pane, cx| {
        let viewer = pane.tabs()[0].image_viewer().unwrap().read(cx);
        assert_eq!(viewer.message(), None);
        assert_eq!(viewer.pixel_size(), Some((40, 30)));
    });
    assert!(workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).is_none()));
}

#[gpui::test]
fn open_as_text_replaces_the_tab_in_place_with_an_editor(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[
        ("a.txt", b"a\n"),
        ("logo.svg", SVG_40_BY_30.as_bytes()),
        ("z.txt", b"z\n"),
    ]);
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);
    click_row(&workbench, "a.txt", cx);
    click_row(&workbench, "logo.svg", cx);
    click_row(&workbench, "z.txt", cx);
    cx.update(|window, cx| {
        let path = root.join("logo.svg");
        pane.update(cx, |pane, cx| pane.open(path, window, cx))
    });
    draw(cx);
    assert_eq!(active(&pane, cx), Some(1));

    run_in("image.open_as_text", "ImageViewer", cx);
    assert_eq!(
        tabs(&pane, cx),
        [
            (root.join("a.txt"), false),
            (root.join("logo.svg"), false),
            (root.join("z.txt"), false)
        ]
    );
    assert_eq!(active(&pane, cx), Some(1));
    pane.read_with(cx, |pane, cx| {
        let editor = pane.tabs()[1].editor().unwrap().read(cx);
        assert_eq!(editor.model().text().to_string(), SVG_40_BY_30);
    });
}

#[gpui::test]
fn open_as_image_goes_back_keeping_the_zoom(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("logo.svg", SVG_40_BY_30.as_bytes()), ("z.txt", b"z\n")]);
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);
    click_row(&workbench, "logo.svg", cx);
    draw(cx);
    run_in("image.zoom_actual", "ImageViewer", cx);
    let zoom = pane.read_with(cx, |pane, cx| {
        pane.tabs()[0].image_viewer().unwrap().read(cx).zoom()
    });
    assert_eq!(zoom, ImageZoom::Scale(1.));

    run_in("image.open_as_text", "ImageViewer", cx);
    assert_eq!(tabs(&pane, cx), [(root.join("logo.svg"), false)]);
    run_in("image.open_as_image", "editor", cx);
    assert_eq!(tabs(&pane, cx), [(root.join("logo.svg"), true)]);
    pane.read_with(cx, |pane, cx| {
        let viewer = pane.tabs()[0].image_viewer().unwrap().read(cx);
        assert_eq!(viewer.zoom(), ImageZoom::Scale(1.));
    });

    // Never an image: the other direction does nothing on a text file.
    click_row(&workbench, "z.txt", cx);
    run_in("image.open_as_image", "editor", cx);
    assert_eq!(
        tabs(&pane, cx),
        [(root.join("logo.svg"), true), (root.join("z.txt"), false)]
    );
}

#[gpui::test]
fn commands_do_nothing_on_a_png(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("logo.png", PNG), ("a.txt", b"a\n")]);
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);
    click_row(&workbench, "logo.png", cx);
    draw(cx);
    let before = tabs(&pane, cx);
    run_in("image.open_as_text", "ImageViewer", cx);
    assert_eq!(tabs(&pane, cx), before);
    assert_eq!(before, [(root.join("logo.png"), true)]);
    run_in("image.open_as_image", "ImageViewer", cx);
    assert_eq!(tabs(&pane, cx), before);

    click_row(&workbench, "a.txt", cx);
    let before = tabs(&pane, cx);
    run_in("image.open_as_image", "editor", cx);
    run_in("image.open_as_text", "editor", cx);
    assert_eq!(tabs(&pane, cx), before);

    // Pane::open_as_text is for the command only, but takes any path.
    cx.update(|window, cx| {
        let path = root.join("logo.png");
        pane.update(cx, |pane, cx| pane.open_as_text(path, window, cx))
    });
    draw(cx);
    assert_eq!(
        tabs(&pane, cx),
        [(root.join("logo.png"), false), (root.join("a.txt"), false)]
    );
}

#[gpui::test]
fn editing_the_svg_text_and_saving_updates_the_viewer_on_return(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("logo.svg", SVG_40_BY_30.as_bytes())]);
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);
    click_row(&workbench, "logo.svg", cx);
    draw(cx);
    run_in("image.open_as_text", "ImageViewer", cx);

    let editor = pane.read_with(cx, |pane, _| pane.tabs()[0].editor().unwrap().clone());
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(SVG_20_BY_10);
    draw(cx);
    // Unsaved changes keep the editor.
    run_in("image.open_as_image", "editor", cx);
    assert_eq!(tabs(&pane, cx), [(root.join("logo.svg"), false)]);

    cx.simulate_keystrokes("cmd-s");
    draw(cx);
    assert_eq!(
        std::fs::read_to_string(root.join("logo.svg")).unwrap(),
        SVG_20_BY_10
    );
    run_in("image.open_as_image", "editor", cx);
    assert_eq!(tabs(&pane, cx), [(root.join("logo.svg"), true)]);
    draw(cx);
    pane.read_with(cx, |pane, cx| {
        let viewer = pane.tabs()[0].image_viewer().unwrap().read(cx);
        assert_eq!(viewer.pixel_size(), Some((20, 10)));
    });
}

fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

#[gpui::test]
fn a_read_for_a_closed_tab_does_not_open_an_editor(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("logo.svg", SVG_40_BY_30.as_bytes())]);
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);
    click_row(&workbench, "logo.svg", cx);
    draw(cx);
    let path = root.join("logo.svg");
    // Asked for, and the tab closed before the read lands.
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.open_as_text(path.clone(), window, cx);
            pane.close_active(window, cx);
        })
    });
    draw(cx);
    assert_eq!(tabs(&pane, cx), []);
}

#[gpui::test]
fn repeated_requests_and_the_way_back_leave_one_tab(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("logo.svg", SVG_40_BY_30.as_bytes())]);
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);
    click_row(&workbench, "logo.svg", cx);
    draw(cx);
    let path = root.join("logo.svg");
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.open_as_text(path.clone(), window, cx);
            pane.open_as_text(path.clone(), window, cx);
        })
    });
    draw(cx);
    assert_eq!(tabs(&pane, cx), [(path.clone(), false)]);
    // Back to the image with a read for the text pending: it is dropped.
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.open_as_image(&path, window, cx);
        })
    });
    draw(cx);
    assert_eq!(tabs(&pane, cx), [(path, true)]);
}

#[gpui::test]
fn an_svg_opens_in_the_viewer_while_the_other_pane_has_it_as_text(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("logo.svg", SVG_40_BY_30.as_bytes())]);
    let (workbench, cx) = open_root(cx, &root);
    let first = pane(&workbench, cx);
    click_row(&workbench, "logo.svg", cx);
    draw(cx);
    run_in("image.open_as_text", "ImageViewer", cx);
    dispatch("pane.split_right", cx);
    let panes = workbench.read_with(cx, |workbench, cx| workbench.panes(cx));
    assert_eq!(panes.len(), 2);
    let second = panes.iter().find(|pane| **pane != first).unwrap().clone();
    // Close the split's copy, then open the file again there.
    cx.update(|window, cx| second.update(cx, |pane, cx| pane.close_active(window, cx)));
    draw(cx);
    let path = root.join("logo.svg");
    cx.update(|window, cx| second.update(cx, |pane, cx| pane.open(path.clone(), window, cx)));
    draw(cx);
    assert_eq!(tabs(&second, cx), [(path.clone(), true)]);
    assert_eq!(tabs(&first, cx), [(path, false)]);
}

#[gpui::test]
fn the_commands_apply_in_an_svg_viewer_and_not_a_png_one(cx: &mut TestAppContext) {
    use fe_command::ContextPredicate;
    let applies = ContextPredicate::parse("ImageViewer && svg").unwrap();
    let svg: ContextStack = [ContextFrame::new("ImageViewer").flag("svg")]
        .into_iter()
        .collect();
    let png: ContextStack = [ContextFrame::new("ImageViewer")].into_iter().collect();
    assert!(applies.eval(&svg));
    assert!(!applies.eval(&png));
    // The context a viewer really has, from its key context.
    let (_dir, root) = project(&[("logo.svg", SVG_40_BY_30.as_bytes()), ("logo.png", PNG)]);
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);
    for (name, svg) in [("logo.svg", true), ("logo.png", false)] {
        click_row(&workbench, name, cx);
        draw(cx);
        let viewer = pane.read_with(cx, |pane, _| {
            pane.tabs()[pane.active().unwrap()]
                .image_viewer()
                .unwrap()
                .clone()
        });
        cx.update(|window, cx| window.focus(&viewer.focus_handle(cx), cx));
        draw(cx);
        let context = cx.update(|window, _| window.context_stack());
        let has = context.iter().any(|key| key.contains("svg"));
        assert_eq!(has, svg, "{name}");
    }
}

#[gpui::test]
fn an_svgz_has_no_text_to_switch_to(cx: &mut TestAppContext) {
    // Gzip of an empty stored block: enough to draw nothing; it is the name
    // that decides.
    let (_dir, root) = project(&[("logo.svgz", &[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff])]);
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);
    click_row(&workbench, "logo.svgz", cx);
    draw(cx);
    let before = tabs(&pane, cx);
    assert_eq!(before, [(root.join("logo.svgz"), true)]);
    run_in("image.open_as_text", "ImageViewer", cx);
    assert_eq!(tabs(&pane, cx), before);
}
