//! Why a save happened ([`SaveReason`]) decides whether the save hook and
//! the save fixes edit the text, and whether a failure asks.

use std::{fs, path::Path, rc::Rc};

use fe_editor::{EditorModel, SaveError, auto_save::SaveReason};
use gpui::{App, Entity, Focusable as _, Task, TestAppContext, VisualTestContext};

use crate::{EditorTheme, EditorView, SaveHook, set_vim_enabled};

/// Inserts `hooked ` at the caret, as format on save would edit.
struct Edits;

impl SaveHook for Edits {
    fn before_save(&self, editor: &Entity<EditorView>, cx: &mut App) -> Task<()> {
        editor.update(cx, |view, cx| {
            view.model_mut().insert("hooked ");
            cx.notify();
        });
        Task::ready(())
    }
}

fn opened<'a>(
    cx: &'a mut TestAppContext,
    path: &Path,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(crate::init);
    let path = path.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::open(&path).unwrap();
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    (view, cx)
}

fn save_with(
    view: &Entity<EditorView>,
    reason: SaveReason,
    cx: &mut VisualTestContext,
) -> Task<gpui::Result<()>> {
    cx.update(|window, cx| view.update(cx, |view, cx| view.save_with(reason, window, cx)))
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

#[gpui::test]
fn delay_skips_edits(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\n").unwrap();
    let (view, cx) = opened(cx, &path);
    view.update(cx, |view, _| view.set_save_hook(Some(Rc::new(Edits))));

    // After a delay: the text as it is, the hook not run.
    view.update(cx, |view, _| view.model_mut().insert("x"));
    let saved = save_with(&view, SaveReason::AutoDelay, cx);
    cx.run_until_parked();
    drop(saved);
    assert_eq!(fs::read_to_string(&path).unwrap(), "xone\n");
    assert_eq!(text(&view, cx), "xone\n");

    // Explicit, and on focus out of insert: the hook runs.
    let saved = save_with(&view, SaveReason::Explicit, cx);
    cx.run_until_parked();
    drop(saved);
    assert_eq!(fs::read_to_string(&path).unwrap(), "xhooked one\n");
    let saved = save_with(&view, SaveReason::AutoFocus, cx);
    cx.run_until_parked();
    drop(saved);
    assert_eq!(fs::read_to_string(&path).unwrap(), "xhooked hooked one\n");

    // On focus out during a vim insert: it doesn't.
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    keys("i", cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("insert")
    );
    let saved = save_with(&view, SaveReason::AutoFocus, cx);
    cx.run_until_parked();
    drop(saved);
    assert_eq!(fs::read_to_string(&path).unwrap(), "xhooked hooked one\n");
}

#[gpui::test]
fn insert_session_kept(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "").unwrap();
    let (view, cx) = opened(cx, &path);
    view.update(cx, |view, _| view.set_save_hook(Some(Rc::new(Edits))));
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();

    keys("i a b c", cx);
    let saved = save_with(&view, SaveReason::AutoFocus, cx);
    cx.run_until_parked();
    drop(saved);
    assert_eq!(fs::read_to_string(&path).unwrap(), "abc");

    // The whole insert repeats, and undoes as one step.
    keys("escape .", cx);
    assert_eq!(text(&view, cx).len(), 6, "{:?}", text(&view, cx));
    keys("u", cx);
    assert_eq!(text(&view, cx), "abc");
}

#[gpui::test]
fn auto_never_prompts(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\n").unwrap();
    let (view, cx) = opened(cx, &path);

    view.update(cx, |view, _| view.model_mut().insert("mine "));
    fs::write(&path, "theirs\n").unwrap();
    let saved = save_with(&view, SaveReason::AutoDelay, cx);
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    let error = cx
        .update(|_, cx| cx.foreground_executor().block_test(saved))
        .unwrap_err();
    assert!(matches!(
        error
            .downcast_ref::<std::io::Error>()
            .and_then(|error| error.get_ref())
            .and_then(|inner| inner.downcast_ref::<SaveError>()),
        Some(SaveError::ChangedOnDisk)
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), "theirs\n");

    // Explicit still asks.
    let saved = save_with(&view, SaveReason::Explicit, cx);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    drop(saved);
}

/// Holds its save until `release` is dropped, then edits.
struct Later(Rc<std::cell::Cell<bool>>);

impl SaveHook for Later {
    fn before_save(&self, editor: &Entity<EditorView>, cx: &mut App) -> Task<()> {
        let (editor, go) = (editor.clone(), self.0.clone());
        cx.spawn(async move |cx| {
            while !go.get() {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(10))
                    .await;
            }
            editor.update(cx, |view, cx| {
                if view.save_edits_allowed(cx) {
                    view.model_mut().insert("hooked ");
                }
            });
        })
    }
}

#[gpui::test]
fn insert_entered_while_waiting(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one  \n").unwrap();
    let (view, cx) = opened(cx, &path);
    let go = Rc::new(std::cell::Cell::new(false));
    view.update(cx, |view, _| {
        view.set_save_hook(Some(Rc::new(Later(go.clone()))));
        view.set_save_fixes(crate::SaveFixes {
            trim_trailing_whitespace: true,
            insert_final_newline: true,
        });
    });
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();

    // Asked in normal mode; vim enters insert while the hook waits.
    let saved = save_with(&view, SaveReason::AutoFocus, cx);
    cx.run_until_parked();
    keys("i", cx);
    go.set(true);
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(20));
    cx.run_until_parked();
    drop(saved);
    assert_eq!(fs::read_to_string(&path).unwrap(), "one  \n");
}

#[test]
fn explicit_raw_edits_nothing_but_asks() {
    for vim_insert in [false, true] {
        assert!(!SaveReason::ExplicitRaw.edits_allowed(vim_insert));
    }
    assert!(SaveReason::ExplicitRaw.asks() && SaveReason::Explicit.asks());
    assert!(!SaveReason::AutoDelay.asks() && !SaveReason::AutoFocus.asks());
}

#[gpui::test]
fn explicit_raw_skips_the_hook_and_prompts(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\n").unwrap();
    let (view, cx) = opened(cx, &path);
    view.update(cx, |view, _| view.set_save_hook(Some(Rc::new(Edits))));
    view.update(cx, |view, _| view.model_mut().insert("x"));
    let saved = save_with(&view, SaveReason::ExplicitRaw, cx);
    cx.run_until_parked();
    drop(saved);
    assert_eq!(fs::read_to_string(&path).unwrap(), "xone\n");

    view.update(cx, |view, _| view.model_mut().insert("mine "));
    fs::write(&path, "theirs\n").unwrap();
    let saved = save_with(&view, SaveReason::ExplicitRaw, cx);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    drop(saved);
}
