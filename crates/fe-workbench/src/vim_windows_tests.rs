//! Vim's window commands on the status bar's line: `:sp`, `:vs`, `:new`,
//! `:vne`, `:on` and `:clo` over the window's panes, as the pane commands
//! run.

use fe_gpui::EditorView;
use gpui::{AnyWindowHandle, Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::super::super::vim_host_tests::{active, dirty, draw, ex, message, window, write};
use crate::{Direction, Pane, Workbench};

/// Each pane's tabs, by file name.
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

fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    workbench.read_with(cx, |workbench, cx| workbench.panes(cx))
}

fn pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

/// The pane `direction` of `from`.
fn neighbour(
    workbench: &Entity<Workbench>,
    from: &Entity<Pane>,
    direction: Direction,
    cx: &mut VisualTestContext,
) -> Option<Entity<Pane>> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.pane_group(cx).neighbour(from, direction, cx)
    })
}

/// The file name the active editor shows, and its text.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (String, String) {
    let editor = active(workbench, cx);
    editor.read_with(cx, |editor, _| {
        let name = fe_gpui::pane_item::file_name(editor.model().path());
        (name, editor.model().text().to_string())
    })
}

fn buffer(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> fe_buffer::BufferId {
    editor.read_with(cx, |editor, _| editor.model().buffer().id())
}

fn focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

fn window_open(window: AnyWindowHandle, cx: &mut VisualTestContext) -> bool {
    cx.cx.update(|cx| cx.windows().contains(&window))
}

/// Types `:{line}` and has it run, its message, if any, read.
fn line(line: &str, workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    ex(line, cx);
    draw(cx);
    message(workbench, cx).map(|message| message.to_string())
}

/// Makes `pane` the active pane, focused.
fn activate(workbench: &Entity<Workbench>, pane: &Entity<Pane>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.activate(pane.clone(), window, cx)
        })
    });
    draw(cx);
}

#[gpui::test]
fn split(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt", "other.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    let first = pane(&workbench, cx);
    let editor = active(&workbench, cx);
    // An unsaved edit: the split shows it, as it shows the same buffer.
    assert_eq!(line("s/one/two/", &workbench, cx), None);
    assert!(dirty(&editor, cx));

    // `:sp` splits downward, as `pane.split_down` does.
    assert_eq!(line("sp", &workbench, cx), None);
    let below = pane(&workbench, cx);
    assert_ne!(below, first);
    assert_eq!(panes(&workbench, cx).len(), 2);
    assert_eq!(
        neighbour(&workbench, &first, Direction::Down, cx),
        Some(below.clone())
    );
    let split = active(&workbench, cx);
    assert_ne!(split, editor, "a second view, active");
    assert_eq!(buffer(&split, cx), buffer(&editor, cx), "the same buffer");
    assert_eq!(shown(&workbench, cx), ("a.txt".into(), "two\n".into()));
    assert!(focused(&split, cx));
    assert_eq!(tabs(&workbench, cx), [["a.txt"], ["a.txt"]]);

    // `:vsp` splits to the right, as `pane.split_right` does.
    assert_eq!(line("vsp", &workbench, cx), None);
    let right = pane(&workbench, cx);
    assert_eq!(
        neighbour(&workbench, &below, Direction::Right, cx),
        Some(right.clone())
    );
    let vsplit = active(&workbench, cx);
    assert_eq!(buffer(&vsplit, cx), buffer(&editor, cx));
    assert!(focused(&vsplit, cx));

    // `:sp {file}` shows the file in the new pane, alone, focused.
    assert_eq!(line("sp other.txt", &workbench, cx), None);
    let other = pane(&workbench, cx);
    assert_eq!(
        neighbour(&workbench, &right, Direction::Down, cx),
        Some(other.clone())
    );
    assert_eq!(shown(&workbench, cx), ("other.txt".into(), "one\n".into()));
    assert!(focused(&active(&workbench, cx), cx));
    assert_eq!(
        tabs(&workbench, cx),
        [
            vec!["a.txt"],
            vec!["a.txt"],
            vec!["a.txt"],
            vec!["other.txt"]
        ]
    );

    // One open in another pane shows its text there too.
    activate(&workbench, &first, cx);
    assert_eq!(line("vs other.txt", &workbench, cx), None);
    let shared = active(&workbench, cx);
    assert_eq!(shown(&workbench, cx).0, "other.txt");
    let opened = other.read_with(cx, |pane, _| pane.active_editor().cloned().unwrap());
    assert_eq!(buffer(&shared, cx), buffer(&opened, cx));
    assert_eq!(tabs(&workbench, cx)[1], ["other.txt"]);

    // `:sp` of the file it shows is `:sp`.
    assert_eq!(line("sp other.txt", &workbench, cx), None);
    assert_eq!(tabs(&workbench, cx).len(), 6);
    assert_eq!(tabs(&workbench, cx)[2], ["other.txt"]);
    assert_eq!(shown(&workbench, cx).0, "other.txt");

    // A file that isn't there: E484, and nothing splits.
    let before = tabs(&workbench, cx);
    let active_pane = pane(&workbench, cx);
    assert_eq!(
        line("sp missing.txt", &workbench, cx).as_deref(),
        Some("E484: Can't open file missing.txt")
    );
    assert_eq!(tabs(&workbench, cx), before);
    assert_eq!(pane(&workbench, cx), active_pane);
    assert_eq!(
        line("vs nowhere/x.txt", &workbench, cx).as_deref(),
        Some("E484: Can't open file nowhere/x.txt")
    );
    assert_eq!(tabs(&workbench, cx), before);
}

