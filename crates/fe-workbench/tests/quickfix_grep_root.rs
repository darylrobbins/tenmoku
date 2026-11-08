//! A `:grep` still searching when the folder switches is dropped: the old
//! project's results don't reach the new window. An empty `:grep` with no
//! previous search is E35.

use std::{fs, path::Path};

use fe_gpui::{EditorSettings, EditorTheme, set_vim_enabled};
use fe_workbench::{Launch, Workbench};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn window(cx: &mut TestAppContext, root: &Path) -> (Entity<Workbench>, VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let cx = &mut vcx;
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = root.join("a.txt");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    (workbench, vcx)
}

fn caret_row(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> u32 {
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a tab is shown");
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let head = model.primary_selection().head();
        model.snapshot().text().offset_to_point(head).row
    })
}

#[gpui::test]
fn a_root_change_mid_search_leaves_the_list_alone(cx: &mut TestAppContext) {
    let (old, new) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    fs::write(old.path().join("a.txt"), "1\n2\nneedle\n").unwrap();
    fs::write(new.path().join("b.txt"), "x\n").unwrap();
    let (workbench, mut vcx) = window(cx, old.path());
    let cx = &mut vcx;
    cx.simulate_keystrokes(":");
    cx.simulate_input("grep needle");
    // Enter without parking, so the search is still running below.
    cx.update(|window, cx| window.dispatch_keystroke(gpui::Keystroke::parse("enter").unwrap(), cx));
    // The search is running: switch folders before it is heard.
    let root = new.path().to_owned();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(root, window, cx))
    });
    draw(cx);
    assert_eq!(
        caret_row(&workbench, cx),
        0,
        "the old project's match was not opened"
    );
}

fn message(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a tab is shown");
    editor.read_with(cx, |editor, _| {
        editor
            .vim_state()
            .and_then(|vim| vim.message().map(String::from))
    })
}

#[gpui::test]
fn an_empty_grep_without_a_previous_search_is_e35(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "1\nneedle\n").unwrap();
    for line in ["grep", "lgrep"] {
        let (workbench, mut vcx) = window(cx, dir.path());
        let cx = &mut vcx;
        cx.simulate_keystrokes(":");
        cx.simulate_input(line);
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert_eq!(caret_row(&workbench, cx), 0, ":{line}");
        let said = message(&workbench, cx).unwrap_or_default();
        assert!(said.starts_with("E35"), ":{line}: {said:?}");
    }
}

#[gpui::test]
fn an_empty_grep_searches_for_the_last_search(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "1\n2\nneedle\n").unwrap();
    let (workbench, mut vcx) = window(cx, dir.path());
    let cx = &mut vcx;
    // A search leaves the caret on the match; go back to the top first.
    cx.simulate_keystrokes("/");
    cx.simulate_input("needle");
    cx.simulate_keystrokes("enter");
    draw(cx);
    cx.simulate_keystrokes("g g");
    draw(cx);
    assert_eq!(caret_row(&workbench, cx), 0);
    cx.simulate_keystrokes(":");
    cx.simulate_input("grep");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(caret_row(&workbench, cx), 2);
}
