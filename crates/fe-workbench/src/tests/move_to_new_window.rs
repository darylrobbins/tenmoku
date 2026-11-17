//! Move to New Window (`pane.move_to_new_window`, the pane's ⋯ menu): the
//! host's opener, faked here as `windows_tests.rs` fakes it, is asked for a
//! window on the same root with the tab's file at its caret, and the tab
//! closes here once the file has landed there, not if it failed to. A tab
//! with unsaved changes asks first; a tab that isn't a file isn't offered
//! the item.

use std::{cell::RefCell, fs, path::PathBuf, rc::Rc};

use fe_command::{CommandId, ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, Modifiers, Task, TestAppContext, VisualTestContext};
use kiln::platform::command_actions::typed_command;
use kiln_l10n::tr;
use kiln_shell::PaneView as _;

use super::open_root;
use crate::{Pane, WindowFile, Workbench};

const ID: &str = "pane.move_to_new_window";

/// What the opener was asked for: each window's root and file.
type Asked = Rc<RefCell<Vec<(Option<PathBuf>, Option<WindowFile>)>>>;

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "alpha\n").unwrap();
    // The caret's column counts chars, as Go to Line does.
    fs::write(dir.path().join("b.txt"), "beta\nwé𝄞rd\n").unwrap();
    dir
}

/// A workbench rooted at `dir` showing `files` opened in turn, the last
/// active, with a fake opener that opens files recording what it's asked.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    dir: &tempfile::TempDir,
    files: &[&str],
) -> (Entity<Workbench>, &'a mut VisualTestContext, Asked) {
    let (workbench, cx) = open_root(cx, dir.path());
    let asked = Asked::default();
    cx.update(|window, cx| {
        let asked = asked.clone();
        crate::set_file_window_opener(cx, move |root, file, _| {
            asked.borrow_mut().push((root, file));
            Task::ready(true)
        });
        window.focus(&workbench.focus_handle(cx), cx);
    });
    for file in files {
        let pane = active(&workbench, cx);
        let path = dir.path().join(file);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        settle(cx);
    }
    (workbench, cx, asked)
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

fn run(command: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(command).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    settle(cx);
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

fn root(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> PathBuf {
    workbench.read_with(cx, |workbench, _| workbench.root().unwrap().to_owned())
}

/// Each pane's tabs, by file name, or `-` for one that isn't a file.
fn tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group(cx).panes().iter())
            .map(|pane| {
                (pane.read(cx).tabs().iter())
                    .map(|tab| match &tab.path {
                        Some(path) => path.file_name().unwrap().to_string_lossy().into_owned(),
                        None => "-".to_owned(),
                    })
                    .collect()
            })
            .collect()
    })
}

fn editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown")
}

/// The ⋯ menu's rows for the active pane, as `(title, command)`, then
/// dismissed.
fn menu(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, &'static str)> {
    let pane = active(workbench, cx);
    let at = pane.read_with(cx, |pane, _| {
        let ix = (pane.drawn_actions().iter())
            .position(|(id, _)| *id == "pane-menu")
            .expect("⋯ is drawn");
        pane.action_bounds(ix).expect("drawn")
    });
    cx.simulate_click(at.center(), Modifiers::none());
    settle(cx);
    let rows = workbench.read_with(cx, |workbench, cx| {
        let picker = &workbench.toolbar_picker().expect("shown").read(cx).delegate;
        (picker.labels().into_iter().zip(picker.commands())).collect()
    });
    cx.simulate_keystrokes("escape");
    settle(cx);
    rows
}

/// What the opener was asked for, its paths resolved, as the root is.
fn asked_for(asked: &Asked) -> Vec<(Option<PathBuf>, Option<WindowFile>)> {
    let real = |path: &PathBuf| dunce::canonicalize(path).unwrap();
    (asked.borrow().iter())
        .map(|(root, file)| {
            let file = file.clone().map(|file| WindowFile {
                path: real(&file.path),
                ..file
            });
            (root.as_ref().map(real), file)
        })
        .collect()
}

