//! Reopen Closed Editor: closed file editors go on a stack of 20, newest
//! on top, and `workbench.reopen_closed_editor` opens the newest again in
//! its pane, with its selections and scroll.

use std::path::{Path, PathBuf};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::EditorModel;
use fe_gpui::{DiffSide, DiffView, EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use gpui::{AppContext as _, Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use crate::{Launch, Pane, Workbench, closed_editors::CAPACITY};

/// A workbench on a folder of `files`, each `rows` rows long, with no
/// file open.
fn opened<'a>(
    files: &[&str],
    rows: usize,
    cx: &'a mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let text: String = (0..rows).map(|row| format!("row {row}\n")).collect();
    for name in files {
        std::fs::write(dir.path().join(name), &text).unwrap();
    }
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
    (dir, workbench, cx)
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

fn active_pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    workbench.read_with(cx, |workbench, cx| workbench.panes(cx))
}

/// Opens `path` in the active pane, read and drawn.
fn open(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = active_pane(workbench, cx);
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
}

/// Each pane's tabs' file names, and its active tab.
fn tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(Vec<String>, usize)> {
    let panes = panes(workbench, cx);
    cx.update(|_, cx| {
        (panes.iter())
            .map(|pane| {
                let pane = pane.read(cx);
                let names = (pane.tabs().iter())
                    .map(|tab| {
                        let path = tab.path.as_deref();
                        let name = path.and_then(Path::file_name).unwrap_or_default();
                        name.to_string_lossy().into_owned()
                    })
                    .collect();
                (names, pane.active().unwrap_or(usize::MAX))
            })
            .collect()
    })
}

/// The active pane's active editor.
fn editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    let pane = active_pane(workbench, cx);
    pane.read_with(cx, |pane, _| pane.active_editor().cloned())
        .expect("an editor is shown")
}

fn active_path(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    let editor = editor(workbench, cx);
    editor.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned))
}

