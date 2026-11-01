//! Ghost text from the command palette: opening it blurs the editor, which
//! drops the suggestion, but a pick of Accept Suggestion still accepts it.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{Buffer, Origin};
use fe_editor::ghost_text::GhostText;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, GhostTextEvent};
use fe_workbench::{Launch, Workbench};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

const TEXT: &str = "alpha\n";
const SUGGESTION: &str = "beta ";

fn opened(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    // Outside the crate's own tests nothing points the workbench at a
    // config directory of its own: with none, it neither reads the user's
    // settings nor watches them on a thread the test scheduler would
    // reject. Before gpui starts any thread, the only test in this binary.
    for var in ["TENMOKU_CONFIG_DIR", "XDG_CONFIG_HOME", "HOME"] {
        // SAFETY: no other thread is running yet to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        // No root, so no project watcher on a thread of its own.
        root: None,
        file: Some((path, Some(Buffer::new(TEXT).into()))),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    // Blur listeners hear only from an active window.
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(cx);
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("the file opens in an editor");
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    (dir, workbench, editor, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Suggests [`SUGGESTION`] at the editor's caret and draws it.
fn suggest(editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    editor.update(cx, |view, cx| {
        let model = view.model();
        let at = model.primary_selection().head();
        let origin = Origin::Ai {
            agent: "agent".into(),
            session: "session".into(),
        };
        let ghost = GhostText::new(model.snapshot(), at, SUGGESTION, origin);
        view.set_ghost_text(Some(ghost), cx);
    });
    draw(cx);
    assert!(shown(editor, cx));
}

fn shown(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |view, _| view.ghost_text().is_some())
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |view, _| view.model().text().to_string())
}

/// What the editor tells its subscribers of its suggestion: `Accepted`
/// with its `whole` alone.
#[derive(Debug, PartialEq)]
enum Seen {
    Accepted { whole: bool },
    Dismissed,
}

#[gpui::test]
fn palette_accepts_the_suggestion(cx: &mut TestAppContext) {
    let (_dir, _workbench, editor, cx) = opened(cx);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let _subscription = cx.update(|_, cx| {
        let seen = seen.clone();
        cx.subscribe(&editor, move |_, event: &GhostTextEvent, _| {
            seen.borrow_mut().push(match event {
                GhostTextEvent::Accepted { whole, .. } => Seen::Accepted { whole: *whole },
                GhostTextEvent::Dismissed => Seen::Dismissed,
            });
        })
    });

    // Escaping out of the palette leaves the suggestion dropped.
    suggest(&editor, cx);
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    assert!(!shown(&editor, cx), "the palette's focus drops it");
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    assert_eq!(text(&editor, cx), TEXT);
    assert!(!shown(&editor, cx));
    assert_eq!(*seen.borrow(), [Seen::Dismissed]);
    seen.borrow_mut().clear();

    // Picking Accept Suggestion accepts the one the palette's focus
    // dropped.
    suggest(&editor, cx);
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    cx.simulate_input("accept suggestion");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(text(&editor, cx), format!("{SUGGESTION}{TEXT}"));
    assert!(!shown(&editor, cx));
    assert_eq!(
        *seen.borrow(),
        [Seen::Dismissed, Seen::Accepted { whole: true }]
    );
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
}
