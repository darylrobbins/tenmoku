//! A [`DiffView`]'s previous and next change buttons and its `Change 2 of
//! 5` count: clicking one does what `shift-f7` / `f7` do, both are off with
//! no hunks, and the count follows moves, edits and inline mode.

use fe_buffer::Buffer;
use gpui::{Modifiers, TestAppContext, VisualTestContext};

use super::*;
use crate::init;

/// Rows `a0`..`a8`, every third one different on the right: three hunks.
fn texts() -> (String, String) {
    let left: Vec<String> = (0..9).map(|row| format!("a{row}")).collect();
    let right: Vec<String> = (0..9)
        .map(|row| match row % 3 == 1 {
            true => format!("b{row}"),
            false => format!("a{row}"),
        })
        .collect();
    (left.join("\n"), right.join("\n"))
}

fn diff_view<'a>(
    left: &str,
    right: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<DiffView>, &'a mut VisualTestContext) {
    cx.update(init);
    let left = EditorModel::new(Buffer::new(left), None);
    let right = EditorModel::new(Buffer::new(right), Some("a.rs".into()));
    let (diff, cx) = cx.add_window_view(move |_, cx| {
        let left = DiffSide::new("Index", left).read_only();
        DiffView::new("a.rs", left, DiffSide::new("Working Tree", right), cx)
    });
    cx.update(|window, cx| window.focus(&diff.focus_handle(cx), cx));
    draw(cx);
    bind_navigation(cx);
    (diff, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// The default keymap with `f7` and `shift-f7` bound to the diff view's
/// commands in its context.
fn bind_navigation(cx: &mut VisualTestContext) {
    let user = format!(
        r#"
        [[bindings]]
        keys = "f7"
        command = "{NEXT_CHANGE}"
        context = "{KEY_CONTEXT}"

        [[bindings]]
        keys = "shift-f7"
        command = "{PREVIOUS_CHANGE}"
        context = "{KEY_CONTEXT}"
        "#
    );
    let path = std::path::Path::new(fe_keymap::DEFAULT_KEYMAP_PATH);
    let (default, _) = fe_keymap::KeymapLayer::parse(path, fe_keymap::DEFAULT_MACOS, None);
    let user_path = std::path::Path::new("keymap.toml");
    let (user, diagnostics) = fe_keymap::KeymapLayer::parse(user_path, &user, None);
    assert_eq!(diagnostics, []);
    let keymap = fe_keymap::Keymap::from_layers([default.unwrap(), user.unwrap()]);
    cx.update(|_, cx| crate::set_user_keymap(cx, keymap));
}

fn click(id: &'static str, cx: &mut VisualTestContext) {
    let selector = match id {
        "next-change" => "icon-button:next-change",
        _ => "icon-button:previous-change",
    };
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("no {id} button"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
    draw(cx);
}

fn key(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
    draw(cx);
}

/// The right side's caret row.
fn caret(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> u32 {
    diff.read_with(cx, |diff, cx| caret_row(diff.views[1].read(cx).model()))
}

fn label(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|window, cx| {
        let (current, count) = diff.read(cx).current_change(window, cx);
        (count > 0).then(|| match current {
            Some(index) => format!("Change {} of {count}", index + 1),
            None => format!("{count} changes"),
        })
    })
}

#[gpui::test]
fn buttons_move(cx: &mut TestAppContext) {
    let (left, right) = texts();
    let (diff, cx) = diff_view(&left, &right, cx);
    click("next-change", cx);
    let first = caret(&diff, cx);
    assert_eq!(first, 1);
    click("next-change", cx);
    assert_eq!(caret(&diff, cx), 4);
    click("previous-change", cx);
    assert_eq!(caret(&diff, cx), first);

    // The keys land in the same places.
    let (diff, cx) = diff_view(&left, &right, cx);
    key("f7", cx);
    assert_eq!(caret(&diff, cx), 1);
    key("f7", cx);
    assert_eq!(caret(&diff, cx), 4);
    key("shift-f7", cx);
    assert_eq!(caret(&diff, cx), 1);
}

#[gpui::test]
fn count(cx: &mut TestAppContext) {
    let (left, right) = texts();
    let (diff, cx) = diff_view(&left, &right, cx);
    click("next-change", cx);
    assert!(cx.debug_bounds("change-count").is_some());
    assert_eq!(label(&diff, cx).as_deref(), Some("Change 1 of 3"));
    click("next-change", cx);
    assert_eq!(label(&diff, cx).as_deref(), Some("Change 2 of 3"));

    // Inline mode counts the same hunks.
    cx.update(|window, cx| diff.update(cx, |diff, cx| diff.set_inline(true, window, cx)));
    draw(cx);
    assert_eq!(label(&diff, cx).as_deref(), Some("Change 2 of 3"));
    click("previous-change", cx);
    assert_eq!(label(&diff, cx).as_deref(), Some("Change 1 of 3"));
    cx.update(|window, cx| diff.update(cx, |diff, cx| diff.set_inline(false, window, cx)));
    draw(cx);

    // Making rows 2 and 3 differ joins the first two hunks.
    let right_view = diff.read_with(cx, |diff, _| diff.views[1].clone());
    right_view.update(cx, |view, _| {
        let model = view.model_mut();
        let at = model.text().row_start(3);
        model.set_cursor(at);
        model.insert("x");
        let at = model.text().row_start(2);
        model.set_cursor(at);
        model.insert("x");
    });
    draw(cx);
    draw(cx);
    let text = right_view.read_with(cx, |view, _| view.model().text().to_string());
    assert!(text.contains("xa2\nxa3"), "{text}");
    assert_eq!(label(&diff, cx).as_deref(), Some("Change 1 of 2"));
}

#[gpui::test]
fn edges(cx: &mut TestAppContext) {
    let (diff, cx) = diff_view("same\nrows", "same\nrows", cx);
    assert_eq!(label(&diff, cx), None);
    assert!(cx.debug_bounds("change-count").is_none());
    click("next-change", cx);
    click("previous-change", cx);
    // A disabled button does nothing: no "no changes" message either.
    assert!(diff.read_with(cx, |diff, _| diff.message.is_none()));
    assert_eq!(caret(&diff, cx), 0);

    // At the last hunk next does what f7 does.
    let (left, right) = texts();
    let (diff, cx) = diff_view(&left, &right, cx);
    for _ in 0..3 {
        click("next-change", cx);
    }
    assert_eq!(caret(&diff, cx), 7);
    click("next-change", cx);
    let by_click = (
        caret(&diff, cx),
        diff.read_with(cx, |d, _| d.message.clone()),
    );
    let (diff, cx) = diff_view(&left, &right, cx);
    for _ in 0..4 {
        key("f7", cx);
    }
    let by_key = (
        caret(&diff, cx),
        diff.read_with(cx, |d, _| d.message.clone()),
    );
    assert_eq!(by_click, by_key);
    assert_eq!(by_click.0, 1);
}

/// Word diffing is costly past [`WORDS_ASYNC_BYTES`], well under
/// [`ASYNC_BYTES`]: such a diff lands off the main thread, after the first
/// frame.
#[gpui::test]
fn costly_word_diffs_are_not_made_on_the_main_thread(cx: &mut TestAppContext) {
    let rows = |tag: &str| -> String {
        (0..4_000)
            .map(|row| format!("row {row} {tag} some filler text to widen it"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let (left, right) = (rows("old"), rows("new"));
    assert!(left.len() > WORDS_ASYNC_BYTES && left.len() < ASYNC_BYTES);
    cx.update(init);
    let left = EditorModel::new(Buffer::new(&left), None);
    let right = EditorModel::new(Buffer::new(&right), Some("a.rs".into()));
    let (diff, cx) = cx.add_window_view(move |_, cx| {
        let left = DiffSide::new("Index", left).read_only();
        DiffView::new("a.rs", left, DiffSide::new("Working Tree", right), cx)
    });
    cx.run_until_parked();
    let right = diff.read_with(cx, |diff, _| diff.views[1].clone());
    right.update(cx, |view, cx| {
        let model = view.model_mut();
        model.set_cursor(0);
        model.insert("more\n");
        cx.notify();
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(diff.read_with(cx, |diff, _| diff.diffing.is_some()));
    cx.run_until_parked();
    assert!(diff.read_with(cx, |diff, _| diff.diffing.is_none()));
}