/// The closed editors' files, oldest first.
fn stack(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<PathBuf> {
    let pane = active_pane(workbench, cx);
    pane.read_with(cx, |pane, _| pane.closed_editors().borrow().paths())
}

fn view(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (Vec<ResolvedSelection>, f32) {
    editor.read_with(cx, |editor, _| {
        (editor.model().selections(), editor.scroll_top())
    })
}

#[gpui::test]
fn close_then_reopen(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(&["a.txt", "b.txt"], 300, cx);
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    open(&workbench, &a, cx);
    // b in a pane split off to the right.
    run("pane.split_right", cx);
    open(&workbench, &b, cx);
    let right = active_pane(&workbench, cx);
    let editor = editor(&workbench, cx);
    editor.update(cx, |editor, cx| {
        editor.model_mut().set_selections([
            ResolvedSelection {
                range: 30..34,
                reversed: true,
            },
            ResolvedSelection {
                range: 900..900,
                reversed: false,
            },
        ]);
        editor.set_scroll_top(40., cx);
    });
    draw(cx);
    let before = view(&editor, cx);
    assert_eq!(before.0.len(), 2);
    assert_eq!(before.1, 40.);

    run("workbench.close_tab", cx);
    assert_eq!(tabs(&workbench, cx)[1], (vec!["a.txt".to_owned()], 0));
    assert_eq!(stack(&workbench, cx), std::slice::from_ref(&b));

    // Reopened from the left pane, it opens in the right one again.
    run("pane.focus_left", cx);
    assert_ne!(active_pane(&workbench, cx), right);
    run("workbench.reopen_closed_editor", cx);
    assert_eq!(active_pane(&workbench, cx), right);
    let names = |names: &[&str]| {
        names
            .iter()
            .map(|name| name.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        tabs(&workbench, cx),
        [(names(&["a.txt"]), 0), (names(&["a.txt", "b.txt"]), 1)]
    );
    let reopened = self::editor(&workbench, cx);
    assert_ne!(reopened, editor);
    assert_eq!(active_path(&workbench, cx), Some(b));
    assert_eq!(view(&reopened, cx), before);
    assert!(stack(&workbench, cx).is_empty());
}

#[gpui::test]
fn stack_order_and_cap(cx: &mut TestAppContext) {
    let names: Vec<String> = (0..=CAPACITY).map(|ix| format!("f{ix:02}.txt")).collect();
    let mut files: Vec<&str> = names.iter().map(String::as_str).collect();
    files.extend(["a.txt", "b.txt", "c.txt"]);
    let (dir, workbench, cx) = opened(&files, 3, cx);
    let path = |name: &str| dir.path().join(name);

    // Opened c, b, a, so closing the active tab closes a, then b, then c.
    for name in ["c.txt", "b.txt", "a.txt"] {
        open(&workbench, &path(name), cx);
    }
    for _ in 0..3 {
        run("workbench.close_tab", cx);
    }
    assert_eq!(
        stack(&workbench, cx),
        [path("a.txt"), path("b.txt"), path("c.txt")]
    );
    for name in ["c.txt", "b.txt", "a.txt"] {
        run("workbench.reopen_closed_editor", cx);
        assert_eq!(active_path(&workbench, cx), Some(path(name)));
    }
    assert!(stack(&workbench, cx).is_empty());

    // A file closed again keeps one entry, on top.
    run("workbench.close_tab", cx);
    run("workbench.close_tab", cx);
    assert_eq!(stack(&workbench, cx), [path("a.txt"), path("b.txt")]);
    open(&workbench, &path("a.txt"), cx);
    run("workbench.close_tab", cx);
    assert_eq!(stack(&workbench, cx), [path("b.txt"), path("a.txt")]);

    // 21 more closes: the 21st drops the oldest, each time.
    for name in &names {
        open(&workbench, &path(name), cx);
        run("workbench.close_tab", cx);
    }
    let kept: Vec<PathBuf> = names[1..].iter().map(|name| path(name)).collect();
    assert_eq!(stack(&workbench, cx), kept);
}

#[gpui::test]
fn already_open_and_missing(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(&["a.txt", "b.txt", "c.txt"], 3, cx);
    let path = |name: &str| dir.path().join(name);

    // Nothing closed: nothing happens.
    open(&workbench, &path("a.txt"), cx);
    run("workbench.reopen_closed_editor", cx);
    assert_eq!(tabs(&workbench, cx), [(vec!["a.txt".to_owned()], 0)]);

    // a, open again, just shows: no second tab, and its entry is gone.
    open(&workbench, &path("b.txt"), cx);
    cx.update(|window, cx| {
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| pane.activate(0, window, cx));
    });
    run("workbench.close_tab", cx);
    assert_eq!(stack(&workbench, cx), [path("a.txt")]);
    open(&workbench, &path("a.txt"), cx);
    open(&workbench, &path("b.txt"), cx);
    run("workbench.reopen_closed_editor", cx);
    let names = vec!["b.txt".to_owned(), "a.txt".to_owned()];
    assert_eq!(tabs(&workbench, cx), [(names, 1)]);
    assert!(stack(&workbench, cx).is_empty());

    // b's and a's files are deleted: they're skipped for c's.
    open(&workbench, &path("c.txt"), cx);
    run("workbench.close_tab", cx);
    run("workbench.close_tab", cx);
    assert_eq!(stack(&workbench, cx), [path("c.txt"), path("a.txt")]);
    run("workbench.close_tab", cx);
    std::fs::remove_file(path("a.txt")).unwrap();
    assert_eq!(
        stack(&workbench, cx),
        [path("c.txt"), path("a.txt"), path("b.txt")]
    );
    std::fs::remove_file(path("b.txt")).unwrap();
    run("workbench.reopen_closed_editor", cx);
    assert_eq!(tabs(&workbench, cx), [(vec!["c.txt".to_owned()], 0)]);
    assert!(stack(&workbench, cx).is_empty());
}

#[gpui::test]
fn every_close_route_pushes(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(&["a.txt", "b.txt", "c.txt", "d.txt"], 3, cx);
    let path = |name: &str| dir.path().join(name);

    // Close Tab.
    open(&workbench, &path("a.txt"), cx);
    run("workbench.close_tab", cx);
    assert_eq!(stack(&workbench, cx), [path("a.txt")]);

    // The tab's ×.
    open(&workbench, &path("b.txt"), cx);
    let close = cx.debug_bounds("tab-close:b.txt").expect("b's × is drawn");
    cx.simulate_click(close.center(), Modifiers::none());
    draw(cx);
    assert!(tabs(&workbench, cx)[0].0.is_empty());
    assert_eq!(stack(&workbench, cx), [path("a.txt"), path("b.txt")]);

    // A preview the next preview replaces isn't closed by the user.
    let pane = active_pane(&workbench, cx);
    for name in ["a.txt", "c.txt"] {
        let path = path(name);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open_preview(path, window, cx)));
        draw(cx);
    }
    assert_eq!(tabs(&workbench, cx), [(vec!["c.txt".to_owned()], 0)]);
    assert_eq!(stack(&workbench, cx), [path("a.txt"), path("b.txt")]);

    // Nor is a tab moved to another pane.
    open(&workbench, &path("a.txt"), cx);
    run("pane.move_tab_right", cx);
    assert_eq!(tabs(&workbench, cx).len(), 2);
    assert_eq!(stack(&workbench, cx), [path("a.txt"), path("b.txt")]);

    // An untitled tab and a diff aren't pushed.
    run("workbench.new_file", cx);
    run("workbench.close_tab", cx);
    let compared = path("c.txt");
    cx.update(|_, cx| {
        // No handle outlives this: Close Pane below must release the pane.
        let pane = workbench.read(cx).pane().clone();
        let side = |title: &str, path: Option<PathBuf>| {
            DiffSide::new(title.to_owned(), EditorModel::new(Buffer::new("x\n"), path))
        };
        let diff = cx.new(|cx| {
            DiffView::new(
                "c.txt",
                side("HEAD", None),
                side("c.txt", Some(compared.clone())),
                cx,
            )
            .with_compared_path(compared)
        });
        pane.update(cx, |pane, cx| pane.add_item(Box::new(diff), cx));
    });
    draw(cx);
    run("workbench.close_tab", cx);
    assert_eq!(stack(&workbench, cx), [path("a.txt"), path("b.txt")]);

    // Close Pane, with its tabs; d then reopens in the pane left.
    open(&workbench, &path("d.txt"), cx);
    run("pane.close", cx);
    assert_eq!(tabs(&workbench, cx), [(vec!["c.txt".to_owned()], 0)]);
    assert_eq!(
        stack(&workbench, cx),
        [path("b.txt"), path("a.txt"), path("d.txt")]
    );
    run("workbench.reopen_closed_editor", cx);
    let names = vec!["c.txt".to_owned(), "d.txt".to_owned()];
    assert_eq!(tabs(&workbench, cx), [(names, 1)]);
}

