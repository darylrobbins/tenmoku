//! Close Other Tabs (`workbench.close_other_tabs`, as VS Code's
//! `workbench.action.closeOtherEditors`): every tab of the active pane
//! but the active one closes as Close Tab closes it, so one with changes
//! stays, and closed files go on the closed-editor stack. Vim's `:tabo`
//! closes them through the same [`Pane::close_others`].

use std::path::{Path, PathBuf};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand, set_vim_enabled};
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

#[gpui::test]
fn closes_other_clean_tabs(cx: &mut TestAppContext) {
    let (dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    // A pane split off to the right, with d and then a in it.
    run("pane.split_right", cx);
    let right = active_pane(&workbench, cx);
    let a = dir.path().join("a.txt");
    cx.update(|window, cx| right.update(cx, |pane, cx| pane.open(a, window, cx)));
    draw(cx);
    run("pane.focus_left", cx);
    show(&workbench, "c.txt", cx);

    run("workbench.close_other_tabs", cx);
    assert_eq!(
        tabs(&workbench, cx),
        [
            pane_tabs(&["c.txt"], "c.txt"),
            pane_tabs(&["d.txt", "a.txt"], "a.txt"),
        ]
    );
    let left = workbench.read_with(cx, |workbench, cx| workbench.panes(cx)[0].clone());
    assert_eq!(active_pane(&workbench, cx), left);
}

#[gpui::test]
fn dirty_tabs_stay(cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    // a has changes; so has b, which a pane split off to the right shows
    // too.
    show(&workbench, "a.txt", cx);
    change(&workbench, cx);
    show(&workbench, "b.txt", cx);
    change(&workbench, cx);
    run("pane.split_right", cx);
    run("pane.focus_left", cx);
    // Close Tab leaves a's tab open.
    show(&workbench, "a.txt", cx);
    run("workbench.close_tab", cx);
    assert_eq!(
        tabs(&workbench, cx)[0],
        pane_tabs(&FILES, "a.txt"),
        "Close Tab keeps a tab with changes"
    );
    // The active tab stays though it has changes too.
    show(&workbench, "c.txt", cx);
    change(&workbench, cx);

    run("workbench.close_other_tabs", cx);
    assert_eq!(
        tabs(&workbench, cx),
        [
            pane_tabs(&["a.txt", "c.txt"], "c.txt"),
            pane_tabs(&["b.txt"], "b.txt"),
        ]
    );
    // b's changes live on in the other pane.
    run("pane.focus_right", cx);
    let b = editor(&workbench, cx);
    assert!(b.read_with(cx, |editor, _| editor.model().is_dirty()));

    // A single tab: nothing to close.
    run("workbench.close_other_tabs", cx);
    assert_eq!(tabs(&workbench, cx)[1], pane_tabs(&["b.txt"], "b.txt"));
    // Again, with the kept tab the only other one: it stays.
    run("pane.focus_left", cx);
    run("workbench.close_other_tabs", cx);
    assert_eq!(
        tabs(&workbench, cx)[0],
        pane_tabs(&["a.txt", "c.txt"], "c.txt")
    );
}

#[gpui::test]
fn pushes_closed_editor_stack(cx: &mut TestAppContext) {
    let (dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    show(&workbench, "c.txt", cx);
    run("workbench.close_other_tabs", cx);
    assert_eq!(tabs(&workbench, cx), [pane_tabs(&["c.txt"], "c.txt")]);

    let path = |name: &str| dir.path().join(name);
    let pane = active_pane(&workbench, cx);
    let stack = pane.read_with(cx, |pane, _| pane.closed_editors().borrow().paths());
    assert_eq!(stack, [path("a.txt"), path("b.txt"), path("d.txt")]);

    // Reopened most recently closed first.
    let mut reopened: Vec<PathBuf> = Vec::new();
    for _ in 0..3 {
        run("workbench.reopen_closed_editor", cx);
        let editor = editor(&workbench, cx);
        reopened.push(editor.read_with(cx, |editor, _| {
            editor.model().path().map(Path::to_owned).unwrap()
        }));
    }
    assert_eq!(reopened, [path("d.txt"), path("b.txt"), path("a.txt")]);
}

/// Types `:{line}` and Enter in the active editor, and returns its vim
/// message.
fn ex(line: &str, workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    cx.simulate_keystrokes(":");
    cx.simulate_input(line);
    cx.simulate_keystrokes("enter");
    draw(cx);
    let editor = editor(workbench, cx);
    editor.read_with(cx, |editor, _| {
        let vim = editor.vim_state()?;
        vim.message().map(str::to_owned)
    })
}

#[gpui::test]
fn shares_core_with_tabo(cx: &mut TestAppContext) {
    // One close-others loop, Pane::close_others, which `:tabo` calls too.
    let tabs_source = include_str!("vim_tabs.rs");
    let tab_only = tabs_source
        .split("fn vim_tab_only(")
        .nth(1)
        .and_then(|rest| rest.split("\n    }\n").next())
        .expect("vim_tabs.rs has vim_tab_only");
    assert!(tab_only.contains(".close_others(force, "), "{tab_only}");
    assert!(!tab_only.contains("take_item"), "{tab_only}");
    assert!(!tab_only.contains("for "), "{tab_only}");
    let commands = include_str!("workbench_commands.rs");
    assert!(commands.contains("pane.close_others(false, window, cx)"));

    // :tabo keeps a tab with changes, as the command does, with E445;
    // :tabo! closes it.
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    show(&workbench, "b.txt", cx);
    change(&workbench, cx);
    show(&workbench, "c.txt", cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    assert_eq!(
        ex("tabo", &workbench, cx).as_deref(),
        Some("E445: Other tab page contains changes")
    );
    assert_eq!(
        tabs(&workbench, cx),
        [pane_tabs(&["b.txt", "c.txt"], "c.txt")]
    );
    assert_eq!(ex("tabo!", &workbench, cx), None);
    assert_eq!(tabs(&workbench, cx), [pane_tabs(&["c.txt"], "c.txt")]);
    assert_eq!(
        ex("tabo", &workbench, cx).as_deref(),
        Some("Already only one tab page")
    );
}

/// Opens a second tab of the active editor's buffer at the end of the
/// active pane, and shows the tab for `then`.
fn copy_active(workbench: &Entity<Workbench>, then: &str, cx: &mut VisualTestContext) {
    let model = editor(workbench, cx).read_with(cx, |editor, _| editor.model().split());
    let pane = active_pane(workbench, cx);
    pane.update(cx, |pane, cx| pane.add(model, cx));
    draw(cx);
    show(workbench, then, cx);
}

/// Two tabs of one buffer with changes, both closing: each is the other's
/// copy before either closes, but the last one left stays, by the command
/// and by `:tabo`.
#[gpui::test]
fn shared_dirty_copies_keep_the_last(cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    show(&workbench, "b.txt", cx);
    change(&workbench, cx);
    copy_active(&workbench, "c.txt", cx);
    assert_eq!(
        tabs(&workbench, cx),
        [pane_tabs(
            &["a.txt", "b.txt", "c.txt", "d.txt", "b.txt"],
            "c.txt"
        )]
    );
    run("workbench.close_other_tabs", cx);
    assert_eq!(
        tabs(&workbench, cx),
        [pane_tabs(&["c.txt", "b.txt"], "c.txt")]
    );
    show(&workbench, "b.txt", cx);
    assert!(editor(&workbench, cx).read_with(cx, |editor, _| editor.model().is_dirty()));

    // :tabo likewise, with E445.
    copy_active(&workbench, "c.txt", cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    assert_eq!(
        ex("tabo", &workbench, cx).as_deref(),
        Some("E445: Other tab page contains changes")
    );
    assert_eq!(
        tabs(&workbench, cx),
        [pane_tabs(&["c.txt", "b.txt"], "c.txt")]
    );
    show(&workbench, "b.txt", cx);
    assert!(editor(&workbench, cx).read_with(cx, |editor, _| editor.model().is_dirty()));
}
