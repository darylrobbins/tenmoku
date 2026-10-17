//! The tab commands, typed commands on the workbench since they moved from
//! its `fe-command` registry: each runs on the active pane from its default
//! keys, from a `RunCommand` as the existing tests dispatch one, and from
//! the command palette; and `command_catalog()` knows each with no app.

use std::{fs, path::Path};

use fe_command::{CommandId, ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::{
    Picker,
    command_palette::{COMMAND_PALETTE, CommandPalette},
};

use crate::{Workbench, tests::open_root};

const COMMANDS: [&str; 3] = [
    "workbench.close_tab",
    "workbench.next_tab",
    "workbench.previous_tab",
];

/// A workbench with `a.txt`, `b.txt` and `c.txt` open in its pane, `c.txt`
/// active and focused.
fn opened(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(dir.path().join(name), name).unwrap();
    }
    let (workbench, cx) = open_root(cx, dir.path());
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    for name in ["a.txt", "b.txt", "c.txt"] {
        let path = dir.path().join(name);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    assert_eq!(tabs(&workbench, cx), (vec!["a.txt", "b.txt", "c.txt"], 2));
    (dir, workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// The active pane's tabs' file names, and its active tab's index.
fn tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (Vec<&'static str>, usize) {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let names = (pane.tabs().iter())
            .map(|tab| name(tab.path.as_deref().unwrap()))
            .collect();
        (names, pane.active().unwrap())
    })
}

fn name(path: &Path) -> &'static str {
    match path.file_name().unwrap().to_str().unwrap() {
        "a.txt" => "a.txt",
        "b.txt" => "b.txt",
        "c.txt" => "c.txt",
        other => panic!("{other}"),
    }
}

/// Runs each command in turn through `run`, checking what it did to the
/// tabs: next wraps to the first, previous back to the last, and close
/// leaves the other two.
fn runs_each(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
    mut run: impl FnMut(&str, &mut VisualTestContext),
) {
    run("workbench.next_tab", cx);
    assert_eq!(tabs(workbench, cx), (vec!["a.txt", "b.txt", "c.txt"], 0));
    run("workbench.previous_tab", cx);
    assert_eq!(tabs(workbench, cx), (vec!["a.txt", "b.txt", "c.txt"], 2));
    run("workbench.previous_tab", cx);
    assert_eq!(tabs(workbench, cx), (vec!["a.txt", "b.txt", "c.txt"], 1));
    run("workbench.close_tab", cx);
    let (names, _) = tabs(workbench, cx);
    assert_eq!(names, ["a.txt", "c.txt"]);
}

#[gpui::test]
fn the_tab_commands_run_from_their_default_keys(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    runs_each(&workbench, cx, |command, cx| {
        let key = match command {
            "workbench.close_tab" => "cmd-w",
            "workbench.next_tab" => "cmd-shift-]",
            "workbench.previous_tab" => "cmd-shift-[",
            other => panic!("{other}"),
        };
        cx.simulate_keystrokes(key);
        draw(cx);
    });
}

#[gpui::test]
fn the_tab_commands_run_from_a_run_command(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    runs_each(&workbench, cx, |command, cx| {
        let run = RunCommand(
            Invocation::of(command)
                .unwrap()
                .context(ContextStack::new().with(ContextFrame::new("workbench"))),
        );
        cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
        draw(cx);
    });
}

#[gpui::test]
fn the_tab_commands_run_from_the_palette(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    runs_each(&workbench, cx, |command, cx| {
        let title = match command {
            "workbench.close_tab" => "Close Tab",
            "workbench.next_tab" => "Next Tab",
            "workbench.previous_tab" => "Previous Tab",
            other => panic!("{other}"),
        };
        cx.simulate_keystrokes("cmd-shift-p");
        draw(cx);
        let picker = palette(&workbench, cx).expect("the palette shows");
        cx.simulate_input(&title.to_lowercase());
        draw(cx);
        let first = picker.read_with(cx, |picker, _| picker.delegate.matches()[0].title.clone());
        assert_eq!(first.as_ref(), title);
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(palette(&workbench, cx).is_none());
    });
}

fn palette(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<Picker<CommandPalette>>> {
    workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        (layer.modal() == Some(COMMAND_PALETTE))
            .then(|| layer.modal_view().unwrap().clone().downcast().unwrap())
    })
}

#[test]
fn the_catalog_knows_the_tab_commands_with_no_app() {
    let catalog = crate::command_catalog();
    for command in COMMANDS {
        let id = CommandId::parse(command).unwrap();
        assert_eq!(catalog.check(&id, &Value::Null), Ok(()), "{command}");
    }
}