/// Closing one of two copies of a file, in split panes, pushes it, by
/// each route; the reopen then shows the copy left.
#[gpui::test]
fn a_closed_split_copy_reopens_as_the_tab_left(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(&["a.txt", "c.txt"], 3, cx);
    let (a, c) = (dir.path().join("a.txt"), dir.path().join("c.txt"));
    let names = |names: &[&str]| {
        names
            .iter()
            .map(|name| name.to_string())
            .collect::<Vec<_>>()
    };
    open(&workbench, &a, cx);
    let left = active_pane(&workbench, cx);
    let reopened = |cx: &mut VisualTestContext| {
        run("workbench.reopen_closed_editor", cx);
        assert_eq!(active_pane(&workbench, cx), left);
        assert!(stack(&workbench, cx).is_empty());
    };

    // Close Tab, in the copy split off: its emptied pane closes.
    run("pane.split_right", cx);
    run("workbench.close_tab", cx);
    assert_eq!(tabs(&workbench, cx), [(names(&["a.txt"]), 0)]);
    assert_eq!(stack(&workbench, cx), std::slice::from_ref(&a));
    reopened(cx);
    assert_eq!(tabs(&workbench, cx), [(names(&["a.txt"]), 0)]);

    // The copy's ×, the only one shown with c active on the left.
    run("pane.split_right", cx);
    cx.update(|window, cx| {
        let path = c.clone();
        left.update(cx, |pane, cx| pane.open(path, window, cx));
    });
    draw(cx);
    assert_eq!(
        tabs(&workbench, cx),
        [(names(&["a.txt", "c.txt"]), 1), (names(&["a.txt"]), 0)]
    );
    let close = cx.debug_bounds("tab-close:a.txt").expect("a's × is drawn");
    cx.simulate_click(close.center(), Modifiers::none());
    draw(cx);
    assert_eq!(tabs(&workbench, cx), [(names(&["a.txt", "c.txt"]), 1)]);
    assert_eq!(stack(&workbench, cx), std::slice::from_ref(&a));
    reopened(cx);
    assert_eq!(tabs(&workbench, cx), [(names(&["a.txt", "c.txt"]), 0)]);

    // Close Pane.
    run("pane.split_right", cx);
    run("pane.close", cx);
    assert_eq!(tabs(&workbench, cx).len(), 1);
    assert_eq!(stack(&workbench, cx), std::slice::from_ref(&a));
    reopened(cx);
    assert_eq!(tabs(&workbench, cx), [(names(&["a.txt", "c.txt"]), 0)]);
}

