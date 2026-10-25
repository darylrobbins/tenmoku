//! A middle click on a file tab closes it like the ×, so it goes on the
//! closed-editor stack and Reopen Closed Editor brings it back with its
//! selections and scroll.

use std::path::PathBuf;

use fe_buffer::ResolvedSelection;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use gpui::{Focusable as _, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext};

use crate::{Launch, Workbench};

#[gpui::test]
fn pushes_closed_editor_stack(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let text: String = (0..300).map(|row| format!("row {row}\n")).collect();
    for name in ["a.txt", "b.txt"] {
        std::fs::write(dir.path().join(name), &text).unwrap();
    }
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let draw = |cx: &mut gpui::VisualTestContext| {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    };
    draw(cx);

    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let (a, b): (PathBuf, PathBuf) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    let mut before = None;
    for path in [&a, &b] {
        let opened = path.clone();
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(opened, window, cx)));
        draw(cx);
        if path == &a {
            let editor = pane.read_with(cx, |pane, _| pane.active_editor().cloned().unwrap());
            editor.update(cx, |editor, cx| {
                editor.model_mut().set_selections([ResolvedSelection {
                    range: 30..34,
                    reversed: true,
                }]);
                editor.set_scroll_top(40., cx);
            });
            draw(cx);
            before = Some(editor.read_with(cx, |editor, _| {
                (editor.model().selections(), editor.scroll_top())
            }));
        }
    }
    let before = before.unwrap();

    // Middle-click a's tab, which is not the active one.
    let at = cx
        .debug_bounds("tab-icon:a.txt:file-text")
        .expect("a's tab is drawn")
        .center();
    cx.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Middle,
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Middle,
        click_count: 1,
    });
    draw(cx);
    let open: Vec<PathBuf> = pane.read_with(cx, |pane, _| {
        pane.tabs()
            .iter()
            .filter_map(|tab| tab.path.clone())
            .collect()
    });
    assert_eq!(open, std::slice::from_ref(&b));
    let stack = pane.read_with(cx, |pane, _| pane.closed_editors().borrow().paths());
    assert_eq!(stack, std::slice::from_ref(&a));

    let run = RunCommand(
        Invocation::of("workbench.reopen_closed_editor")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let reopened = pane.read_with(cx, |pane, _| pane.active_editor().cloned().unwrap());
    reopened.read_with(cx, |editor, _| {
        assert_eq!(editor.model().path(), Some(a.as_path()));
        assert_eq!((editor.model().selections(), editor.scroll_top()), before);
    });
}
