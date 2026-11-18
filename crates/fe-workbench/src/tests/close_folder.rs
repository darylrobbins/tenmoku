//! File > Close Folder (`workbench.close_folder`): every tab closes, pinned
//! ones too, asking about unsaved ones as Close All Editors does, then the
//! window has no folder: the Start page, the project panel's No folder
//! open, nothing for quick open, git or the settings watch, and the root's
//! language servers shut down.

use std::{collections::BTreeSet, fs, path::Path};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_lsp::fake::Record;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{lsp::Lsp, open_root};
use crate::{DockPanel, RootClosed, Workbench};

/// A folder of `a.txt` and `b.txt`.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.txt", "b.txt"] {
        fs::write(dir.path().join(name), format!("{name}\n")).unwrap();
    }
    dir
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// A workbench on `root`, focused, with `names` open in its pane in order.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    names: &[&str],
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, root);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    for name in names {
        let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
        let path = root.join(name);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    (workbench, cx)
}

/// Each pane's tabs' file names.
fn tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.panes(cx).iter())
            .map(|pane| {
                (pane.read(cx).tabs().iter())
                    .map(|tab| fe_gpui::pane_item::file_name(tab.path.as_deref()))
                    .collect()
            })
            .collect()
    })
}

/// How many [`RootClosed`] the workbench emits from now.
fn count_closed(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (std::rc::Rc<std::cell::Cell<usize>>, gpui::Subscription) {
    let closed = std::rc::Rc::new(std::cell::Cell::new(0));
    let subscription = cx.update(|_, cx| {
        let closed = closed.clone();
        cx.subscribe(workbench, move |_, RootClosed, _| {
            closed.set(closed.get() + 1)
        })
    });
    (closed, subscription)
}

/// What quick open lists for the empty query.
fn quick_open_paths(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> BTreeSet<String> {
    cx.simulate_keystrokes("cmd-p");
    draw(cx);
    let picker = workbench.read_with(cx, |workbench, _| workbench.quick_open().clone());
    let paths = picker.read_with(cx, |picker, _| {
        let matches = picker.delegate.matches();
        matches.iter().map(|found| found.path.clone()).collect()
    });
    cx.simulate_keystrokes("escape");
    draw(cx);
    paths
}

/// Whether the git state reads a root's status when asked.
fn git_refreshes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let git = workbench.read_with(cx, |workbench, _| workbench.git().clone());
    let started = git.update(cx, |git, cx| {
        git.refresh(cx);
        git.is_running()
    });
    draw(cx);
    started
}

#[gpui::test]
fn close_folder_returns_to_no_folder(cx: &mut TestAppContext) {
    let dir = project();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let (workbench, cx) = opened(cx, &root, &["a.txt", "b.txt"]);
    run("workbench.pin_tab", cx);
    run("dock.show_project", cx);
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    let state = |cx: &mut VisualTestContext| {
        dock.read_with(cx, |dock, _| (dock.is_visible(), dock.active()))
    };
    assert_eq!(state(cx), (true, DockPanel::Explorer));
    let pinned = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        (0..pane.tabs().len())
            .filter(|ix| pane.is_pinned(*ix))
            .count()
    });
    assert_eq!(pinned, 1, "b.txt is pinned");
    assert_eq!(
        tabs(&workbench, cx),
        [["b.txt", "a.txt"]],
        "pinned, b.txt leads"
    );
    assert_eq!(
        quick_open_paths(&workbench, cx),
        BTreeSet::from(["a.txt".to_owned(), "b.txt".to_owned()])
    );
    assert!(git_refreshes(&workbench, cx), "the root's status is read");
    assert!(workbench.read_with(cx, |workbench, _| workbench.watches_root_settings()));
    let old_panel = workbench.read_with(cx, |workbench, _| workbench.panel().downgrade());
    let (closed, _subscription) = count_closed(&workbench, cx);

    run("workbench.close_folder", cx);
    assert_eq!(cx.pending_prompt(), None, "nothing unsaved to ask about");
    assert_eq!(closed.get(), 1, "emitted once");
    assert!(workbench.read_with(cx, |workbench, _| workbench.root().is_none()));
    assert_eq!(tabs(&workbench, cx), [Vec::<String>::new()], "pinned too");
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert!(pane.read_with(cx, |pane, _| pane.shown_start_page().is_some()));
    assert!(
        cx.debug_bounds("start-page").is_some(),
        "the Start page drew"
    );

    // The project panel, still shown, has no folder: no tree, so no
    // watcher either.
    assert_eq!(state(cx), (true, DockPanel::Explorer));
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    let (tree, words) = panel.read_with(cx, |panel, cx| {
        (panel.tree().is_some(), panel.shown_words(cx))
    });
    assert!(!tree, "no tree, no watcher");
    assert!(
        words.contains(&("message", "No folder open".to_owned())),
        "{words:?}"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    assert!(
        old_panel.upgrade().is_none(),
        "the old tree and its watcher"
    );

    let title = workbench.read_with(cx, |workbench, cx| {
        workbench.shell().read(cx).title().clone()
    });
    assert_eq!(title.as_ref(), fe_config::APP_NAME);
    assert_eq!(quick_open_paths(&workbench, cx), BTreeSet::new());
    assert!(!git_refreshes(&workbench, cx), "no root for git");
    assert!(!workbench.read_with(cx, |workbench, _| workbench.watches_root_settings()));

    // Again, with no folder: nothing.
    run("workbench.close_folder", cx);
    assert_eq!(closed.get(), 1);
    assert_eq!(cx.pending_prompt(), None);
}