/// The editor of `path` in the active pane, shown or not.
fn editor_of(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = active_pane(workbench, cx);
    pane.read_with(cx, |pane, _| {
        let tab = pane
            .tabs()
            .iter()
            .find(|tab| tab.path.as_deref() == Some(path));
        tab.and_then(|tab| tab.editor()).cloned()
    })
    .expect("an editor of the file")
}

fn activate(workbench: &Entity<Workbench>, ix: usize, cx: &mut VisualTestContext) {
    let pane = active_pane(workbench, cx);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(ix, window, cx)));
    draw(cx);
}

/// Each reopen gets its own file's selections and scroll back, whenever
/// its read lands: two reopens before either lands, a tab shown while one
/// is read, and one that waits behind a Save As of the file.
#[gpui::test]
fn restores_land_with_their_files(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(&["a.txt", "b.txt", "c.txt"], 300, cx);
    let path = |name: &str| dir.path().join(name);
    let mut saved = Vec::new();
    for (name, caret, top) in [("a.txt", 100, 20.), ("b.txt", 700, 60.)] {
        open(&workbench, &path(name), cx);
        let editor = editor(&workbench, cx);
        editor.update(cx, |editor, cx| {
            editor.model_mut().set_selections([ResolvedSelection {
                range: caret..caret + 3,
                reversed: false,
            }]);
            editor.set_scroll_top(top, cx);
        });
        draw(cx);
        saved.push(view(&editor, cx));
    }
    open(&workbench, &path("c.txt"), cx);
    activate(&workbench, 0, cx);
    run("workbench.close_tab", cx);
    run("workbench.close_tab", cx);
    assert_eq!(stack(&workbench, cx), [path("a.txt"), path("b.txt")]);
    let restored = |name: &str, cx: &mut VisualTestContext| {
        let editor = editor_of(&workbench, &path(name), cx);
        view(&editor, cx)
    };

    // b, then a, before either is read.
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.reopen_closed(window, cx);
            workbench.reopen_closed(window, cx);
        })
    });
    draw(cx);
    assert_eq!(restored("b.txt", cx), saved[1]);
    assert_eq!(restored("a.txt", cx), saved[0]);
    assert_eq!(active_path(&workbench, cx), Some(path("a.txt")));

    // c shown while a is read: a lands unshown, restored.
    run("workbench.close_tab", cx);
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.reopen_closed(window, cx));
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| pane.activate(0, window, cx));
    });
    draw(cx);
    assert_eq!(active_path(&workbench, cx), Some(path("c.txt")));
    assert_eq!(restored("a.txt", cx), saved[0]);

    // a waits behind a Save As to its path, then opens restored.
    activate(&workbench, 2, cx);
    run("workbench.close_tab", cx);
    let identity = crate::open::file_identity(&path("a.txt"));
    let saving = crate::open::reserve(Buffer::new("").id(), path("a.txt"), identity);
    run("workbench.reopen_closed_editor", cx);
    let names = vec!["c.txt".to_owned(), "b.txt".to_owned()];
    assert_eq!(tabs(&workbench, cx)[0].0, names, "waiting");
    drop(saving);
    draw(cx);
    assert_eq!(active_path(&workbench, cx), Some(path("a.txt")));
    assert_eq!(restored("a.txt", cx), saved[0]);
}
