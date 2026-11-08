//! Landing on quickfix and location list items (`quickfix_commands.rs`):
//! the typed `quickfix.*` and `location.*` commands and vim's `:cn` and
//! friends open the item's file in the active pane at its row and column
//! and say nvim's `(2 of 3) error: text`.

use std::path::{Path, PathBuf};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand, set_vim_enabled};
use gpui::{Entity, Focusable as _, SharedString, TestAppContext, VisualTestContext};

use crate::{
    Launch, StatusItem, StatusSide, Workbench,
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
/// `c.txt`'s row 3.
fn three(root: &Path) -> QuickfixList {
    QuickfixList::new(
        "three",
        vec![
            item(root, "a.txt", 0, 0, "first"),
            item(root, "b.txt", 2, 4, "second here"),
            item(root, "c.txt", 3, 1, "third"),
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
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a tab is shown")
}

/// The active editor's file and its caret's (row, byte column), with
/// whether it's the only selection and empty.
fn landed(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (PathBuf, (u32, u32), bool) {
    let editor = active(workbench, cx);
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let selection = model.primary_selection();
        let point = model.snapshot().text().offset_to_point(selection.head());
        let single = model.selections().len() == 1 && selection.range.is_empty();
        (
            model.path().expect("a file").to_owned(),
            (point.row, point.column),
            single,
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

fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

/// The list in use's selected item, from 0.
fn selected(
    workbench: &Entity<Workbench>,
    location: bool,
    cx: &mut VisualTestContext,
) -> Option<usize> {
    workbench.update(cx, |workbench, cx| {
        workbench
            .quickfix_list(location, cx)
            .map(|list| list.current)
    })
}

fn tab_count(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).tabs().len())
}

#[gpui::test]
fn cnext_opens_the_item_in_the_active_pane_at_its_row_and_column(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (workbench, mut vcx) = window(cx, root);
    let cx = &mut vcx;
    push(&workbench, false, three(root), cx);

    // The typed command.
    run("quickfix.next", cx);
    assert_eq!(landed(&workbench, cx), (root.join("b.txt"), (2, 4), true));
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("(2 of 3) error: second here")
    );
    assert_eq!(selected(&workbench, false, cx), Some(1));
    assert_eq!(tab_count(&workbench, cx), 2, "opened in the active pane");

    // Back to the first, then the vim line.
    run("quickfix.first", cx);
    assert_eq!(landed(&workbench, cx), (root.join("a.txt"), (0, 0), true));
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("(1 of 3) error: first")
    );
    ex("cnext", cx);
    assert_eq!(landed(&workbench, cx), (root.join("b.txt"), (2, 4), true));
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("(2 of 3) error: second here")
    );
    // The file was open already: its tab is reused.
    assert_eq!(tab_count(&workbench, cx), 2);

    run("quickfix.last", cx);
    assert_eq!(landed(&workbench, cx), (root.join("c.txt"), (3, 1), true));
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("(3 of 3) error: third")
    );
    // Past the end: E553, and nothing moves.
    ex("cnext", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E553: No more items")
    );
    assert_eq!(landed(&workbench, cx).0, root.join("c.txt"));
    run("quickfix.previous", cx);
    assert_eq!(landed(&workbench, cx), (root.join("b.txt"), (2, 4), true));
    ex("cc 9", cx);
    assert_eq!(landed(&workbench, cx).0, root.join("c.txt"));
    assert_eq!(selected(&workbench, false, cx), Some(2));
}

#[gpui::test]
fn lists_answer_in_the_status_line(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (workbench, mut vcx) = window(cx, root);
    let cx = &mut vcx;

    // No list yet.
    ex("cnext", cx);
    assert_eq!(message(&workbench, cx).as_deref(), Some("E42: No Errors"));
    ex("colder", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E380: At bottom of quickfix stack")
    );
    ex("lnext", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E776: No location list")
    );
    // Nothing to fill a list with keeps the stack as it is.
    ex("grep foo", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E480: No match: foo")
    );

    push(
        &workbench,
        false,
        QuickfixList::new("older", vec![item(root, "c.txt", 0, 0, "old")]),
        cx,
    );
    push(&workbench, false, three(root), cx);
    ex("clist", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some(
            "1 a.txt:1 col 1 error: first | 2 b.txt:3 col 5 error: second here \
             | 3 c.txt:4 col 2 error: third"
        )
    );
    ex("colder", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("error list 1 of 2; 1 errors       older")
    );
    ex("cc", cx);
    assert_eq!(landed(&workbench, cx).0, root.join("c.txt"));
    ex("cnewer", cx);
    ex("cnf", cx);
    assert_eq!(landed(&workbench, cx), (root.join("b.txt"), (2, 4), true));

    // With vim off, a typed command answers in a toast.
    cx.update(|_, cx| set_vim_enabled(cx, false));
    draw(cx);
    run("quickfix.next", cx);
    assert_eq!(landed(&workbench, cx).0, root.join("c.txt"));
    run("quickfix.next", cx);
    let toasts = toasts(&workbench, cx);
    assert!(
        toasts.iter().any(|toast| toast == "(3 of 3) error: third"),
        "{toasts:?}"
    );
    assert!(
        toasts.iter().any(|toast| toast == "E553: No more items"),
        "{toasts:?}"
    );
}

