//! The Quickfix List panel (`quickfix_panel.rs`): `:copen` and `:lopen`
//! show the window's quickfix list or the active pane's location list in
//! the sidebar, one `path:row:col  text` row per item under the list's
//! title and count, the current item highlighted and scrolled into view
//! as `:cnext` moves it; Enter on a row lands on its item; `:cclose`
//! hides the panel.

use std::path::{Path, PathBuf};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand, set_vim_enabled};
use gpui::{Entity, Focusable as _, SharedString, TestAppContext, VisualTestContext};

use super::QuickfixPanel;
use crate::{
    DockPanel, Launch, StatusItem, StatusSide, Workbench,
    quickfix::{QuickfixItem, QuickfixKind, QuickfixList},
};

/// Each file's text: rows long enough for a column.
const TEXT: &str = "row zero\nrow one\nrow two here\nrow three\n";

/// A window on `root` holding `a.txt`, `b.txt` and `c.txt`, with `a.txt`
/// open and focused, and vim on.
fn window(cx: &mut TestAppContext, root: &Path) -> (Entity<Workbench>, VisualTestContext) {
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(root.join(name), TEXT).unwrap();
    }
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
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

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Runs the typed command `id`.
fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// Types `:{line}` and Enter in the focused editor.
fn ex(line: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(":");
    cx.simulate_input(line);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

fn item(root: &Path, name: &str, row: u32, col: u32, text: &str) -> QuickfixItem {
    QuickfixItem {
        path: root.join(name),
        row,
        col,
        text: text.to_owned(),
        kind: QuickfixKind::Error,
    }
}

/// Three items: `a.txt`'s first row, `b.txt` at row 2 column 4, then
/// `c.txt`'s row 3, a warning.
fn three(root: &Path) -> QuickfixList {
    let mut third = item(root, "c.txt", 3, 1, "third");
    third.kind = QuickfixKind::Warning;
    QuickfixList::new(
        ":grep foo",
        vec![
            item(root, "a.txt", 0, 0, "first"),
            item(root, "b.txt", 2, 4, "second\n   here"),
            third,
        ],
    )
}

fn push(
    workbench: &Entity<Workbench>,
    location: bool,
    list: QuickfixList,
    cx: &mut VisualTestContext,
) {
    workbench.update(cx, |workbench, cx| {
        workbench.push_quickfix_list(location, list, cx)
    });
    draw(cx);
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a tab is shown")
}

/// Focuses the active editor, as a click in it would.
fn focus_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let editor = active(workbench, cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
}

/// The active editor's file and its caret's (row, byte column).
fn landed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (PathBuf, (u32, u32)) {
    let editor = active(workbench, cx);
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let head = model.primary_selection().head();
        let point = model.snapshot().text().offset_to_point(head);
        (
            model.path().expect("a file").to_owned(),
            (point.row, point.column),
        )
    })
}

/// The vim message the status bar draws.
fn message(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<SharedString> {
    draw(cx);
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.read_with(cx, |bar, _| bar.drawn(StatusSide::Left))
        .into_iter()
        .find(|(id, _)| id == StatusItem::VIM_MESSAGE)
        .map(|(_, text)| text)
}

/// Whether the dock shows, the panel it shows and its title row.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (bool, DockPanel, String) {
    workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        let title = dock.state().title(cx).unwrap_or_default().to_string();
        (dock.is_visible(), dock.active(), title)
    })
}

fn panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<QuickfixPanel> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).quickfix().cloned().expect("made")
    })
}

/// The last frame's rows, `(row, "place  text")`.
fn rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(usize, String)> {
    let panel = panel(workbench, cx);
    panel.read_with(cx, |panel, _| {
        (panel.test.rows.iter())
            .map(|(ix, (place, text))| (*ix, format!("{place}  {text}")))
            .collect()
    })
}

/// The last frame's header, and the row it drew as the current item.
fn drawn(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Option<(String, String)>, Option<usize>) {
    let panel = panel(workbench, cx);
    panel.read_with(cx, |panel, _| {
        (panel.test.header.clone(), panel.test.highlighted)
    })
}