#[gpui::test]
fn a_dirty_tab_asks_first(cx: &mut TestAppContext) {
    let dir = project();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let (workbench, cx) = opened(cx, &root, &["a.txt", "b.txt"]);
    run("pane.split_right", cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .unwrap();
    editor.update(cx, |editor, cx| {
        editor.model_mut().insert("x");
        cx.notify();
    });
    draw(cx);
    let (closed, _subscription) = count_closed(&workbench, cx);
    let before = tabs(&workbench, cx);
    assert_eq!(before, [vec!["a.txt", "b.txt"], vec!["b.txt"]]);

    // As Close All Editors asks.
    run("workbench.close_folder", cx);
    let asked = cx.pending_prompt().map(|(question, _)| question);
    assert_eq!(asked.as_deref(), Some("Discard unsaved changes to b.txt?"));
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert_eq!(cx.pending_prompt(), None);
    assert_eq!(tabs(&workbench, cx), before, "every tab stays");
    assert_eq!(
        workbench.read_with(cx, |workbench, _| workbench.root().map(Path::to_owned)),
        Some(root.clone()),
        "and the folder"
    );
    assert_eq!(closed.get(), 0);

    run("workbench.close_folder", cx);
    cx.simulate_prompt_answer("Discard");
    draw(cx);
    assert_eq!(cx.pending_prompt(), None, "asked once for the buffer");
    assert_eq!(tabs(&workbench, cx), [Vec::<String>::new(), Vec::new()]);
    assert!(workbench.read_with(cx, |workbench, _| workbench.root().is_none()));
    assert_eq!(closed.get(), 1);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert!(pane.read_with(cx, |pane, _| pane.shown_start_page().is_some()));
    assert_eq!(
        fs::read_to_string(root.join("b.txt")).unwrap(),
        "b.txt\n",
        "discarded, not saved"
    );
}

/// Whether `records` has `shutdown`, then `exit`.
fn shut_down(records: &[Record]) -> bool {
    let shutdown = records.iter().position(|r| r.method() == "shutdown");
    let exit = records.iter().position(|r| r.is_notification("exit"));
    matches!((shutdown, exit), (Some(shutdown), Some(exit)) if shutdown < exit)
}

#[gpui::test]
fn the_roots_servers_stop(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.open("tool.py");
    s.python.wait_initialized();
    let root = s.root.clone();
    assert!(
        s.workbench
            .read_with(&s.cx, |w, _| w.lsp_manager_at(&root).is_some())
    );

    s.run("workbench.close_folder");
    s.python.wait_for_at(1, "shutdown, exit", shut_down);
    assert!(s.workbench.read_with(&s.cx, |w, _| w.root().is_none()));
    // Nothing starts again for the root.
    for _ in 0..20 {
        s.draw();
    }
    assert_eq!(s.python.launches(), 1);
    assert!(
        s.workbench
            .read_with(&s.cx, |w, _| w.lsp_manager_at(&root).is_none())
    );
}