#[gpui::test]
fn location_lists_follow_the_active_pane_and_close_with_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (workbench, mut vcx) = window(cx, root);
    let cx = &mut vcx;
    let first = workbench.read_with(cx, |workbench, _| workbench.pane().entity_id());
    push(&workbench, true, three(root), cx);
    run("location.next", cx);
    assert_eq!(landed(&workbench, cx), (root.join("b.txt"), (2, 4), true));
    assert_eq!(selected(&workbench, true, cx), Some(1));
    // The quickfix list is apart.
    ex("cnext", cx);
    assert_eq!(message(&workbench, cx).as_deref(), Some("E42: No Errors"));

    run("pane.split_right", cx);
    let second = workbench.read_with(cx, |workbench, _| workbench.pane().entity_id());
    assert_ne!(first, second);
    ex("lnext", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E776: No location list")
    );
    push(
        &workbench,
        true,
        QuickfixList::new("second", vec![item(root, "c.txt", 1, 0, "c")]),
        cx,
    );
    ex("ll", cx);
    assert_eq!(landed(&workbench, cx), (root.join("c.txt"), (1, 0), true));
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("(1 of 1) error: c")
    );

    let has = |pane, cx: &mut VisualTestContext| {
        workbench.update(cx, |workbench, cx| workbench.has_location_list(pane, cx))
    };
    assert!(has(first, cx) && has(second, cx));
    run("pane.close", cx);
    assert!(!has(second, cx), "dropped with its pane");
    assert!(has(first, cx));
    assert_eq!(selected(&workbench, true, cx), Some(1));
    run("location.previous", cx);
    assert_eq!(landed(&workbench, cx), (root.join("a.txt"), (0, 0), true));
}

#[gpui::test]
fn a_superseded_landing_says_nothing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (workbench, mut vcx) = window(cx, root);
    let cx = &mut vcx;
    // c.txt is open already; b.txt is not, so landing on it loads.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let c = root.join("c.txt");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(c, window, cx)));
    draw(cx);
    push(&workbench, false, three(root), cx);
    cx.update(|_, cx| set_vim_enabled(cx, false));
    draw(cx);

    // Two landings with no settling between.
    for _ in 0..2 {
        let run = RunCommand(
            Invocation::of("quickfix.next")
                .unwrap()
                .context(ContextStack::new().with(ContextFrame::new("workbench"))),
        );
        cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    }
    draw(cx);
    let toasts = toasts(&workbench, cx);
    assert!(
        toasts.iter().any(|toast| toast == "(3 of 3) error: third"),
        "{toasts:?}"
    );
    assert!(
        !toasts.iter().any(|toast| toast.starts_with("(2 of 3)")),
        "{toasts:?}"
    );
}

#[gpui::test]
fn a_failed_open_lands_no_message(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (workbench, mut vcx) = window(cx, root);
    let cx = &mut vcx;
    std::fs::create_dir(root.join("sub")).unwrap();
    push(
        &workbench,
        false,
        QuickfixList::new(
            "dir",
            vec![
                item(root, "a.txt", 0, 0, "first"),
                item(root, "sub", 0, 0, "a directory"),
            ],
        ),
        cx,
    );
    cx.update(|_, cx| set_vim_enabled(cx, false));
    draw(cx);
    run("quickfix.next", cx);
    let toasts = toasts(&workbench, cx);
    assert!(
        !toasts.iter().any(|toast| toast.contains("a directory")),
        "{toasts:?}"
    );
}
