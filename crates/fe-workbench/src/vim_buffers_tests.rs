//! Vim's buffer list on the status bar's line: `:b`, `:bn`, `:bp`, `:bd`,
//! `:bf` and `:bl` over the window's open files, numbered as nvim numbers
//! buffers.

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::EditorModel;
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::super::super::vim_host_tests::{active, dirty, draw, ex, message, window, write};
use crate::{Pane, Workbench};

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

/// The active pane's active tab's file name.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    let editor = active(workbench, cx);
    editor.read_with(cx, |editor, _| {
        fe_gpui::pane_item::file_name(editor.model().path())
    })
}

fn pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
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

/// Types `:{line}` and has it run, its message, if any, read.
fn line(line: &str, workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    ex(line, cx);
    draw(cx);
    message(workbench, cx).map(|message| message.to_string())
}

#[gpui::test]
fn numbering(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt", "b.txt", "c.txt", "d.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt"]);
    let cx = &mut vcx;

    // Nothing shown before: no alternate. `:b` alone changes nothing.
    assert_eq!(
        line("b#", &workbench, cx).as_deref(),
        Some("E23: No alternate file")
    );
    assert_eq!(line("b", &workbench, cx), None);
    assert_eq!(tabs(&workbench, cx), [["a.txt"]]);
    assert_eq!(shown(&workbench, cx), "a.txt");

    // Each file opened gets the next number.
    for file in ["b.txt", "c.txt"] {
        assert_eq!(line(&format!("e {file}"), &workbench, cx), None);
    }
    assert_eq!(tabs(&workbench, cx), [["a.txt", "b.txt", "c.txt"]]);
    for (line_, file) in [
        ("b 1", "a.txt"),
        ("b3", "c.txt"),
        ("2b", "b.txt"),
        ("b 1", "a.txt"),
    ] {
        assert_eq!(line(line_, &workbench, cx), None, ":{line_}");
        assert_eq!(shown(&workbench, cx), file, ":{line_}");
    }
    assert_eq!(tabs(&workbench, cx), [["a.txt", "b.txt", "c.txt"]]);
    assert_eq!(
        line("b 9", &workbench, cx).as_deref(),
        Some("E86: Buffer 9 does not exist")
    );
    assert_eq!(
        line("9b", &workbench, cx).as_deref(),
        Some("E16: Invalid range")
    );
    assert_eq!(shown(&workbench, cx), "a.txt");

    // `#` is the buffer shown before, back and forth.
    assert_eq!(line("b#", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "b.txt");
    assert_eq!(line("b #", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "a.txt");

    // A number isn't reused after :bd: d.txt gets 4, and :b 2 opens b.txt
    // again, as nvim lists its buffer again.
    assert_eq!(line("bd 2", &workbench, cx), None);
    assert_eq!(tabs(&workbench, cx), [["a.txt", "c.txt"]]);
    assert_eq!(line("e d.txt", &workbench, cx), None);
    assert_eq!(line("b 1", &workbench, cx), None);
    assert_eq!(line("b 4", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "d.txt");
    assert_eq!(line("b 2", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "b.txt");
    assert_eq!(tabs(&workbench, cx), [["a.txt", "c.txt", "d.txt", "b.txt"]]);

    // In a split, :b N opens the file in this pane when it has no tab of
    // it, sharing its text, and activates its tab when it has one; the
    // other pane is left as it was.
    run("pane.split_right", cx);
    let first = workbench.read_with(cx, |workbench, cx| workbench.panes(cx)[0].clone());
    let second = pane(&workbench, cx);
    assert_ne!(first, second);
    assert_eq!(tabs(&workbench, cx)[1], ["b.txt"]);
    assert_eq!(line("b 3", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "c.txt");
    assert_eq!(pane(&workbench, cx), second);
    assert_eq!(line("b 2", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "b.txt");
    assert_eq!(
        tabs(&workbench, cx),
        [
            vec!["a.txt", "c.txt", "d.txt", "b.txt"],
            vec!["b.txt", "c.txt"]
        ]
    );
    let first_shown = first.read_with(cx, |pane, _| pane.active());
    assert_eq!(first_shown, Some(3));
    // The split's text is the other tab's.
    cx.simulate_keystrokes("x");
    let shared = first.read_with(cx, |pane, cx| {
        pane.tabs()[3]
            .editor()
            .unwrap()
            .read(cx)
            .model()
            .text()
            .to_string()
    });
    assert_eq!(shared, "ne\n");
}

#[gpui::test]
fn name_and_cycle(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("src")).unwrap();
    let files = ["alpha.txt", "alpine.txt", "src/Beta.rs", "xalpha.txt"];
    write(root, &files);
    let (workbench, mut vcx) = window(cx, root, &files);
    let cx = &mut vcx;
    assert_eq!(shown(&workbench, cx), "xalpha.txt");

    // A part of the path, relative to the folder or whole: anywhere, then
    // at the start, then at the end, then whole, until one buffer matches.
    // (Each name is one the temporary folder's own path can't make
    // ambiguous.)
    let cases = [
        (
            "b alp",
            Some("E93: More than one match for alp"),
            "xalpha.txt",
        ),
        ("b alpha", None, "alpha.txt"),
        ("b xal", None, "xalpha.txt"),
        ("b pine.txt", None, "alpine.txt"),
        ("b src/b", None, "Beta.rs"),
        (
            "b q.rs",
            Some("E94: No matching buffer for q.rs"),
            "Beta.rs",
        ),
        // Lower case ignores case; any capital doesn't.
        ("b alpine", None, "alpine.txt"),
        ("b /beta", None, "Beta.rs"),
        (
            "b xALPHA",
            Some("E94: No matching buffer for xALPHA"),
            "Beta.rs",
        ),
        ("b Beta", None, "Beta.rs"),
        (
            "b ALPHA.TXT",
            Some("E94: No matching buffer for ALPHA.TXT"),
            "Beta.rs",
        ),
        (
            "b .txt",
            Some("E93: More than one match for .txt"),
            "Beta.rs",
        ),
    ];
    for (typed, error, file) in cases {
        assert_eq!(line(typed, &workbench, cx).as_deref(), error, ":{typed}");
        assert_eq!(shown(&workbench, cx), file, ":{typed}");
    }
    // The whole path: across the folder's name and the file's.
    let folder = root.file_name().unwrap().to_str().unwrap();
    assert_eq!(line(&format!("b {folder}/alpha"), &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "alpha.txt");
    assert_eq!(line(&format!("b {folder}/src/Be"), &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "Beta.rs");
    let whole = root.join("alpine.txt").display().to_string();
    assert_eq!(line(&format!("b {whole}"), &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "alpine.txt");

    // :bn and :bp go by number and wrap; with a count, that many.
    let cases = [
        ("b 4", "xalpha.txt"),
        ("bn", "alpha.txt"),
        ("bp", "xalpha.txt"),
        ("bN", "Beta.rs"),
        ("bn 2", "alpha.txt"),
        ("bp 3", "alpine.txt"),
        ("3bn", "alpha.txt"),
        ("bn 9", "alpine.txt"),
        ("bl", "xalpha.txt"),
        ("bf", "alpha.txt"),
        ("blast", "xalpha.txt"),
        ("br", "alpha.txt"),
    ];
    for (typed, file) in cases {
        assert_eq!(line(typed, &workbench, cx), None, ":{typed}");
        assert_eq!(shown(&workbench, cx), file, ":{typed}");
    }

    // A deleted buffer is skipped, and isn't matched for :bd.
    assert_eq!(line("bd 2", &workbench, cx), None);
    assert_eq!(line("bn", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "Beta.rs");
    assert_eq!(line("bp", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "alpha.txt");
    assert_eq!(
        line("bd pine.txt", &workbench, cx).as_deref(),
        Some("E94: No matching buffer for pine.txt")
    );
    // :b matches it, once no listed buffer does, and opens it again.
    assert_eq!(line("b pine.txt", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "alpine.txt");
    assert_eq!(
        tabs(&workbench, cx),
        [["alpha.txt", "Beta.rs", "xalpha.txt", "alpine.txt"]]
    );
}

#[gpui::test]
fn delete(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt", "b.txt", "c.txt", "d.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt", "b.txt", "c.txt"]);
    let cx = &mut vcx;
    let window = cx.update(|window, _| window.window_handle());

    // A dirty buffer: E89, unless !.
    cx.simulate_keystrokes("x");
    assert!(dirty(&active(&workbench, cx), cx));
    assert_eq!(
        line("bd", &workbench, cx).as_deref(),
        Some("E89: No write since last change for buffer 3 (add ! to override)")
    );
    assert_eq!(tabs(&workbench, cx), [["a.txt", "b.txt", "c.txt"]]);
    assert_eq!(cx.pending_prompt(), None);
    assert_eq!(line("bd!", &workbench, cx), None);
    assert_eq!(cx.pending_prompt(), None);
    assert_eq!(tabs(&workbench, cx), [["a.txt", "b.txt"]]);
    // The pane shows the buffer it showed before, as nvim's alternate.
    assert_eq!(shown(&workbench, cx), "b.txt");
    assert_eq!(
        std::fs::read_to_string(root.join("c.txt")).unwrap(),
        "one\n"
    );

    // The alternate is the one shown last, not the next tab.
    assert_eq!(line("b 1", &workbench, cx), None);
    assert_eq!(line("e d.txt", &workbench, cx), None);
    assert_eq!(line("b 1", &workbench, cx), None);
    assert_eq!(tabs(&workbench, cx), [["a.txt", "b.txt", "d.txt"]]);
    assert_eq!(line("bd", &workbench, cx), None);
    assert_eq!(tabs(&workbench, cx), [["b.txt", "d.txt"]]);
    assert_eq!(shown(&workbench, cx), "d.txt");
    // ... and # is then the one deleted, opened again.
    assert_eq!(line("b#", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "a.txt");
    assert_eq!(tabs(&workbench, cx), [["b.txt", "d.txt", "a.txt"]]);

    // A buffer not shown: its tab closes, what's shown stays. One not
    // listed: E516.
    assert_eq!(line("bd 2", &workbench, cx), None);
    assert_eq!(tabs(&workbench, cx), [["d.txt", "a.txt"]]);
    assert_eq!(shown(&workbench, cx), "a.txt");
    for typed in ["bd 2", "bd 3", "bd 9"] {
        assert_eq!(
            line(typed, &workbench, cx).as_deref(),
            Some("E516: No buffers were deleted"),
            ":{typed}"
        );
    }

    // Its tabs in every pane close, this pane showing another buffer
    // first; a pane emptied closes, as nvim closes the other windows
    // showing it.
    run("pane.split_right", cx);
    let second = pane(&workbench, cx);
    assert_eq!(
        tabs(&workbench, cx),
        [vec!["d.txt", "a.txt"], vec!["a.txt"]]
    );
    assert_eq!(line("bd 1", &workbench, cx), None);
    assert_eq!(tabs(&workbench, cx), [["d.txt"], ["d.txt"]]);
    assert_eq!(pane(&workbench, cx), second);
    assert_eq!(shown(&workbench, cx), "d.txt");
    // A buffer not shown here: a pane showing nothing else closes.
    run("pane.split_right", cx);
    assert_eq!(line("b 1", &workbench, cx), None);
    assert_eq!(
        tabs(&workbench, cx),
        [vec!["d.txt"], vec!["d.txt"], vec!["d.txt", "a.txt"]]
    );
    assert_eq!(line("bd 4", &workbench, cx), None);
    assert_eq!(tabs(&workbench, cx), [["a.txt"]]);
    assert_eq!(shown(&workbench, cx), "a.txt");
    assert_eq!(line("e d.txt", &workbench, cx), None);
    assert_eq!(line("bd 1", &workbench, cx), None);
    let first = pane(&workbench, cx);

    // An untitled buffer has a number too.
    cx.update(|window, cx| {
        first.update(cx, |pane, cx| {
            pane.add(EditorModel::new(Buffer::new("new"), None), cx);
            let ix = pane.tabs().len() - 1;
            pane.activate(ix, window, cx);
        })
    });
    draw(cx);
    assert_eq!(line("b 4", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "d.txt");
    assert_eq!(line("b 5", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "untitled");
    assert_eq!(line("bd", &workbench, cx), None);
    assert_eq!(tabs(&workbench, cx), [["d.txt"]]);

    // The last one: an empty pane, the window open.
    assert_eq!(line("bd", &workbench, cx), None);
    assert_eq!(tabs(&workbench, cx), [Vec::<String>::new()]);
    assert!(cx.update(|_, cx| cx.windows().contains(&window)));
}

fn focused(editor: &Entity<fe_gpui::EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// Opens `file` from `root` in `pane`, running until it lands, but not
/// drawing.
fn open_in(pane: &Entity<Pane>, root: &std::path::Path, file: &str, cx: &mut VisualTestContext) {
    let path = root.join(file);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    cx.run_until_parked();
}

#[gpui::test]
fn history_without_draws(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt", "b.txt", "c.txt", "d.txt", "x.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt", "b.txt", "c.txt"]);
    let cx = &mut vcx;

    // Several switches with no draw between: # is the one before the last.
    for typed in ["b 1", "b 2", "b 3"] {
        ex(typed, cx);
    }
    ex("b#", cx);
    assert_eq!(shown(&workbench, cx), "b.txt");
    assert_eq!(message(&workbench, cx), None);

    // Tabs opened with no draw between are numbered as they come, not in
    // pane order: d.txt in the split before x.txt in the first pane.
    let first = pane(&workbench, cx);
    run("pane.split_right", cx);
    let second = pane(&workbench, cx);
    open_in(&second, root, "d.txt", cx);
    open_in(&first, root, "x.txt", cx);
    draw(cx);
    assert_eq!(
        tabs(&workbench, cx),
        [
            vec!["a.txt", "b.txt", "c.txt", "x.txt"],
            vec!["b.txt", "d.txt"]
        ]
    );
    let editor = workbench.read_with(cx, |_, cx| {
        let first = first.read(cx);
        first.tabs()[first.active().unwrap()]
            .editor()
            .unwrap()
            .clone()
    });
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    assert_eq!(pane(&workbench, cx), first);
    assert_eq!(line("b 4", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "d.txt");
    assert_eq!(line("b 5", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "x.txt");

    // Saved under another name, a buffer keeps its number; the old name,
    // opened again, gets a new one.
    assert_eq!(line("b 1", &workbench, cx), None);
    assert_eq!(line("sav e.txt", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "e.txt");
    assert_eq!(line("b 2", &workbench, cx), None);
    assert_eq!(line("b 1", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "e.txt");
    assert_eq!(line("b#", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "b.txt");
    assert_eq!(line("e a.txt", &workbench, cx), None);
    assert_eq!(line("b 2", &workbench, cx), None);
    assert_eq!(line("b 6", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "a.txt");
    assert_eq!(line("b e.txt", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "e.txt");
}

/// `:bd` closing a tab another pane shows leaves focus in the line's pane.
#[gpui::test]
fn delete_keeps_focus(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt", "b.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt", "b.txt"]);
    let cx = &mut vcx;
    // The first pane holds a and b, showing a; the split shows a.
    assert_eq!(line("b 1", &workbench, cx), None);
    let first = pane(&workbench, cx);
    run("pane.split_right", cx);
    let second = pane(&workbench, cx);
    assert_eq!(
        tabs(&workbench, cx),
        [vec!["a.txt", "b.txt"], vec!["a.txt"]]
    );

    assert_eq!(line("bd", &workbench, cx), None);
    assert_eq!(tabs(&workbench, cx), [vec!["b.txt"], vec!["b.txt"]]);
    assert_eq!(pane(&workbench, cx), second);
    let shown_here = second.read_with(cx, |pane, _| pane.tabs()[0].editor().unwrap().clone());
    assert!(focused(&shown_here, cx));
    assert_eq!(active(&workbench, cx), shown_here);
    assert_ne!(first, second);
}

/// Saved over a closed buffer's file, a buffer takes the file: the closed
/// one keeps its number but not the file, so the name matches one buffer.
#[gpui::test]
fn save_as_onto_a_closed_buffer(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt", "b.txt", "c.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["a.txt", "b.txt", "c.txt"]);
    let cx = &mut vcx;
    for typed in ["b 1", "bd 2", "sav! b.txt"] {
        assert_eq!(line(typed, &workbench, cx), None, ":{typed}");
    }
    assert_eq!(tabs(&workbench, cx), [["b.txt", "c.txt"]]);
    assert_eq!(line("bd 1", &workbench, cx), None);
    assert_eq!(tabs(&workbench, cx), [["c.txt"]]);
    assert_eq!(line("b b.txt", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "b.txt");
    assert_eq!(line("b 3", &workbench, cx), None);
    assert_eq!(line("b 1", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "b.txt");
    assert_eq!(
        line("b 2", &workbench, cx).as_deref(),
        Some("E86: Buffer 2 does not exist")
    );
}