fn item() -> String {
    tr!("workbench-pane-action-move-to-new-window").to_string()
}

#[gpui::test]
fn the_new_window_gets_the_root_file_and_position_and_the_tab_closes_here(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx, asked) = opened(cx, &dir, &["a.txt", "b.txt"]);
    let root = dunce::canonicalize(root(&workbench, cx)).unwrap();
    // The caret after "wé𝄞", on line 2: column 4 in chars.
    let editor = editor(&workbench, cx);
    let at = "beta\nwé𝄞".len();
    editor.update(cx, |editor, cx| editor.select_and_reveal(at..at, cx));

    run(ID, cx);
    let file = WindowFile {
        path: root.join("b.txt"),
        line: Some(2),
        col: Some(4),
    };
    assert_eq!(asked_for(&asked), [(Some(root.clone()), Some(file))]);
    assert_eq!(tabs(&workbench, cx), [["a.txt"]]);
    assert!(!cx.has_pending_prompt());

    // An opener that only opens folders can't take the file: it stays.
    cx.update(|_, cx| crate::set_window_opener(cx, |_, _| panic!("not asked")));
    run(ID, cx);
    assert_eq!(tabs(&workbench, cx), [["a.txt"]]);
    assert_eq!(asked.borrow().len(), 1);
}

#[gpui::test]
fn a_tab_closes_only_once_its_file_has_landed_in_the_new_window(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx, _) = opened(cx, &dir, &["a.txt", "b.txt"]);
    // An opener whose windows land when the test says, true or false.
    let (land, lands) = async_channel::unbounded::<bool>();
    let asked = Rc::new(RefCell::new(0));
    cx.update(|_, cx| {
        let asked = asked.clone();
        crate::set_file_window_opener(cx, move |_, _, cx| {
            *asked.borrow_mut() += 1;
            let lands = lands.clone();
            cx.spawn(async move |_| lands.recv().await.unwrap_or(false))
        });
    });

    // Until it lands, the tab stays, and asking again opens no second window.
    run(ID, cx);
    run(ID, cx);
    assert_eq!(*asked.borrow(), 1);
    assert_eq!(tabs(&workbench, cx), [["a.txt", "b.txt"]]);

    // The window opened, but the file didn't open in it: the tab stays.
    land.try_send(false).unwrap();
    settle(cx);
    assert_eq!(tabs(&workbench, cx), [["a.txt", "b.txt"]]);

    // Edited while the window opens: it stays, with its changes.
    run(ID, cx);
    assert_eq!(*asked.borrow(), 2);
    let editor = editor(&workbench, cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_input("x");
    settle(cx);
    land.try_send(true).unwrap();
    settle(cx);
    assert_eq!(tabs(&workbench, cx), [["a.txt", "b.txt"]]);
    assert!(!cx.has_pending_prompt());

    // Saved, then moved: it lands, and only then does the tab close.
    cx.simulate_keystrokes("cmd-s");
    settle(cx);
    assert!(!editor.read_with(cx, |editor, _| editor.model().is_dirty()));
    run(ID, cx);
    assert_eq!(*asked.borrow(), 3);
    assert_eq!(tabs(&workbench, cx), [["a.txt", "b.txt"]]);
    land.try_send(true).unwrap();
    settle(cx);
    assert_eq!(tabs(&workbench, cx), [["a.txt"]]);
}

