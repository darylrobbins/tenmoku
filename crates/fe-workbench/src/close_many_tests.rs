//! Close Saved Editors, Close Editors in Group and Close All Editors
//! ([`crate::close_many`]): saved tabs close at once; the others once
//! their tabs agree, a dirty one asking to discard, and a Cancel keeping
//! every tab open.

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Pane, Workbench};

const FILES: [&str; 4] = ["a.txt", "b.txt", "c.txt", "d.txt"];

/// A workbench on a folder of `a.txt` to `d.txt`, all open in one pane in
/// order, `d.txt` active.
fn opened(cx: &mut TestAppContext) -> (tempfile::TempDir, Entity<Workbench>, VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    for name in FILES {
        std::fs::write(dir.path().join(name), "one\n").unwrap();
    }
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
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
    for name in FILES {
        let pane = active_pane(&workbench, cx);
        let path = dir.path().join(name);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    (dir, workbench, vcx)
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

/// Each pane's tabs' file names, and its active tab's.
fn tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(Vec<String>, String)> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.panes(cx).iter())
            .map(|pane| {
                let pane = pane.read(cx);
                let names: Vec<String> = (pane.tabs().iter())
                    .map(|tab| fe_gpui::pane_item::file_name(tab.path.as_deref()))
                    .collect();
                let active = pane.active().map(|ix| names[ix].clone());
                (names, active.unwrap_or_default())
            })
            .collect()
    })
}

/// `names` and `active` as [`tabs`] has a pane.
fn pane_tabs(names: &[&str], active: &str) -> (Vec<String>, String) {
    let names = names.iter().map(|name| name.to_string()).collect();
    (names, active.to_owned())
}

/// Activates the active pane's tab for `name`.
fn show(workbench: &Entity<Workbench>, name: &str, cx: &mut VisualTestContext) {
    let pane = active_pane(workbench, cx);
    let ix = pane.read_with(cx, |pane, _| {
        (pane.tabs().iter())
            .position(|tab| fe_gpui::pane_item::file_name(tab.path.as_deref()) == name)
            .expect("the tab is open")
    });
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.activate(ix, window, cx);
            cx.notify();
        })
    });
    draw(cx);
}

fn editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    let pane = active_pane(workbench, cx);
    pane.read_with(cx, |pane, _| pane.active_editor().cloned())
        .expect("an editor is shown")
}

/// Gives the active tab unsaved changes.
fn change(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let editor = editor(workbench, cx);
    editor.update(cx, |editor, cx| {
        editor.model_mut().insert("x");
        cx.notify();
    });
    draw(cx);
    assert!(editor.read_with(cx, |editor, _| editor.model().is_dirty()));
}

fn prompt(cx: &mut VisualTestContext) -> Option<String> {
    cx.pending_prompt().map(|(question, _)| question)
}

fn answer(button: &str, cx: &mut VisualTestContext) {
    cx.simulate_prompt_answer(button);
    draw(cx);
}

#[gpui::test]
fn close_saved(cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    // d, shown in a pane split off to the right, stays there.
    run("pane.split_right", cx);
    run("pane.focus_left", cx);
    show(&workbench, "b.txt", cx);
    change(&workbench, cx);
    show(&workbench, "c.txt", cx);

    run("workbench.close_saved", cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(
        tabs(&workbench, cx),
        [
            pane_tabs(&["b.txt"], "b.txt"),
            pane_tabs(&["d.txt"], "d.txt"),
        ]
    );

    // The active tab survives: it has changes.
    run("workbench.close_saved", cx);
    assert_eq!(tabs(&workbench, cx)[0], pane_tabs(&["b.txt"], "b.txt"));
}

#[gpui::test]
fn close_in_group(cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    // c is shown in a pane split off to the right too.
    show(&workbench, "c.txt", cx);
    run("pane.split_right", cx);
    run("pane.focus_left", cx);
    show(&workbench, "b.txt", cx);
    change(&workbench, cx);

    run("workbench.close_group_editors", cx);
    assert_eq!(prompt(cx), Some("Discard unsaved changes to b.txt?".into()));
    answer("Cancel", cx);
    assert_eq!(tabs(&workbench, cx)[0].0, FILES);
    assert_eq!(prompt(cx), None);

    run("workbench.close_group_editors", cx);
    answer("Discard", cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(
        tabs(&workbench, cx),
        [pane_tabs(&[], ""), pane_tabs(&["c.txt"], "c.txt")]
    );
}

/// The closed editors' file names, oldest first.
fn history(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let pane = active_pane(workbench, cx);
    pane.read_with(cx, |pane, _| {
        (pane.closed_editors().borrow().paths().iter())
            .map(|path| fe_gpui::pane_item::file_name(Some(path)))
            .collect()
    })
}

#[gpui::test]
fn close_saved_shows_the_nearest_survivor(cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    // a and d have changes; b, active, and c are saved.
    show(&workbench, "a.txt", cx);
    change(&workbench, cx);
    show(&workbench, "d.txt", cx);
    change(&workbench, cx);
    show(&workbench, "b.txt", cx);

    run("workbench.close_saved", cx);
    assert_eq!(
        tabs(&workbench, cx),
        [pane_tabs(&["a.txt", "d.txt"], "a.txt")]
    );
}

#[gpui::test]
fn close_saved_keeps_an_emptied_pane(cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    run("pane.split_right", cx);
    run("pane.focus_left", cx);

    run("workbench.close_saved", cx);
    assert_eq!(
        tabs(&workbench, cx),
        [pane_tabs(&[], ""), pane_tabs(&["d.txt"], "d.txt")]
    );
}

#[gpui::test]
fn close_in_group_asks_for_no_buffer_another_pane_shows(cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    // b has changes, and a pane split off to the right shows it too.
    show(&workbench, "b.txt", cx);
    change(&workbench, cx);
    run("pane.split_right", cx);
    run("pane.focus_left", cx);

    run("workbench.close_group_editors", cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(
        tabs(&workbench, cx),
        [pane_tabs(&[], ""), pane_tabs(&["b.txt"], "b.txt")]
    );
}

#[gpui::test]
fn the_chords_close_tabs(cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    show(&workbench, "a.txt", cx);
    change(&workbench, cx);
    cx.simulate_keystrokes("cmd-k u");
    draw(cx);
    assert_eq!(tabs(&workbench, cx), [pane_tabs(&["a.txt"], "a.txt")]);
    cx.simulate_keystrokes("cmd-k w");
    draw(cx);
    assert_eq!(prompt(cx), Some("Discard unsaved changes to a.txt?".into()));
}

#[gpui::test]
fn close_all(cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    // b has changes, and both panes show it.
    show(&workbench, "b.txt", cx);
    change(&workbench, cx);
    run("pane.split_right", cx);

    run("workbench.close_all_editors", cx);
    assert_eq!(prompt(cx), Some("Discard unsaved changes to b.txt?".into()));
    answer("Discard", cx);
    assert_eq!(prompt(cx), None, "asked once for the buffer");
    assert_eq!(
        tabs(&workbench, cx),
        [pane_tabs(&[], ""), pane_tabs(&[], "")]
    );

    // Every file went on the stack; the last closed comes back.
    assert_eq!(
        history(&workbench, cx),
        ["a.txt", "c.txt", "d.txt", "b.txt"],
        "b closed last"
    );
    run("workbench.reopen_closed_editor", cx);
    let reopened: Vec<_> = tabs(&workbench, cx).into_iter().flat_map(|p| p.0).collect();
    assert_eq!(reopened, ["b.txt"]);
}
