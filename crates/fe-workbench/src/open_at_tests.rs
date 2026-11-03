//! [`Workbench::open_document_at`], in a headless window short enough for a
//! file to scroll.

use std::{
    cell::Cell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Pane, Workbench, loader, tests::open};

/// 200 rows, "line 1" and so on, but for row 150's two-byte chars, ahead of
/// its column 3.
fn text() -> String {
    let rows = (1..=200).map(|n| match n {
        150 => "éé line 150".to_owned(),
        n => format!("line {n}"),
    });
    rows.collect::<Vec<_>>().join("\n")
}

fn window(launch: Launch, cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let (workbench, cx, _) = open(cx, launch);
    cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(300.)));
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    (workbench, cx)
}

/// A window on `dir`, drawn, with `a.txt` of [`text`] on disk.
fn project<'a>(
    dir: &Path,
    cx: &'a mut TestAppContext,
) -> (Entity<Workbench>, &'a mut VisualTestContext, PathBuf) {
    let path = dir.join("a.txt");
    fs::write(&path, text()).unwrap();
    let launch = Launch {
        root: Some(dir.to_owned()),
        file: None,
    };
    let (workbench, cx) = window(launch, cx);
    draw(cx);
    (workbench, cx, path)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Opens `path` at `line` and `col`, and waits for it, drawing.
fn open_at(
    workbench: &Entity<Workbench>,
    path: &Path,
    line: Option<u32>,
    col: Option<u32>,
    cx: &mut VisualTestContext,
) {
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.open_document_at(path.to_owned(), line, col, window, cx)
        })
    })
    .detach();
    draw(cx);
}

fn pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

fn editor_of(pane: &Entity<Pane>, path: &Path, cx: &mut VisualTestContext) -> Entity<EditorView> {
    pane.read_with(cx, |pane, _| {
        let tab = pane.position(path).expect("a tab of the file");
        pane.tabs()[tab].editor().expect("an editor").clone()
    })
}

/// The caret's (row, byte column), and whether it's the only selection.
fn caret(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> ((u32, u32), bool) {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let selection = model.primary_selection();
        let point = model.snapshot().text().offset_to_point(selection.head());
        (
            (point.row, point.column),
            model.selections().len() == 1 && selection.range.is_empty(),
        )
    })
}

/// The tabs' paths, and which is active.
fn tabs(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> (Vec<PathBuf>, Option<usize>) {
    pane.read_with(cx, |pane, _| {
        let paths = pane.tabs().iter().map(|tab| tab.path.clone().unwrap());
        (paths.collect(), pane.active())
    })
}

#[gpui::test]
fn open_document_at_puts_the_caret_at_line_and_column_clamped_and_scrolled(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx, path) = project(dir.path(), cx);
    open_at(&workbench, &path, Some(150), Some(3), cx);

    let pane = pane(&workbench, cx);
    let editor = editor_of(&pane, &path, cx);
    // Column 3 is past two chars of two bytes each.
    assert_eq!(caret(&editor, cx), ((149, 4), true));
    let shown = |row, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| editor.shows_buffer_row(row).expect("drawn"))
    };
    assert!(shown(149, cx), "scrolled to the caret");
    assert!(!shown(0, cx), "the file's start is scrolled away");
    assert!(
        cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)),
        "focused, as an open is"
    );

    // Open already: the same tab moves, whatever it selected.
    editor.update(cx, |editor, _| editor.model_mut().select_range(3..9, false));
    open_at(&workbench, &path, Some(10), None, cx);
    assert_eq!(caret(&editor, cx), ((9, 0), true));
    assert!(shown(9, cx));

    // No line leaves the caret be, and only one tab of the file.
    open_at(&workbench, &path, None, None, cx);
    assert_eq!(caret(&editor, cx), ((9, 0), true));
    assert_eq!(tabs(&pane, cx).0, std::slice::from_ref(&path));

    // Past the row's end, past the last row, and before the first.
    open_at(&workbench, &path, Some(10), Some(99), cx);
    assert_eq!(caret(&editor, cx), ((9, 7), true));
    open_at(&workbench, &path, Some(9999), Some(2), cx);
    assert_eq!(caret(&editor, cx), ((199, 1), true));
    assert!(shown(199, cx));
    open_at(&workbench, &path, Some(u32::MAX), Some(u32::MAX), cx);
    assert_eq!(caret(&editor, cx), ((199, 8), true));
    open_at(&workbench, &path, Some(0), Some(0), cx);
    assert_eq!(caret(&editor, cx), ((0, 0), true));
    assert!(shown(0, cx));
}

/// As `main.rs` opens the command line's files: each once the last is in.
#[gpui::test]
fn files_opened_in_turn_keep_their_order_each_at_its_place(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx, a) = project(dir.path(), cx);
    let (b, new, folder) = (
        dir.path().join("b.txt"),
        dir.path().join("new.txt"),
        dir.path().join("folder"),
    );
    fs::write(&b, "one\ntwo\nthree\n").unwrap();
    fs::create_dir(&folder).unwrap();
    let handle = cx.update(|window, _| window.window_handle());
    let handle = handle.downcast::<Workbench>().unwrap();
    let files = [
        (a.clone(), Some(20)),
        // Fails to open, and the rest open all the same.
        (folder, Some(1)),
        // A new file has a caret too, at its start.
        (new.clone(), Some(5)),
        (b.clone(), Some(2)),
    ];
    cx.spawn(async move |mut cx| {
        for (path, line) in files {
            let landed = handle.update(&mut cx, |workbench, window, cx| {
                workbench.open_document_at(path, line, None, window, cx)
            });
            landed.unwrap().await;
        }
    })
    .detach();
    draw(cx);

    let pane = pane(&workbench, cx);
    let (paths, active) = tabs(&pane, cx);
    assert_eq!(paths, [a.clone(), new.clone(), b.clone()]);
    assert_eq!(active, Some(2), "the last is active");
    assert_eq!(caret(&editor_of(&pane, &a, cx), cx).0, (19, 0));
    assert_eq!(caret(&editor_of(&pane, &new, cx), cx).0, (0, 0));
    assert_eq!(caret(&editor_of(&pane, &b, cx), cx).0, (1, 0));
}

