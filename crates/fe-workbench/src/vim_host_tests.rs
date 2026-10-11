//! Vim's file and window commands typed on the status bar's line, run as
//! fastedit's: `:w` saves, `:q` closes a tab or gives `E37`, never with
//! the Discard/Cancel prompt, a window's last tab closing closes it, and
//! `:wa`, `:qa` and `:e` act on the line's window and pane.

use std::path::{Path, PathBuf};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand, set_vim_enabled};
use gpui::{
    AnyWindowHandle, Entity, Focusable as _, SharedString, TestAppContext, VisualTestContext,
};

use crate::{Launch, StatusItem, StatusSide, Workbench};

const TEXT: &str = "one\n";
const E37: &str = "E37: No write since last change (add ! to override)";

/// Writes `files`, each holding [`TEXT`], in `dir`.
fn write(dir: &Path, files: &[&str]) {
    for file in files {
        std::fs::write(dir.join(file), TEXT).unwrap();
    }
}

/// A window of a workbench rooted at `root`, active, showing `files` from
/// it opened in order, the last focused, with vim on.
fn window(
    cx: &mut TestAppContext,
    root: &Path,
    files: &[&str],
) -> (Entity<Workbench>, VisualTestContext) {
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
    for file in files {
        open(&workbench, root.join(file), cx);
    }
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    (workbench, vcx)
}

/// Opens `path` in the active pane, focused.
fn open(workbench: &Entity<Workbench>, path: PathBuf, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Types `:{line}` and Enter.
fn ex(line: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(":");
    cx.simulate_input(line);
    cx.simulate_keystrokes("enter");
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

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a tab is shown")
}

fn focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

fn dirty(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |editor, _| editor.model().is_dirty())
}

/// Each pane's tabs, by file name.
fn tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group().panes().iter())
            .map(|pane| {
                (pane.read(cx).tabs().iter())
                    .map(|tab| fe_gpui::pane_item::file_name(tab.path.as_deref()))
                    .collect()
            })
            .collect()
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

fn prompt(cx: &mut VisualTestContext) -> Option<String> {
    cx.pending_prompt().map(|(question, _)| question)
}

fn window_open(window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
    cx.update(|cx| cx.windows().contains(&window))
}

fn handle(cx: &mut VisualTestContext) -> AnyWindowHandle {
    cx.update(|window, _| window.window_handle())
}

fn on_disk(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[gpui::test]
fn tab_commands(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["keep.txt", "a.txt", "b.txt", "c.txt", "d.txt"]);
    let (workbench, mut vcx) = window(cx, root, &["keep.txt", "a.txt"]);
    let cx = &mut vcx;

    // :w saves.
    let a = active(&workbench, cx);
    cx.simulate_keystrokes("x");
    assert!(dirty(&a, cx));
    ex("w", cx);
    assert!(!dirty(&a, cx));
    assert_eq!(on_disk(&root.join("a.txt")), "ne\n");
    assert_eq!(message(&workbench, cx), None);

    // :q on a dirty tab: E37, with no prompt.
    cx.simulate_keystrokes("x");
    ex("q", cx);
    assert_eq!(message(&workbench, cx).as_deref(), Some(E37));
    assert_eq!(prompt(cx), None);
    assert_eq!(tabs(&workbench, cx), [["keep.txt", "a.txt"]]);

    // :q! discards.
    ex("q!", cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(tabs(&workbench, cx), [["keep.txt"]]);
    assert_eq!(on_disk(&root.join("a.txt")), "ne\n");

    // :wq saves and closes.
    open(&workbench, root.join("b.txt"), cx);
    cx.simulate_keystrokes("x");
    ex("wq", cx);
    assert_eq!(on_disk(&root.join("b.txt")), "ne\n");
    assert_eq!(tabs(&workbench, cx), [["keep.txt"]]);

    // :x on a clean tab closes it without writing.
    let c = root.join("c.txt");
    open(&workbench, c.clone(), cx);
    let written = std::fs::metadata(&c).unwrap().modified().unwrap();
    ex("x", cx);
    assert_eq!(tabs(&workbench, cx), [["keep.txt"]]);
    assert_eq!(std::fs::metadata(&c).unwrap().modified().unwrap(), written);

    // A dirty tab another pane shows closes on :q: its changes live on.
    open(&workbench, root.join("d.txt"), cx);
    cx.simulate_keystrokes("x");
    run("pane.split_right", cx);
    let split = active(&workbench, cx);
    assert!(focused(&split, cx));
    assert_eq!(
        tabs(&workbench, cx),
        [vec!["keep.txt", "d.txt"], vec!["d.txt"]]
    );
    ex("q", cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(tabs(&workbench, cx), [["keep.txt", "d.txt"]]);
    assert!(dirty(&active(&workbench, cx), cx));

    // :w on an untitled tab.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.add(EditorModel::new(Buffer::new("new"), None), cx);
            let ix = pane.tabs().len() - 1;
            pane.activate(ix, window, cx);
        })
    });
    draw(cx);
    ex("w", cx);
    assert_eq!(
        message(&workbench, cx).as_deref(),
        Some("E32: No file name")
    );
}

