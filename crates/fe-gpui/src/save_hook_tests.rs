//! A [`SaveHook`] runs before the write, and the save doesn't wait for one
//! past a second.

use std::{fs, rc::Rc, time::Duration};

use gpui::{App, Entity, Task, TestAppContext, VisualTestContext};

use super::SaveHook;
use crate::{EditorTheme, EditorView};

/// Inserts `text` at the caret when its turn comes, then finishes, or never
/// finishes if `stuck`.
struct Inserts {
    text: &'static str,
    stuck: bool,
}

impl SaveHook for Inserts {
    fn before_save(&self, editor: &Entity<EditorView>, cx: &mut App) -> Task<()> {
        let (editor, text, stuck) = (editor.clone(), self.text, self.stuck);
        cx.spawn(async move |cx| {
            editor.update(cx, |view, cx| {
                view.model_mut().insert(text);
                cx.notify();
            });
            if stuck {
                futures::future::pending::<()>().await;
            }
        })
    }
}

fn opened<'a>(
    cx: &'a mut TestAppContext,
    path: &std::path::Path,
    hook: Inserts,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(crate::init);
    let path = path.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = fe_editor::EditorModel::open(&path).unwrap();
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    view.update(cx, |view, _| view.set_save_hook(Some(Rc::new(hook))));
    (view, cx)
}

#[gpui::test]
fn hook_runs_before_the_write(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\n").unwrap();

    // The text the hook edits is the text written.
    let (view, cx) = opened(
        cx,
        &path,
        Inserts {
            text: "hooked ",
            stuck: false,
        },
    );
    let saved = cx.update(|window, cx| view.update(cx, |view, cx| view.save(window, cx)));
    assert!(view.update(cx, |view, _| view.is_saving()));
    cx.run_until_parked();
    drop(saved);
    assert_eq!(fs::read_to_string(&path).unwrap(), "hooked one\n");
    assert!(!view.update(cx, |view, _| view.model().is_dirty()));

    // One that never finishes delays the save by a second, then it's written.
    view.update(cx, |view, cx| {
        view.set_save_hook(Some(Rc::new(Inserts {
            text: "stuck ",
            stuck: true,
        })));
        cx.notify();
    });
    let saved = cx.update(|window, cx| view.update(cx, |view, cx| view.save(window, cx)));
    cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "hooked one\n",
        "waiting"
    );
    cx.executor().advance_clock(Duration::from_millis(999));
    cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "hooked one\n",
        "still waiting"
    );
    cx.executor().advance_clock(Duration::from_millis(2));
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&path).unwrap(), "hooked stuck one\n");
    drop(saved);

    // A save that waits for another runs the hook too, over what was typed
    // meanwhile.
    view.update(cx, |view, _| {
        view.set_save_hook(Some(Rc::new(Inserts {
            text: "again ",
            stuck: false,
        })));
    });
    let first = cx.update(|window, cx| view.update(cx, |view, cx| view.save(window, cx)));
    let second = cx.update(|window, cx| view.update(cx, |view, cx| view.save(window, cx)));
    cx.run_until_parked();
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("again ").count(), 2, "{text:?}");
    assert!(!view.update(cx, |view, _| view.model().is_dirty()));
    drop((first, second));
}
