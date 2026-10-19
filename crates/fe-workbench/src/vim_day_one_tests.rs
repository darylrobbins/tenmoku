//! The day-one features' ex commands on the status bar's line: `:Ex`
//! reveals the editor's file in the project tree, `:copen` and `:cclose`
//! show and hide the Problems panel, `:oldfiles` opens Open Recent and
//! `:Replace` runs the project search's Replace All.

use gpui::{Focusable as _, TestAppContext, VisualTestContext};

use super::super::recent_picker::set_recent_file;
use super::super::vim_host_tests::{active, draw, ex, message, window, write};
use crate::dock::DockPanel;
use crate::workbench::Palette;

fn focused(handle: &impl gpui::Focusable, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| handle.focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn explore_and_problems(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/deep")).unwrap();
    write(root, &["top.txt", "src/deep/a.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["top.txt", "src/deep/a.txt"]);
    let cx = &mut vcx;
    let a = active(&workbench, cx);
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());

    // The tree, from the dock hidden: focused, with a.txt revealed and
    // selected.
    cx.update(|window, cx| dock.update(cx, |dock, cx| dock.show(DockPanel::Problems, window, cx)));
    cx.update(|window, cx| window.focus(&a.focus_handle(cx), cx));
    draw(cx);
    assert!(dock.read_with(cx, |dock, _| dock.is_visible()));
    ex("cclose", cx);
    draw(cx);
    assert!(!dock.read_with(cx, |dock, _| dock.is_visible()));
    let selected = |cx: &mut VisualTestContext| {
        panel.read_with(cx, |panel, _| {
            let tree = panel.tree().expect("the root is open");
            let row = panel.selected_row()?;
            let row = tree.rows(row..row + 1).next()?;
            Some(row.name.to_string_lossy().into_owned())
        })
    };
    // Some other row selected since a.txt was revealed.
    panel.update(cx, |panel, cx| panel.reveal(root.join("top.txt"), cx));
    draw(cx);
    assert_eq!(selected(cx).as_deref(), Some("top.txt"));
    ex("Ex", cx);
    draw(cx);
    assert!(dock.read_with(cx, |dock, _| dock.is_visible()));
    assert_eq!(
        dock.read_with(cx, |dock, _| dock.active()),
        DockPanel::Explorer
    );
    assert!(focused(&panel, cx));
    assert_eq!(selected(cx).as_deref(), Some("a.txt"));
    assert_eq!(message(&workbench, cx), None);

    // :copen from the editor: Problems shown, focused.
    cx.update(|window, cx| window.focus(&a.focus_handle(cx), cx));
    draw(cx);
    ex("copen", cx);
    draw(cx);
    let problems = dock.read_with(cx, |dock, _| dock.problems().clone());
    assert!(dock.read_with(cx, |dock, _| dock.is_visible()));
    assert_eq!(
        dock.read_with(cx, |dock, _| dock.active()),
        DockPanel::Problems
    );
    assert!(focused(&problems, cx));

    // :cclose hides it, the editor keeping focus.
    cx.update(|window, cx| window.focus(&a.focus_handle(cx), cx));
    draw(cx);
    ex("ccl", cx);
    draw(cx);
    assert!(!dock.read_with(cx, |dock, _| dock.is_visible()));
    assert!(focused(&a, cx));
    assert_eq!(a.read_with(cx, |a, _| a.vim_mode()), Some("normal"));

    // With another panel shown, :cclose leaves it.
    ex("Ex", cx);
    draw(cx);
    cx.update(|window, cx| window.focus(&a.focus_handle(cx), cx));
    draw(cx);
    ex("lclose", cx);
    draw(cx);
    assert!(dock.read_with(cx, |dock, _| dock.is_visible()));
    assert_eq!(
        dock.read_with(cx, |dock, _| dock.active()),
        DockPanel::Explorer
    );
    ex("lop", cx);
    draw(cx);
    assert_eq!(
        dock.read_with(cx, |dock, _| dock.active()),
        DockPanel::Problems
    );
}

#[gpui::test]
fn oldfiles(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &["a.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path(), &["a.txt"]);
    let cx = &mut vcx;
    cx.update(|_, cx| set_recent_file(cx, state.path().join("recent.toml")));
    let a = active(&workbench, cx);
    let palette = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.palette);

    for line in ["oldfiles", "bro ol"] {
        ex(line, cx);
        draw(cx);
        assert_eq!(palette(cx), Some(Palette::Recent), ":{line}");
        let picker = workbench.read_with(cx, |w, _| w.recent_picker().clone());
        assert!(focused(&picker, cx));
        // The picker opens in insert mode: one escape for its normal mode,
        // the next dismisses it.
        cx.simulate_keystrokes("escape");
        draw(cx);
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert_eq!(palette(cx), None);
        assert!(focused(&a, cx));
        assert_eq!(a.read_with(cx, |a, _| a.vim_mode()), Some("normal"));
    }
}

#[gpui::test]
fn replace(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (file, text) in [
        ("a.txt", "foo one\n"),
        ("b.txt", "foo foo\n"),
        ("c.txt", "none\n"),
    ] {
        std::fs::write(root.join(file), text).unwrap();
    }
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    let a = active(&workbench, cx);
    let search = workbench.read_with(cx, |w, cx| w.dock().read(cx).search().clone());

    // No search: nothing asked.
    ex("Replace bar", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("No search to replace")
    );
    assert!(cx.pending_prompt().is_none());

    // A search for foo: the question Replace All asks, and cancelled,
    // nothing is replaced.
    let input = search.read_with(cx, |search, _| search.input().clone());
    input.update(cx, |input, cx| input.set_text("foo", cx));
    search.update(cx, |search, cx| search.run(cx));
    draw(cx);
    cx.update(|window, cx| window.focus(&a.focus_handle(cx), cx));
    draw(cx);
    ex("Replace bar", cx);
    let question = cx.pending_prompt().map(|(question, _)| question);
    assert_eq!(
        question.as_deref(),
        Some("Replace 3 occurrences across 2 files with “bar”?")
    );
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert_eq!(
        std::fs::read_to_string(root.join("b.txt")).unwrap(),
        "foo foo\n"
    );
    assert_eq!(
        a.read_with(cx, |a, _| a.model().text().to_string()),
        "foo one\n"
    );

    // Confirmed, every result file: the open one in its tab, the other
    // on disk.
    ex("Replace bar", cx);
    assert!(cx.pending_prompt().is_some());
    cx.simulate_prompt_answer("Replace");
    draw(cx);
    assert_eq!(
        std::fs::read_to_string(root.join("b.txt")).unwrap(),
        "bar bar\n"
    );
    assert_eq!(
        a.read_with(cx, |a, _| a.model().text().to_string()),
        "bar one\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("c.txt")).unwrap(),
        "none\n"
    );
    let field = search.read_with(cx, |search, _| search.replace_input().clone());
    assert_eq!(
        field.read_with(cx, |field, _| field.text().to_owned()),
        "bar"
    );
}
