//! File > Open Folder… and Open File…: the system's open panel, answered
//! with `simulate_path_prompt_response`. A folder re-roots the workbench in
//! place; a file opens in the active pane, the root staying.

use std::{
    cell::RefCell,
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::Ordering,
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_project::FsChange;
use gpui::{
    Entity, Focusable as _, Keystroke, PathPromptOptions, Subscription, TestAppContext,
    VisualTestContext,
};

use super::{click_row, open_root, panel, rows};
use crate::{DockPanel, RootChanged, SearchPanel, Workbench, search_panel::ShownRow};

/// Two projects, each with a file of its own, a text file holding
/// `needle`, and `shared.txt`.
fn roots() -> (tempfile::TempDir, tempfile::TempDir) {
    let project = |name: &str| {
        let dir = tempfile::tempdir().unwrap();
        let files = [
            (format!("{name}.rs"), format!("fn {name}() {{}}\n")),
            (format!("notes_{name}.txt"), "a needle\n".to_owned()),
            ("shared.txt".to_owned(), "needle again\n".to_owned()),
        ];
        for (file, text) in files {
            fs::write(dir.path().join(file), text).unwrap();
        }
        dir
    };
    (project("a"), project("b"))
}

/// A workbench rooted at `root`, focused as the app focuses it, and the
/// roots it reports changing to.
fn workbench_at<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (
    Entity<Workbench>,
    Rc<RefCell<Vec<PathBuf>>>,
    Subscription,
    &'a mut VisualTestContext,
) {
    let (workbench, cx) = open_root(cx, root);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let changed = Rc::new(RefCell::new(Vec::new()));
    let subscription = cx.update(|_, cx| {
        let changed = changed.clone();
        cx.subscribe(&workbench, move |_, RootChanged(root), _| {
            changed.borrow_mut().push(root.clone())
        })
    });
    cx.run_until_parked();
    (workbench, changed, subscription, cx)
}

/// Answers the pending open panel with `answer`, and returns the options
/// it was shown with.
fn answer(answer: Option<&Path>, cx: &mut VisualTestContext) -> PathPromptOptions {
    assert!(cx.did_prompt_for_paths(), "the open panel shows");
    let shown = Rc::new(RefCell::new(None));
    cx.simulate_path_prompt_response({
        let shown = shown.clone();
        move |options| {
            shown.replace(Some(options.clone()));
            answer.map(|path| vec![path.to_owned()])
        }
    });
    cx.run_until_parked();
    shown.take().expect("answered")
}

fn assert_shows(options: &PathPromptOptions, files: bool, directories: bool) {
    assert_eq!(
        (options.files, options.directories, options.multiple),
        (files, directories, false)
    );
    assert_eq!(options.prompt.as_deref(), Some("Open"));
}

fn search(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<SearchPanel> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).search().clone()
    })
}

/// The files the search panel shows results in.
fn result_files(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> BTreeSet<String> {
    let rows = search.read_with(cx, |search, _| search.shown_rows());
    rows.into_iter()
        .filter_map(|row| match row {
            ShownRow::File(rel, _) => Some(rel),
            ShownRow::Line(..) => None,
        })
        .collect()
}

/// The names of the tree's rows.
fn tree(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> BTreeSet<String> {
    let panel = panel(workbench, cx);
    rows(&panel, cx).into_iter().map(|(_, name)| name).collect()
}

/// What quick open lists for the empty query.
fn quick_open_paths(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> BTreeSet<String> {
    cx.simulate_keystrokes("cmd-p");
    cx.run_until_parked();
    let picker = workbench.read_with(cx, |workbench, _| workbench.quick_open().clone());
    let paths = picker.read_with(cx, |picker, _| {
        let matches = picker.delegate.matches();
        matches.iter().map(|found| found.path.clone()).collect()
    });
    cx.simulate_keystrokes("escape");
    paths
}

/// The open tabs' paths, whether each is dirty, and the active tab.
fn tabs(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Vec<(PathBuf, bool)>, Option<usize>) {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let tabs = pane
            .tabs()
            .iter()
            .map(|tab| {
                let model = tab.editor.read(cx).model();
                (model.path().unwrap().to_owned(), model.is_dirty())
            })
            .collect();
        (tabs, pane.active())
    })
}

fn names(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// Presses `keys` without running the executor.
fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.dispatch_keystroke(Keystroke::parse(keys).unwrap(), cx);
    });
}