#[gpui::test]
fn last_tab_closes_its_window(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt", "b.txt", "c.txt", "d.txt"]);
    let (other, mut other_cx) = window(cx, root, &["b.txt", "c.txt"]);
    let other_window = handle(&mut other_cx);

    let (_, mut vcx) = window(cx, root, &["a.txt"]);
    let closing = handle(&mut vcx);
    ex("q", &mut vcx);
    assert!(!window_open(closing, cx));
    assert!(window_open(other_window, cx));
    assert_eq!(tabs(&other, &mut other_cx), [["b.txt", "c.txt"]]);

    // A dirty last tab gives E37 like any; :q! closes the window.
    let (workbench, mut vcx) = window(cx, root, &["d.txt"]);
    let window = handle(&mut vcx);
    vcx.simulate_keystrokes("x");
    ex("q", &mut vcx);
    assert_eq!(message(&workbench, &mut vcx).as_deref(), Some(E37));
    assert_eq!(prompt(&mut vcx), None);
    assert!(window_open(window, cx));
    ex("q!", &mut vcx);
    assert_eq!(prompt(&mut other_cx), None);
    assert!(!window_open(window, cx));
    assert!(window_open(other_window, cx));
    assert_eq!(on_disk(&root.join("d.txt")), TEXT);
}

#[gpui::test]
fn window_commands(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, &["a.txt", "b.txt", "c.txt", "d.txt", "other.txt"]);
    let (second, mut second_cx) = window(cx, root, &["d.txt"]);
    let second_window = handle(&mut second_cx);
    second_cx.simulate_keystrokes("x");
    let (workbench, mut vcx) = window(cx, root, &["a.txt", "b.txt", "c.txt"]);
    let first_window = handle(&mut vcx);
    let cx1 = &mut vcx;

    // :wa saves this window's dirty tabs only.
    let pane = workbench.read_with(cx1, |workbench, _| workbench.pane().clone());
    let activate = |ix: usize, cx: &mut VisualTestContext| {
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(ix, window, cx)));
        draw(cx);
    };
    activate(0, cx1);
    cx1.simulate_keystrokes("x");
    activate(1, cx1);
    cx1.simulate_keystrokes("x");
    activate(2, cx1);
    ex("wa", cx1);
    assert_eq!(on_disk(&root.join("a.txt")), "ne\n");
    assert_eq!(on_disk(&root.join("b.txt")), "ne\n");
    assert_eq!(on_disk(&root.join("d.txt")), TEXT);
    assert!(dirty(&active(&second, &mut second_cx), &mut second_cx));
    assert_eq!(message(&workbench, cx1), None);

    // :qa with a dirty tab closes nothing.
    activate(1, cx1);
    cx1.simulate_keystrokes("x");
    ex("qa", cx1);
    assert_eq!(message(&workbench, cx1).as_deref(), Some(E37));
    activate(2, cx1);
    ex("qa", cx1);
    let e162 = "E162: No write since last change for buffer \"b.txt\"";
    assert_eq!(message(&workbench, cx1).as_deref(), Some(e162));
    assert_eq!(prompt(cx1), None);
    assert_eq!(tabs(&workbench, cx1), [["a.txt", "b.txt", "c.txt"]]);

    // :e opens files in the tab's pane, by path.
    ex("e other.txt", cx1);
    draw(cx1);
    let editor = active(&workbench, cx1);
    let path = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned))
    };
    assert_eq!(path(&editor, cx1), Some(root.join("other.txt")));
    assert_eq!(
        editor.read_with(cx1, |editor, _| editor.model().text().to_string()),
        TEXT
    );
    let elsewhere = tempfile::tempdir().unwrap();
    let absolute = elsewhere.path().join("abs.txt");
    std::fs::write(&absolute, TEXT).unwrap();
    ex(&format!("e {}", absolute.display()), cx1);
    draw(cx1);
    assert_eq!(path(&active(&workbench, cx1), cx1), Some(absolute));
    ex("e missing.txt", cx1);
    draw(cx1);
    let missing = active(&workbench, cx1);
    assert_eq!(path(&missing, cx1), Some(root.join("missing.txt")));
    assert_eq!(
        missing.read_with(cx1, |editor, _| editor.model().text().to_string()),
        ""
    );
    assert!(!root.join("missing.txt").exists());
    assert_eq!(
        tabs(&workbench, cx1),
        [[
            "a.txt",
            "b.txt",
            "c.txt",
            "other.txt",
            "abs.txt",
            "missing.txt"
        ]]
    );
    for line in ["e", "e!"] {
        ex(line, cx1);
        let reload = "Not supported yet: reloading a file from disk";
        assert_eq!(message(&workbench, cx1).as_deref(), Some(reload), ":{line}");
    }

    // :qa! closes only its window, discarding.
    ex("qa!", &mut second_cx);
    assert_eq!(prompt(cx1), None);
    assert!(!window_open(second_window, cx));
    assert!(window_open(first_window, cx));
    assert_eq!(on_disk(&root.join("d.txt")), TEXT);

    // :qa with nothing dirty closes its window.
    ex("wa", cx1);
    ex("qa", cx1);
    assert_eq!(prompt(cx1), None);
    assert!(!window_open(first_window, cx));
}
