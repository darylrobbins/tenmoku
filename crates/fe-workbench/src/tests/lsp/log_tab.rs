//! The language server log tab: titled, read-only, and saying why when the
//! server never wrote anything.

use fe_lsp::ServerStatus;
use gpui::TestAppContext;

use crate::tests::lsp::Lsp;

fn tab_count(s: &mut Lsp) -> usize {
    s.workbench
        .read_with(&s.cx, |w, cx| w.pane().read(cx).tabs().len())
}

fn active_title(s: &mut Lsp) -> String {
    s.workbench.read_with(&s.cx, |w, cx| {
        let pane = w.pane().read(cx);
        pane.active_item().unwrap().tab_title(cx).to_string()
    })
}

#[gpui::test]
fn a_stopped_servers_log_tab_is_titled_and_says_why(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.open("tool.py");
    s.wait_item(Some("pyright: starting…"));
    s.until("the buffer's server is admitted", |s| {
        s.clock.advance(std::time::Duration::from_millis(20));
        s.cx.executor()
            .advance_clock(std::time::Duration::from_millis(250));
        s.workbench.read_with(&s.cx, |w, cx| {
            let buffer = w
                .active_editor(cx)
                .map(|e| e.read(cx).model().buffer().id());
            buffer.is_some_and(|b| w.lsp_code_actioner(b).is_some())
        })
    });
    s.workbench.update(&mut s.cx, |w, cx| {
        let status = ServerStatus::Stopped(Some("pyright-langserver not found".into()));
        w.force_lsp_status(status, None, cx);
    });
    let before = tab_count(&mut s);

    s.run("lsp.show_log");
    assert_eq!(tab_count(&mut s), before + 1);
    assert_eq!(active_title(&mut s), "pyright Log");
    let editor = s.active_editor();
    let (read_only, text) = editor.read_with(&s.cx, |view, _| {
        (view.is_read_only(), view.model().text().to_string())
    });
    assert!(read_only);
    assert!(text.contains("not found"), "{text}");
    assert!(text.contains(&s.root.display().to_string()), "{text}");

    // Again: the same tab, its text fresh.
    s.open("tool.py");
    s.workbench.update(&mut s.cx, |w, cx| {
        w.force_lsp_status(ServerStatus::Stopped(Some("still gone".into())), None, cx);
    });
    s.run("lsp.show_log");
    assert_eq!(tab_count(&mut s), before + 1);
    assert_eq!(active_title(&mut s), "pyright Log");
    let editor = s.active_editor();
    let text = editor.read_with(&s.cx, |view, _| view.model().text().to_string());
    assert!(text.contains("still gone"), "{text}");
    // A refresh isn't an edit: closing the tab asks nothing.
    assert!(!editor.read_with(&s.cx, |view, _| view.model().is_dirty()));
}

fn all_tabs(s: &mut Lsp) -> usize {
    s.workbench.read_with(&s.cx, |w, cx| {
        w.panes(cx)
            .iter()
            .map(|pane| pane.read(cx).tabs().len())
            .sum()
    })
}

#[gpui::test]
fn a_log_open_in_another_split_is_reused(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.open("tool.py");
    s.wait_item(Some("pyright: starting…"));
    s.until("the buffer's server is admitted", |s| {
        s.clock.advance(std::time::Duration::from_millis(20));
        s.cx.executor()
            .advance_clock(std::time::Duration::from_millis(250));
        s.workbench.read_with(&s.cx, |w, cx| {
            let buffer = w
                .active_editor(cx)
                .map(|e| e.read(cx).model().buffer().id());
            buffer.is_some_and(|b| w.lsp_code_actioner(b).is_some())
        })
    });
    s.workbench.update(&mut s.cx, |w, cx| {
        let status = ServerStatus::Stopped(Some("gone".into()));
        w.force_lsp_status(status, None, cx);
    });
    s.run("lsp.show_log");
    s.open("tool.py");
    s.run("pane.split_right");
    let panes = s.workbench.read_with(&s.cx, |w, cx| w.panes(cx).len());
    assert_eq!(panes, 2);
    let before = all_tabs(&mut s);
    s.workbench.update(&mut s.cx, |w, cx| {
        let status = ServerStatus::Stopped(Some("still gone".into()));
        w.force_lsp_status(status, None, cx);
    });
    s.run("lsp.show_log");
    assert_eq!(all_tabs(&mut s), before);
    assert_eq!(active_title(&mut s), "pyright Log");
    let text = s
        .active_editor()
        .read_with(&s.cx, |v, _| v.model().text().to_string());
    assert!(text.contains("still gone"), "{text}");
}