#[gpui::test]
fn a_dirty_tab_asks_save_and_move_or_cancel(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx, asked) = opened(cx, &dir, &["a.txt", "b.txt"]);
    let root = dunce::canonicalize(root(&workbench, cx)).unwrap();
    let editor = editor(&workbench, cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    editor.update(cx, |editor, cx| editor.select_and_reveal(0..0, cx));
    cx.simulate_input("new ");
    settle(cx);
    assert!(editor.read_with(cx, |editor, _| editor.model().is_dirty()));

    // Cancelled: nothing opens, nothing is saved, the tab stays.
    run(ID, cx);
    let (question, _) = cx.pending_prompt().expect("asked");
    let expected = tr!("workbench-pane-move-question", name = "b.txt");
    assert_eq!(question, expected.to_string());
    cx.simulate_prompt_answer(tr!("workbench-pane-cancel").as_ref());
    settle(cx);
    assert!(asked.borrow().is_empty());
    assert_eq!(tabs(&workbench, cx), [["a.txt", "b.txt"]]);
    assert_eq!(
        fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "beta\nwé𝄞rd\n"
    );
    assert!(editor.read_with(cx, |editor, _| editor.model().is_dirty()));

    // Saved, then moved: the new window reads the saved file, at the caret.
    run(ID, cx);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer(tr!("workbench-pane-save-and-move").as_ref());
    settle(cx);
    assert_eq!(
        fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "new beta\nwé𝄞rd\n"
    );
    let file = WindowFile {
        path: root.join("b.txt"),
        line: Some(1),
        col: Some(5),
    };
    assert_eq!(asked_for(&asked), [(Some(root), Some(file))]);
    assert_eq!(tabs(&workbench, cx), [["a.txt"]]);
}

#[gpui::test]
fn a_non_file_tab_has_no_item(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx, asked) = opened(cx, &dir, &["a.txt"]);
    assert!(menu(&workbench, cx).iter().any(|row| row.1 == ID));

    // The Settings tab, then an untitled file: no item, and the command
    // does nothing.
    run("workbench.open_settings_ui", cx);
    assert_eq!(tabs(&workbench, cx), [["a.txt", "-"]]);
    run("workbench.new_file", cx);
    assert_eq!(tabs(&workbench, cx), [["a.txt", "-", "-"]]);
    for _ in 0..2 {
        let rows = menu(&workbench, cx);
        assert!(!rows.iter().any(|row| row.1 == ID || row.0 == item()));
        run(ID, cx);
        assert!(asked.borrow().is_empty());
        assert_eq!(tabs(&workbench, cx), [["a.txt", "-", "-"]]);
        // Back to the Settings tab.
        let pane = active(&workbench, cx);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate_item(1, window, cx)));
        settle(cx);
    }
}

#[gpui::test]
fn the_last_tab_keeps_its_window_open(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx, asked) = opened(cx, &dir, &["a.txt"]);
    let windows = cx.update(|_, cx| cx.windows().len());

    // In a split, the pane it leaves empty closes, as Close Tab's does.
    run("pane.split_right", cx);
    assert_eq!(tabs(&workbench, cx), [["a.txt"], ["a.txt"]]);
    run(ID, cx);
    assert_eq!(tabs(&workbench, cx), [["a.txt"]]);

    // The window's last tab moves too; the window stays, its pane empty.
    run(ID, cx);
    assert_eq!(asked.borrow().len(), 2);
    assert_eq!(tabs(&workbench, cx), [Vec::<String>::new()]);
    assert_eq!(cx.update(|_, cx| cx.windows().len()), windows);
    assert!(workbench.read_with(cx, |workbench, cx| workbench.pane_group(cx).panes().len()) == 1);
}

#[gpui::test]
fn the_menu_item_and_command_exist(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx, _) = opened(cx, &dir, &["a.txt"]);

    // The design's place: after Maximize, before Move to the Other Split.
    let rows = menu(&workbench, cx);
    let ix = rows.iter().position(|row| row.1 == ID).expect("listed");
    assert_eq!(rows[ix].0, item());
    assert_eq!(rows[ix - 1].1, "pane.toggle_maximize");
    assert_eq!(rows[ix + 1].1, "pane.exchange");

    // A typed command of the workbench's, titled from the catalogue: the
    // palette's "Pane: Move to New Window".
    cx.update(|_, cx| {
        let command = typed_command(&CommandId::parse(ID).unwrap(), cx).expect("typed");
        let title = tr!("command-pane-move_to_new_window").to_string();
        assert_eq!(command.title.to_string(), title);
        assert_eq!(
            command.context,
            Some(fe_command::ContextPredicate::identifier("workbench"))
        );
    });
    assert_eq!(
        fe_command::l10n::command_label(ID, "").to_string(),
        tr!("command-pane-move_to_new_window").to_string()
    );
}