#[gpui::test]
fn open_folder_re_roots_in_place(cx: &mut TestAppContext) {
    let (a, b) = roots();
    let (workbench, changed, _subscription, cx) = workbench_at(cx, a.path());
    let panel_a = panel(&workbench, cx);
    click_row(&panel_a, "a.rs", cx);
    cx.run_until_parked();
    cx.simulate_input("x");
    let a_rs = a.path().join("a.rs");
    assert_eq!(tabs(&workbench, cx), (vec![(a_rs.clone(), true)], Some(0)));
    assert!(!cx.did_prompt_for_paths());

    cx.simulate_keystrokes("cmd-o");
    let options = answer(Some(b.path()), cx);
    assert_shows(&options, false, true);

    assert_eq!(*changed.borrow(), [b.path().to_owned()], "emitted once");
    assert_eq!(
        tree(&workbench, cx),
        names(&["b.rs", "notes_b.txt", "shared.txt"])
    );
    assert_eq!(
        quick_open_paths(&workbench, cx),
        names(&["b.rs", "notes_b.txt", "shared.txt"]),
        "none of a's files"
    );
    // The tab stays, unsaved.
    assert_eq!(tabs(&workbench, cx), (vec![(a_rs, true)], Some(0)));

    cx.simulate_keystrokes("cmd-shift-f");
    cx.simulate_input("needle");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let search = search(&workbench, cx);
    assert_eq!(
        result_files(&search, cx),
        names(&["notes_b.txt", "shared.txt"])
    );
    let results = search.read_with(cx, |search, _| search.shown_rows());
    assert_eq!(results.len(), 4, "{results:?}");
    assert_eq!(*changed.borrow(), [b.path().to_owned()]);

    // The new tree's watcher batches and clicks reach the workbench.
    let new_rs = b.path().join("new.rs");
    fs::write(&new_rs, "").unwrap();
    let panel_b = panel(&workbench, cx);
    panel_b.update(cx, |panel, cx| {
        panel.apply_fs_changes(&[FsChange::Path(new_rs.clone())], cx)
    });
    cx.run_until_parked();
    let paths = quick_open_paths(&workbench, cx);
    assert!(paths.contains("new.rs"), "{paths:?}");
    cx.simulate_keystrokes("cmd-shift-e");
    click_row(&panel_b, "new.rs", cx);
    cx.run_until_parked();
    let (tabs, active) = tabs(&workbench, cx);
    assert_eq!(tabs[1], (new_rs, false));
    assert_eq!(active, Some(1));
}

