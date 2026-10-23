//! Open editors follow the host's palette ([`fe_gpui::ActivePalette`]) as
//! it changes, and those opened later start in it, in every pane.

use std::{fs, path::Path, sync::Arc};

use fe_buffer::Buffer;
use fe_config::theme::EditorPalette;
use fe_gpui::{EditorTheme, EditorView};
use fe_syntax::Highlight;
use gpui::{Entity, Hsla, TestAppContext, VisualTestContext};

use crate::{Direction, Launch, Pane, Workbench, tests::open};

/// Draws a frame once the work queued has run.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// An editor's background and keyword colour.
fn colors(theme: &EditorTheme) -> (Hsla, Hsla) {
    let keyword = Highlight::from_capture("keyword").expect("a highlight");
    let keyword = theme.syntax_color(keyword).expect("a keyword colour");
    (theme.background, keyword)
}

/// The colours of every editor open, in every pane.
fn open_colors(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(Hsla, Hsla)> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.open_editors(cx).iter())
            .map(|editor| colors(editor.read(cx).theme()))
            .collect()
    })
}

/// Opens `path` in `pane`, returning its editor once read.
fn open_in(pane: &Entity<Pane>, path: &Path, cx: &mut VisualTestContext) -> Entity<EditorView> {
    let opened = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(opened, window, cx)));
    draw(cx);
    pane.read_with(cx, |pane, cx| {
        (pane.tabs().iter())
            .filter_map(|tab| tab.editor())
            .find(|editor| editor.read(cx).model().path() == Some(path))
            .cloned()
            .expect("open")
    })
}

#[gpui::test]
fn open_editors_follow_the_palette(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.rs"), dir.path().join("b.rs"));
    fs::write(&a, "fn a() {}\n").unwrap();
    fs::write(&b, "fn b() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((a, Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    draw(cx);
    let left = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.split(Direction::Right, window, cx)
    });
    draw(cx);
    let right = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_ne!(left, right, "a split");

    let (dark, light) = cx.update(|_, cx| {
        let dark = EditorTheme::default_dark(cx);
        let mut light = dark.clone();
        light.set_colors(EditorPalette::fastedit_light());
        (colors(&dark), colors(&light))
    });
    assert_ne!(dark.0, light.0, "the backgrounds differ");
    assert_ne!(dark.1, light.1, "the keyword colours differ");
    assert_eq!(open_colors(&workbench, cx), [dark, dark]);

    cx.update(|_, cx| {
        let palette = Arc::new(EditorPalette::fastedit_light().clone());
        fe_gpui::set_editor_palette(cx, palette);
    });
    draw(cx);
    assert_eq!(open_colors(&workbench, cx), [light, light], "recoloured");

    // Editors opened since, in the first pane and in its split, start in it.
    for pane in [&left, &right] {
        let editor = open_in(pane, &b, cx);
        let opened = editor.read_with(cx, |editor, _| colors(editor.theme()));
        assert_eq!(opened, light);
    }
}