#[gpui::test]
fn new(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    let first = pane(&workbench, cx);

    // `:new` splits down with an untitled empty tab, focused.
    assert_eq!(line("new", &workbench, cx), None);
    let below = pane(&workbench, cx);
    assert_eq!(
        neighbour(&workbench, &first, Direction::Down, cx),
        Some(below.clone())
    );
    assert_eq!(shown(&workbench, cx), ("untitled".into(), String::new()));
    assert!(focused(&active(&workbench, cx), cx));
    assert_eq!(tabs(&workbench, cx), [["a.txt"], ["untitled"]]);

    // `:vnew` splits right, from the untitled one.
    assert_eq!(line("vne", &workbench, cx), None);
    let right = pane(&workbench, cx);
    assert_eq!(
        neighbour(&workbench, &below, Direction::Right, cx),
        Some(right)
    );
    let untitled = active(&workbench, cx);
    assert_eq!(shown(&workbench, cx), ("untitled".into(), String::new()));
    assert!(focused(&untitled, cx));
    assert_eq!(
        tabs(&workbench, cx),
        [["a.txt"], ["untitled"], ["untitled"]]
    );
    let left = below.read_with(cx, |pane, _| pane.active_editor().cloned().unwrap());
    assert_ne!(
        buffer(&untitled, cx),
        buffer(&left, cx),
        "a new buffer each"
    );
}

#[gpui::test]
fn only_close(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt", "b.txt", "c.txt", "d.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;
    let handle = cx.update(|window, _| window.window_handle());
    let first = pane(&workbench, cx);

    // Three panes: a; a, b changed; b, c changed.
    assert_eq!(line("vsp", &workbench, cx), None);
    assert_eq!(line("e b.txt", &workbench, cx), None);
    assert_eq!(line("s/one/bee/", &workbench, cx), None);
    assert_eq!(line("sp", &workbench, cx), None);
    assert_eq!(line("e c.txt", &workbench, cx), None);
    assert_eq!(line("s/one/sea/", &workbench, cx), None);
    assert_eq!(
        tabs(&workbench, cx),
        [
            vec!["a.txt"],
            vec!["a.txt", "b.txt"],
            vec!["b.txt", "c.txt"]
        ]
    );

    // `:only` closes the others, the tabs with changes moving here, each
    // once; the clean second view of a closes. This pane shows a still.
    activate(&workbench, &first, cx);
    assert_eq!(line("only", &workbench, cx), None);
    assert_eq!(panes(&workbench, cx), std::slice::from_ref(&first));
    assert_eq!(tabs(&workbench, cx), [["a.txt", "b.txt", "c.txt"]]);
    assert_eq!(shown(&workbench, cx), ("a.txt".into(), "one\n".into()));
    assert!(focused(&active(&workbench, cx), cx));
    let texts: Vec<(String, bool)> = first.read_with(cx, |pane, cx| {
        (pane.tabs().iter())
            .map(|tab| {
                let model = tab.editor().unwrap().read(cx).model();
                (model.text().to_string(), model.is_dirty())
            })
            .collect()
    });
    assert_eq!(
        texts,
        [
            ("one\n".into(), false),
            ("bee\n".into(), true),
            ("sea\n".into(), true)
        ]
    );
    assert!(window_open(handle, cx));

    // Alone, it says so.
    assert_eq!(
        line("on!", &workbench, cx).as_deref(),
        Some("Already only one window")
    );

    // `:close` of the last pane is E444, with or without `!`.
    for close in ["clo", "close!"] {
        assert_eq!(
            line(close, &workbench, cx).as_deref(),
            Some("E444: Cannot close last window")
        );
        assert_eq!(panes(&workbench, cx), std::slice::from_ref(&first));
        assert!(window_open(handle, cx));
    }

    // Of another, it closes it, its tabs shown here staying.
    assert_eq!(line("sp", &workbench, cx), None);
    assert_eq!(panes(&workbench, cx).len(), 2);
    assert_eq!(line("clo", &workbench, cx), None);
    assert_eq!(panes(&workbench, cx), std::slice::from_ref(&first));
    assert_eq!(tabs(&workbench, cx), [["a.txt", "b.txt", "c.txt"]]);
    assert!(window_open(handle, cx));

    // A tab with changes no other pane shows asks, as `pane.close` does.
    assert_eq!(line("vs d.txt", &workbench, cx), None);
    assert_eq!(line("s/one/dee/", &workbench, cx), None);
    assert_eq!(line("clo", &workbench, cx), None);
    assert_eq!(
        cx.pending_prompt().map(|(question, _)| question),
        Some("Discard unsaved changes to d.txt?".into())
    );
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert_eq!(panes(&workbench, cx).len(), 2);
    assert_eq!(line("clo", &workbench, cx), None);
    cx.simulate_prompt_answer("Discard");
    draw(cx);
    assert_eq!(panes(&workbench, cx), std::slice::from_ref(&first));
    assert!(window_open(handle, cx));

    // `:only` from the other pane keeps that one.
    assert_eq!(line("vs d.txt", &workbench, cx), None);
    let second = pane(&workbench, cx);
    assert_eq!(line("on", &workbench, cx), None);
    assert_eq!(panes(&workbench, cx), [second]);
    assert_eq!(tabs(&workbench, cx), [["d.txt", "b.txt", "c.txt"]]);
    assert_eq!(shown(&workbench, cx).0, "d.txt");
    assert!(window_open(handle, cx));
}