/// A launch whose file the host is still reading.
fn launching<'a>(
    dir: &Path,
    cx: &'a mut TestAppContext,
) -> (Entity<Workbench>, &'a mut VisualTestContext, PathBuf) {
    let path = dir.join("a.txt");
    fs::write(&path, text()).unwrap();
    let launch = Launch {
        root: Some(dir.to_owned()),
        file: Some((path.clone(), None)),
    };
    let (workbench, cx) = window(launch, cx);
    (workbench, cx, path)
}

/// Hands the host's read of `path` to the workbench.
fn host_read(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let read = loader::open(path.to_owned());
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.file_read(path.to_owned(), read, window, cx)
        })
    });
    draw(cx);
}

#[gpui::test]
fn launch_file_still_being_read_is_not_read_again(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx, path) = launching(dir.path(), cx);
    open_at(&workbench, &path, Some(150), Some(3), cx);
    let pane = pane(&workbench, cx);
    pane.read_with(cx, |pane, _| {
        assert_eq!(pane.reading().collect::<Vec<_>>(), [path.as_path()]);
        assert!(pane.tabs().is_empty(), "the host has it");
    });

    host_read(&workbench, &path, cx);
    let editor = editor_of(&pane, &path, cx);
    assert_eq!(caret(&editor, cx), ((149, 4), true));
    assert!(editor.read_with(cx, |editor, _| editor.shows_buffer_row(149).expect("drawn")));
}

#[gpui::test]
fn launch_file_landing_behind_another_gets_its_caret_and_stays_behind(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx, path) = launching(dir.path(), cx);
    let other = dir.path().join("b.txt");
    fs::write(&other, "other\n").unwrap();
    open_at(&workbench, &path, Some(20), None, cx);
    let pane = pane(&workbench, cx);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(other.clone(), window, cx)));
    draw(cx);

    host_read(&workbench, &path, cx);
    assert_eq!(tabs(&pane, cx), (vec![other, path.clone()], Some(0)));
    assert_eq!(caret(&editor_of(&pane, &path, cx), cx).0, (19, 0));
}

#[gpui::test]
fn launch_file_failing_to_read_ends_the_wait(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx, path) = launching(dir.path(), cx);
    let landed = cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.open_document_at(path.clone(), Some(2), None, window, cx)
        })
    });
    let read = Err(std::io::Error::other("no"));
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.file_read(path.clone(), read, window, cx)
        })
    });
    let over = Rc::new(Cell::new(false));
    cx.spawn({
        let over = over.clone();
        async move |_| {
            landed.await;
            over.set(true);
        }
    })
    .detach();
    draw(cx);
    assert!(over.get(), "the next file needn't wait for ever");
}

#[gpui::test]
fn a_place_in_a_previewed_tab_shows_the_text(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.md");
    fs::write(&path, "# one\n\ntwo\nthree\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx) = window(launch, cx);
    open_at(&workbench, &path, None, None, cx);
    let pane = pane(&workbench, cx);
    let editor = editor_of(&pane, &path, cx);
    let context: ContextStack = ["workbench", "editor"]
        .into_iter()
        .map(ContextFrame::new)
        .collect();
    let run = RunCommand(
        Invocation::of("markdown.toggle_preview")
            .unwrap()
            .context(context),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let previewed = |cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, _| pane.tabs()[0].preview().is_some())
    };
    assert!(previewed(cx));

    open_at(&workbench, &path, Some(4), Some(2), cx);
    assert!(!previewed(cx));
    assert_eq!(caret(&editor, cx), ((3, 1), true));
}

#[gpui::test]
fn a_viewer_goes_to_the_place_too(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    loader::test_support::lower_threshold(&root, 1024, false);
    let path = root.join("large.txt");
    let rows: String = (0..2000).map(|row| format!("line {row:06}\n")).collect();
    fs::write(&path, rows).unwrap();
    let launch = Launch {
        root: Some(root.clone()),
        file: None,
    };
    let (workbench, cx) = window(launch, cx);
    open_at(&workbench, &path, Some(1500), Some(3), cx);
    let viewer = pane(&workbench, cx).read_with(cx, |pane, _| {
        let tab = pane.position(&path).expect("a tab of the file");
        pane.tabs()[tab].viewer().expect("a viewer").clone()
    });
    // Its index runs on a thread of its own.
    let wanted = 1499 * 12 + 2;
    let deadline = Instant::now() + Duration::from_secs(30);
    while viewer.read_with(cx, |viewer, _| viewer.model().selection()) != (wanted..wanted) {
        assert!(Instant::now() < deadline, "the viewer never got there");
        cx.executor().advance_clock(Duration::from_millis(100));
        draw(cx);
        std::thread::sleep(Duration::from_millis(5));
    }
}
