//! The language each side of a diff is highlighted in.

use super::*;

fn language(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    view.read_with(cx, |view, _| view.syntax().map(|s| s.language().name()))
}

/// A diff named `name` of two path-less sides.
fn pathless<'a>(
    name: &'static str,
    cx: &'a mut TestAppContext,
) -> (Entity<DiffView>, &'a mut VisualTestContext) {
    cx.update(init);
    let (diff, cx) = cx.add_window_view(move |_, cx| {
        let left = EditorModel::new(Buffer::new(""), None);
        let right = EditorModel::new(Buffer::new(""), None);
        DiffView::new(
            name,
            DiffSide::new("HEAD", left).read_only(),
            DiffSide::new("Index", right).read_only(),
            cx,
        )
    });
    draw(cx);
    (diff, cx)
}

#[gpui::test]
fn the_left_side_takes_the_files_language(app: &mut TestAppContext) {
    let right = EditorModel::new(Buffer::new("fn b() {}\n"), Some("a.rs".into()));
    let (diff, cx) = diff_view_of("fn a() {}\n", right, app);
    cx.run_until_parked();
    for ix in 0..2 {
        let view = side(&diff, ix, cx);
        assert_eq!(language(&view, cx), Some("rust"), "side {ix}");
    }
}

#[gpui::test]
fn both_sides_of_a_commit_diff_take_the_files_language(app: &mut TestAppContext) {
    for (name, expected) in [("a.rs", "rust"), ("regatta.toml", "toml")] {
        let (diff, cx) = pathless(name, app);
        cx.run_until_parked();
        for ix in 0..2 {
            let view = side(&diff, ix, cx);
            assert_eq!(language(&view, cx), Some(expected), "{name} side {ix}");
        }
    }
}

#[gpui::test]
fn a_left_side_whose_text_lands_later_keeps_the_language(app: &mut TestAppContext) {
    let (diff, cx) = pathless("a.rs", app);
    cx.run_until_parked();
    let left = side(&diff, 0, cx);
    left.update(cx, |view, cx| {
        let len = view.model().text().len();
        let mut buffer = view.model_mut().buffer_mut();
        buffer.edit([(0..len, "fn a() {}\n")], fe_buffer::Origin::User);
        drop(buffer);
        cx.notify();
    });
    draw(cx);
    cx.run_until_parked();
    let (name, parsed, current) = left.read_with(cx, |view, _| {
        let syntax = view.syntax().expect("highlighted");
        (
            syntax.language().name(),
            syntax.version(),
            view.model().snapshot().version(),
        )
    });
    assert_eq!(name, "rust");
    assert_eq!(parsed, current);
}

#[gpui::test]
fn a_side_with_a_path_keeps_its_own_language(app: &mut TestAppContext) {
    let right = EditorModel::new(Buffer::new("x = 1\n"), Some("b.py".into()));
    let (diff, cx) = diff_view_of("fn a() {}\n", right, app);
    cx.run_until_parked();
    let view = side(&diff, 1, cx);
    assert_eq!(language(&view, cx), Some("python"));
}
