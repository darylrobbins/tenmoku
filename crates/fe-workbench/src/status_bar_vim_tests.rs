//! The status bar's vim mode item, which shows only with vim on.

use fe_gpui::{EditorSettings, EditorTheme, set_vim_enabled};
use gpui::{Focusable as _, SharedString, TestAppContext, VisualTestContext};

use crate::{Launch, StatusItem, StatusSide, Workbench};

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn shows_the_vim_mode(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "one\ntwo\n").unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    let drawn = |cx: &mut VisualTestContext| {
        bar.read_with(cx, |bar, _| {
            (bar.drawn(StatusSide::Left), bar.drawn(StatusSide::Right))
        })
    };
    let vim_mode = |text: &str| vec![(SharedString::from(StatusItem::VIM_MODE), text.into())];

    let (left, right) = drawn(cx);
    assert_eq!(left, []);
    assert!(right.iter().any(|(id, _)| id == StatusItem::CURSOR));

    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    assert_eq!(drawn(cx), (vim_mode("NORMAL"), right.clone()));

    cx.simulate_keystrokes("d");
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .unwrap();
    let mode = editor.read_with(cx, |editor, _| editor.vim_mode());
    assert_eq!(mode, Some("operator_pending"));
    assert_eq!(drawn(cx), (vim_mode("NORMAL"), right.clone()));

    cx.update(|_, cx| set_vim_enabled(cx, false));
    draw(cx);
    assert_eq!(drawn(cx), (vec![], right));
}