#[gpui::test]
fn the_old_root_is_released(cx: &mut TestAppContext) {
    let (a, b) = roots();
    let (workbench, changed, _subscription, cx) = workbench_at(cx, a.path());
    let old_panel = panel(&workbench, cx).downgrade();
    let old_picker = workbench.read_with(cx, |workbench, _| workbench.quick_open().downgrade());
    let search = search(&workbench, cx);
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    let state = |cx: &mut VisualTestContext| {
        dock.read_with(cx, |dock, _| (dock.is_visible(), dock.active()))
    };

    // A search over a, with regex on, still running.
    cx.simulate_keystrokes("cmd-shift-f");
    cx.simulate_input("need.e");
    cx.simulate_keystrokes("alt-cmd-r");
    cx.run_until_parked();
    assert!(search.read_with(cx, |search, cx| search.query(cx).regex));
    assert_eq!(
        result_files(&search, cx),
        names(&["notes_a.txt", "shared.txt"])
    );
    press("enter", cx);
    let cancel = search.read_with(cx, |search, _| search.cancel_flag());
    let status = search.read_with(cx, |search, _| search.status_text());
    assert_eq!(status.as_deref(), Some("Searching…"));
    assert!(result_files(&search, cx).is_empty(), "cleared for the run");
    assert_eq!(state(cx), (true, DockPanel::Search));
    let query = search.read_with(cx, |search, cx| search.query(cx));

    // Re-rooted before it lands.
    let b_root = b.path().to_owned();
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_root(b_root, window, cx)
    });
    assert!(
        cancel.load(Ordering::Relaxed),
        "the run over a is cancelled"
    );
    assert_eq!(search.read_with(cx, |search, _| search.status_text()), None);
    cx.run_until_parked();
    assert!(result_files(&search, cx).is_empty(), "a's results dropped");
    assert_eq!(search.read_with(cx, |search, cx| search.query(cx)), query);
    assert_eq!(state(cx), (true, DockPanel::Search), "the dock as it was");
    assert_eq!(*changed.borrow(), [b.path().to_owned()]);
    assert_eq!(
        search.read_with(cx, |search, _| search.status_text()),
        None,
        "a's run never finishes"
    );

    // The query runs again over b.
    cx.update(|window, cx| {
        let input = search.read(cx).input().focus_handle(cx);
        window.focus(&input, cx);
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        result_files(&search, cx),
        names(&["notes_b.txt", "shared.txt"])
    );

    // a's tree and index are gone.
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    assert!(old_panel.upgrade().is_none(), "the old project panel");
    assert!(old_picker.upgrade().is_none(), "the old quick-open index");

    // With the dock hidden on the outline, and the palette open mid-walk.
    cx.simulate_keystrokes("cmd-shift-o");
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-b");
    let hidden = state(cx);
    let old_picker = workbench.read_with(cx, |workbench, _| workbench.quick_open().downgrade());
    press("cmd-p", cx);
    assert!(workbench.read_with(cx, |workbench, _| workbench.palette_open()));
    press("cmd-o", cx);
    answer(Some(a.path()), cx);
    assert!(!workbench.read_with(cx, |workbench, _| workbench.palette_open()));
    assert_eq!(state(cx), hidden);
    assert_eq!(
        tree(&workbench, cx),
        names(&["a.rs", "notes_a.txt", "shared.txt"])
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    assert!(old_picker.upgrade().is_none(), "the palette's walk dropped");
    assert_eq!(
        *changed.borrow(),
        [b.path().to_owned(), a.path().to_owned()]
    );
}

#[gpui::test]
fn cancel_and_open_file(cx: &mut TestAppContext) {
    let (a, b) = roots();
    let (workbench, changed, _subscription, cx) = workbench_at(cx, a.path());
    let panel_a = panel(&workbench, cx);
    let tree_a = tree(&workbench, cx);

    // Cancelled: nothing changes. Asked again while the panel shows, no
    // second panel.
    cx.simulate_keystrokes("cmd-o");
    cx.simulate_keystrokes("cmd-o");
    let options = answer(None, cx);
    assert_shows(&options, false, true);
    assert!(!cx.did_prompt_for_paths(), "one panel");
    assert_eq!(panel(&workbench, cx), panel_a);
    assert_eq!(tree(&workbench, cx), tree_a);
    assert!(changed.borrow().is_empty());
    assert_eq!(tabs(&workbench, cx), (vec![], None));

    // A file outside the root opens in a new active tab.
    let open_file = || {
        RunCommand(
            Invocation::of("workbench.open_file")
                .unwrap()
                .context(ContextStack::new().with(ContextFrame::new("workbench"))),
        )
    };
    click_row(&panel_a, "a.rs", cx);
    cx.run_until_parked();
    cx.update(|window, cx| window.dispatch_action(Box::new(open_file()), cx));
    let b_rs = b.path().join("b.rs");
    let options = answer(Some(&b_rs), cx);
    assert_shows(&options, true, false);
    let a_rs = a.path().join("a.rs");
    assert_eq!(
        tabs(&workbench, cx),
        (vec![(a_rs.clone(), false), (b_rs.clone(), false)], Some(1))
    );
    assert_eq!(panel(&workbench, cx), panel_a, "the root stays");
    assert_eq!(tree(&workbench, cx), tree_a);
    assert!(changed.borrow().is_empty());
    let text = workbench.read_with(cx, |workbench, cx| {
        let editor = workbench.active_editor(cx).expect("read");
        editor.read(cx).model().text().to_string()
    });
    assert_eq!(text, "fn b() {}\n");

    // A file already open activates its tab.
    cx.update(|window, cx| window.dispatch_action(Box::new(open_file()), cx));
    answer(Some(&a_rs), cx);
    assert_eq!(
        tabs(&workbench, cx),
        (vec![(a_rs, false), (b_rs, false)], Some(0))
    );

    // Cancelled: nothing opens.
    cx.update(|window, cx| window.dispatch_action(Box::new(open_file()), cx));
    answer(None, cx);
    assert_eq!(tabs(&workbench, cx).0.len(), 2);
    assert!(changed.borrow().is_empty());
}

#[test]
fn a_failed_open_panel_says_why() {
    let (message, detail) = crate::workbench::open_panel_failed("Portal request failed");
    assert_eq!(message, "The open panel isn't available");
    assert!(detail.starts_with("Portal request failed"), "{detail}");
    assert_eq!(
        detail.contains("xdg-desktop-portal-gtk"),
        cfg!(target_os = "linux"),
        "{detail}"
    );
}