fn panel_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let panel = panel(workbench, cx);
    cx.update(|window, cx| panel.focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn copen_shows_the_panel_with_the_list_rows(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (workbench, mut vcx) = window(cx, root);
    let cx = &mut vcx;
    push(&workbench, false, three(root), cx);

    ex("copen", cx);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::Quickfix, "Quickfix List".to_owned())
    );
    assert!(panel_focused(&workbench, cx), "focused, as nvim's window");
    assert_eq!(
        rows(&workbench, cx),
        [
            (0, "a.txt:1:1  first".to_owned()),
            (1, "b.txt:3:5  second here".to_owned()),
            (2, "c.txt:4:2  third".to_owned()),
        ]
    );
    assert_eq!(
        drawn(&workbench, cx),
        (Some((":grep foo".into(), "3 items".into())), Some(0))
    );
    // Nothing landed: the editor still shows a.txt.
    assert_eq!(landed(&workbench, cx), (root.join("a.txt"), (0, 0)));

    // A new list replaces the rows.
    let one = QuickfixList::new(":make", vec![item(root, "c.txt", 1, 0, "only")]);
    push(&workbench, false, one, cx);
    assert_eq!(rows(&workbench, cx), [(0, "c.txt:2:1  only".to_owned())]);
    assert_eq!(
        drawn(&workbench, cx).0,
        Some((":make".into(), "1 item".into()))
    );

    // The typed command shows it too, and with no list, an empty panel.
    focus_editor(&workbench, cx);
    run("dock.toggle", cx);
    assert!(!shown(&workbench, cx).0);
    run("quickfix.show", cx);
    assert_eq!(shown(&workbench, cx).1, DockPanel::Quickfix);
}

#[gpui::test]
fn an_empty_window_shows_no_list(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = window(cx, dir.path());
    let cx = &mut vcx;
    ex("copen", cx);
    assert_eq!(shown(&workbench, cx).1, DockPanel::Quickfix);
    assert_eq!(rows(&workbench, cx), []);
    assert_eq!(
        drawn(&workbench, cx).0,
        Some(("No quickfix list".into(), String::new()))
    );
}

#[gpui::test]
fn cclose_hides_the_panel(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (workbench, mut vcx) = window(cx, root);
    let cx = &mut vcx;
    push(&workbench, false, three(root), cx);
    ex("copen", cx);
    focus_editor(&workbench, cx);

    // `:lclose` is the location list's: the quickfix list stays.
    ex("lclose", cx);
    assert!(shown(&workbench, cx).0);
    ex("cclose", cx);
    assert!(!shown(&workbench, cx).0);
    // Hidden, it stays hidden.
    ex("ccl", cx);
    assert!(!shown(&workbench, cx).0);

    // Closed from the panel, focus goes back to the editor.
    ex("copen", cx);
    assert!(panel_focused(&workbench, cx));
    run("quickfix.toggle", cx);
    assert!(!shown(&workbench, cx).0);
    let editor = active(&workbench, cx);
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    run("quickfix.toggle", cx);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::Quickfix, "Quickfix List".into())
    );

    // Another panel shown: `:cclose` leaves it.
    focus_editor(&workbench, cx);
    run("problems.show", cx);
    focus_editor(&workbench, cx);
    ex("cclose", cx);
    assert_eq!(shown(&workbench, cx).1, DockPanel::Problems);
    assert!(shown(&workbench, cx).0);
}

#[gpui::test]
fn cnext_moves_the_highlight_and_scrolls(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (workbench, mut vcx) = window(cx, root);
    let cx = &mut vcx;
    let items = (0..300)
        .map(|ix| item(root, "a.txt", ix % 4, 0, &format!("item {ix}")))
        .collect();
    push(&workbench, false, QuickfixList::new("many", items), cx);
    ex("copen", cx);
    let built = |cx: &mut VisualTestContext| -> Vec<usize> {
        rows(&workbench, cx).into_iter().map(|(ix, _)| ix).collect()
    };
    // The first row drawn in view: row 0 is drawn to measure the rows
    // wherever the list is scrolled.
    let top = |cx: &mut VisualTestContext| {
        let built = built(cx);
        match built.contains(&1) {
            true => 0,
            false => built.into_iter().find(|&ix| ix > 0).unwrap_or(0),
        }
    };
    assert_eq!(drawn(&workbench, cx).1, Some(0));
    assert!(built(cx).contains(&0));
    assert!(!built(cx).contains(&250), "only the rows in view");
    assert_eq!(top(cx), 0);

    focus_editor(&workbench, cx);
    ex("cnext", cx);
    assert_eq!(drawn(&workbench, cx).1, Some(1));
    // The typed command moves it too.
    run("quickfix.next", cx);
    assert_eq!(drawn(&workbench, cx).1, Some(2));
    // So do nvim's `]q` and `[q`, in the editor in normal mode.
    focus_editor(&workbench, cx);
    cx.simulate_keystrokes("] q");
    draw(cx);
    assert_eq!(drawn(&workbench, cx).1, Some(3));
    focus_editor(&workbench, cx);
    cx.simulate_keystrokes("[ q");
    draw(cx);
    assert_eq!(drawn(&workbench, cx).1, Some(2));

    // Far down the list, the current item is scrolled into view.
    focus_editor(&workbench, cx);
    ex("cc 250", cx);
    draw(cx);
    assert_eq!(drawn(&workbench, cx).1, Some(249));
    let rows_now = built(cx);
    assert!(rows_now.contains(&249), "{rows_now:?}");
    assert!(!rows_now.contains(&100), "{rows_now:?}");
    let scrolled = top(cx);
    assert!((150..=249).contains(&scrolled), "{scrolled}");
    assert!(rows(&workbench, cx).contains(&(249, "a.txt:2:1  item 249".to_owned())));
    // A move within the view doesn't scroll.
    run("quickfix.previous", cx);
    assert_eq!(drawn(&workbench, cx).1, Some(248));
    assert_eq!(top(cx), scrolled);

    // And back up.
    run("quickfix.first", cx);
    draw(cx);
    assert_eq!(drawn(&workbench, cx).1, Some(0));
    assert_eq!(top(cx), 0);
}

