//! Closing an editor with unsaved changes asks whether to discard them:
//! its tab, its pane, its window and quitting. Cancel keeps it; Discard
//! closes it. An editor whose buffer another pane shows closes without
//! asking.

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use gpui::{AnyWindowHandle, Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench};

/// A workbench showing `a.txt`, focused, in a temporary folder.
fn opened(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "alpha\n").unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("alpha\n")))),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.run_until_parked();
    (dir, workbench, cx)
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    cx.run_until_parked();
}

fn tabs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<usize> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group().panes().iter())
            .map(|pane| pane.read(cx).tabs().len())
            .collect()
    })
}

fn prompt(cx: &mut VisualTestContext) -> Option<String> {
    cx.pending_prompt().map(|(question, _)| question)
}

fn answer(button: &str, cx: &mut VisualTestContext) {
    cx.simulate_prompt_answer(button);
    cx.run_until_parked();
}

fn window_open(window: AnyWindowHandle, cx: &mut VisualTestContext) -> bool {
    cx.cx.update(|cx| cx.windows().contains(&window))
}

#[gpui::test]
fn a_dirty_tab_asks_before_it_closes(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    cx.simulate_input("x");
    cx.run_until_parked();

    run("workbench.close_tab", cx);
    let question = Some("Discard unsaved changes to a.txt?".to_owned());
    assert_eq!(prompt(cx), question);
    answer("Cancel", cx);
    assert_eq!(tabs(&workbench, cx), [1]);

    run("workbench.close_tab", cx);
    assert_eq!(prompt(cx), question);
    answer("Discard", cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(tabs(&workbench, cx), [0]);
}

#[gpui::test]
fn a_tab_another_pane_shows_closes_without_asking(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    cx.simulate_input("x");
    run("pane.split_right", cx);
    assert_eq!(tabs(&workbench, cx), [1, 1]);
    run("workbench.close_tab", cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(tabs(&workbench, cx), [1]);

    // The last view of it asks, as its pane closes too.
    run("pane.split_right", cx);
    run("pane.close", cx);
    assert_eq!(prompt(cx), None, "the other pane shows it");
    assert_eq!(tabs(&workbench, cx), [1]);
}

#[gpui::test]
fn a_window_and_a_quit_ask(cx: &mut TestAppContext) {
    let (_dir, _workbench, cx) = opened(cx);
    let window = cx.update(|window, _| window.window_handle());
    cx.simulate_input("x");
    let quits = std::rc::Rc::new(std::cell::Cell::new(0));
    let counted = quits.clone();
    cx.update(|_, cx| kiln_shell::close::set_quit(cx, move |_| counted.set(counted.get() + 1)));

    cx.update(|window, _| window.activate_window());
    run("app.quit", cx);
    assert_eq!(prompt(cx), Some("Discard unsaved changes to a.txt?".into()));
    answer("Cancel", cx);
    assert_eq!(quits.get(), 0);
    run("app.quit", cx);
    answer("Discard", cx);
    assert_eq!(quits.get(), 1);

    // The close button.
    assert!(!cx.simulate_close());
    cx.run_until_parked();
    assert_eq!(prompt(cx), Some("Discard unsaved changes to a.txt?".into()));
    answer("Cancel", cx);
    assert!(window_open(window, cx));

    // `window.close`.
    run("window.close", cx);
    answer("Discard", cx);
    assert!(!window_open(window, cx));
}
