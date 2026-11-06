//! A window's layout saved and restored (`app-shell.md` §4.6): the same
//! file in two split panes, each editor with its own caret, and the dock
//! hidden. Restored, the two editors share a buffer again, an edit in one
//! shows in the other, each keeps its caret, and closing one while it's
//! dirty asks nothing, as the other still shows the buffer.

use std::path::Path;

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::layout::{LayoutStore, WindowLayout};

use crate::{Launch, Workbench};

fn open(cx: &mut TestAppContext, launch: Launch) -> (Entity<Workbench>, &mut VisualTestContext) {
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx)
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

/// Each pane's editors.
fn editors(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<Vec<Entity<EditorView>>> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group(cx).panes().iter())
            .map(|pane| {
                pane.read(cx)
                    .tabs()
                    .iter()
                    .map(|tab| tab.editor().unwrap().clone())
                    .collect()
            })
            .collect()
    })
}

fn caret(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    editor.read_with(cx, |editor, _| editor.model().primary_selection().head())
}

fn place(editor: &Entity<EditorView>, at: usize, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| editor.select_and_reveal(at..at, cx));
}

#[gpui::test]
fn two_split_views_of_a_file_round_trip(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("a.txt");
    std::fs::write(&path, "alpha\nbeta\n").unwrap();
    let root = dunce::canonicalize(&root).unwrap();
    let store = LayoutStore::new(dir.path().join("layouts"), &root);
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });

    let launch = Launch {
        root: Some(root.clone()),
        file: Some((
            root.join("a.txt"),
            Some(Buffer::new("alpha\nbeta\n").into()),
        )),
    };
    let mut app = cx.clone();
    let (first, cx) = open(&mut app, launch);
    run("pane.split_right", cx);
    run("dock.toggle", cx);
    let [left, right] = <[_; 2]>::try_from(editors(&first, cx)).unwrap();
    place(&left[0], 2, cx);
    place(&right[0], 8, cx);
    let claim = store.claim_new().unwrap();
    first.update_in(cx, |workbench, window, cx| {
        workbench.set_layout(WindowLayout::new(store.clone(), claim), window, cx)
    });
    draw(cx);
    assert_eq!(store.entries().len(), 1);

    // The last window closes: its entry stays, in full.
    cx.update(|window, _| window.remove_window());
    drop(first);
    cx.run_until_parked();
    let entries = store.entries();
    assert_eq!(entries.len(), 1);
    let panes = &entries[0]["panes"]["tree"]["members"];
    assert_eq!(panes[1]["pane"]["items"][0]["state"]["caret"], 8);

    // Restored in a new window.
    let mut app = app.clone();
    let launch = Launch {
        root: Some(root.clone()),
        file: None,
    };
    let (second, cx) = open(&mut app, launch);
    let (claim, entry) = store.restorable().pop().expect("the entry restores");
    second.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx);
        workbench.set_layout(WindowLayout::new(store.clone(), claim), window, cx);
    });
    draw(cx);
    let [left, right] = <[_; 2]>::try_from(editors(&second, cx)).expect("two panes");
    let (one, two) = (left[0].clone(), right[0].clone());
    let shared = cx.update(|_, cx| {
        one.read(cx)
            .model()
            .shares_buffer_with(two.read(cx).model())
    });
    assert!(shared);
    assert_eq!((caret(&one, cx), caret(&two, cx)), (2, 8));
    assert!(!second.read_with(cx, |workbench, cx| workbench.dock().read(cx).is_visible()));
    assert_eq!(
        one.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned)),
        Some(root.join("a.txt"))
    );

    // An edit in one shows in the other; dirty, one closes without asking.
    let active = second.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    assert_eq!(active, Some(two.clone()));
    cx.simulate_input("x");
    draw(cx);
    let text = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| editor.model().text().to_string())
    };
    assert_eq!(text(&one, cx), "alpha\nbexta\n");
    assert_eq!(text(&two, cx), "alpha\nbexta\n");
    run("workbench.close_tab", cx);
    assert!(!cx.has_pending_prompt());
    assert_eq!(editors(&second, cx).len(), 1);
}