#[gpui::test]
fn enter_on_a_row_opens_the_item_at_its_position(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (workbench, mut vcx) = window(cx, root);
    let cx = &mut vcx;
    push(&workbench, false, three(root), cx);
    ex("copen", cx);
    assert!(panel_focused(&workbench, cx));

    // Down to the second row, then Enter: `:cc 2`.
    cx.simulate_keystrokes("down");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(landed(&workbench, cx), (root.join("b.txt"), (2, 4)));
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("(2 of 3) error: second here")
    );
    assert_eq!(drawn(&workbench, cx).1, Some(1), "now the current item");
    let editor = active(&workbench, cx);
    assert!(
        cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)),
        "the editor landed in has focus, as nvim's previous window"
    );
    assert!(shown(&workbench, cx).0, "the panel stays");

    // With vim on, `j` and `o` do what Down and Enter do; `gg` goes to the
    // first row.
    run("quickfix.show", cx);
    cx.simulate_keystrokes("j");
    draw(cx);
    cx.simulate_keystrokes("o");
    draw(cx);
    assert_eq!(landed(&workbench, cx), (root.join("c.txt"), (3, 1)));
    run("quickfix.show", cx);
    cx.simulate_keystrokes("g g enter");
    draw(cx);
    assert_eq!(landed(&workbench, cx), (root.join("a.txt"), (0, 0)));
    assert_eq!(drawn(&workbench, cx).1, Some(0));
}

#[gpui::test]
fn lopen_shows_the_panes_location_list(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (workbench, mut vcx) = window(cx, root);
    let cx = &mut vcx;
    // No location list yet: nvim's E776, and nothing shown.
    ex("lopen", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E776: No location list")
    );
    assert!(!shown(&workbench, cx).0 || shown(&workbench, cx).1 != DockPanel::Quickfix);

    push(&workbench, false, three(root), cx);
    let location = QuickfixList::new(
        ":lgrep bar",
        vec![
            item(root, "c.txt", 0, 2, "here"),
            item(root, "b.txt", 1, 0, "there"),
        ],
    );
    push(&workbench, true, location, cx);
    ex("lopen", cx);
    assert_eq!(
        shown(&workbench, cx),
        (true, DockPanel::Quickfix, "Location List".to_owned())
    );
    assert_eq!(
        rows(&workbench, cx),
        [
            (0, "c.txt:1:3  here".to_owned()),
            (1, "b.txt:2:1  there".to_owned()),
        ]
    );
    assert_eq!(
        drawn(&workbench, cx).0,
        Some((":lgrep bar".into(), "2 items".into()))
    );

    // Enter lands on the location list's item, moving it, not the
    // quickfix list's.
    cx.simulate_keystrokes("down enter");
    draw(cx);
    assert_eq!(landed(&workbench, cx), (root.join("b.txt"), (1, 0)));
    let current = |location, cx: &mut VisualTestContext| {
        workbench.update(cx, |workbench, cx| {
            workbench
                .quickfix_list(location, cx)
                .map(|list| list.current)
        })
    };
    assert_eq!((current(true, cx), current(false, cx)), (Some(1), Some(0)));
    // `:lnext` past the end says so; `:lprevious` moves the highlight.
    ex("lprevious", cx);
    assert_eq!(drawn(&workbench, cx).1, Some(0));

    // `:cclose` leaves the location list; `:lclose` hides it.
    ex("cclose", cx);
    assert!(shown(&workbench, cx).0);
    ex("lclose", cx);
    assert!(!shown(&workbench, cx).0);

    // `:copen` switches the panel to the quickfix list.
    run("location.toggle", cx);
    assert_eq!(shown(&workbench, cx).2, "Location List");
    focus_editor(&workbench, cx);
    ex("copen", cx);
    assert_eq!(shown(&workbench, cx).2, "Quickfix List");
    assert_eq!(rows(&workbench, cx).len(), 3);
}
