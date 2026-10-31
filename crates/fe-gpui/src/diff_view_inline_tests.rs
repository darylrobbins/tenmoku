//! A [`DiffView`] inline: removed rows as blocks above their place in the
//! right editor alone, switching modes keeping the place, and starting
//! inline in a narrow window.

use fe_buffer::Buffer;
use fe_command::Invocation;
use gpui::{Bounds, TestAppContext, VisualTestContext, WindowBounds, WindowOptions, point, size};

use super::*;
use crate::init;

fn text(rows: &[&str]) -> String {
    rows.join("\n")
}

/// A focused diff view in a window `width` px wide, drawn.
fn diff_view<'a>(
    left: &str,
    right: &str,
    width: f32,
    cx: &'a mut TestAppContext,
) -> (Entity<DiffView>, &'a mut VisualTestContext) {
    cx.update(init);
    let left = EditorModel::new(Buffer::new(left), None);
    let right = EditorModel::new(Buffer::new(right), Some("a.rs".into()));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            size(px(width), px(600.)),
        ))),
        ..Default::default()
    };
    let window = cx.update(|cx| {
        cx.open_window(options, move |_, cx| {
            cx.new(|cx| {
                let left = DiffSide::new("Index", left).read_only();
                DiffView::new("a.rs", left, DiffSide::new("Working Tree", right), cx)
            })
        })
        .unwrap()
    });
    let diff = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.update(|window, cx| window.focus(&diff.focus_handle(cx), cx));
    draw(cx);
    (diff, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn side(diff: &Entity<DiffView>, ix: usize, cx: &mut VisualTestContext) -> Entity<EditorView> {
    diff.read_with(cx, |diff, _| diff.views[ix].clone())
}

fn blocks(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<HostBlock> {
    view.read_with(cx, |view, _| view.blocks().to_vec())
}

fn toggle(_diff: &Entity<DiffView>, cx: &mut VisualTestContext) {
    // As a binding or the palette dispatches it: by command name.
    let invocation = Invocation::of(TOGGLE_INLINE).unwrap();
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    draw(cx);
}

fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    view.read_with(cx, |view, _| caret_row(view.model()))
}

fn set_caret(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let at = row_offset(view.model().text(), row);
        view.select_and_reveal(at..at, cx);
    });
    draw(cx);
}

fn focused(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| view.focus_handle(cx).is_focused(window))
}

fn rows(n: u32) -> String {
    (0..n)
        .map(|row| format!("row {row}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[gpui::test]
fn removed_rows_above(app: &mut TestAppContext) {
    let (diff, cx) = diff_view(
        &text(&["a", "b", "c", "d"]),
        &text(&["a", "X", "d"]),
        1200.,
        app,
    );
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    assert!(!diff.read_with(cx, |diff, _| diff.is_inline()));
    toggle(&diff, cx);
    assert!(diff.read_with(cx, |diff, _| diff.is_inline()));

    // Only the right editor is shown, with the removed rows above X.
    assert_eq!(
        blocks(&right, cx),
        [HostBlock {
            above: 1,
            rows: 2,
            kind: HostBlockKind::Lines {
                lines: vec!["b".into(), "c".into()],
                tint: Tint::DiffRemoved,
                // b's pair is X; c has none.
                words: vec![vec![0..1], vec![0..1]],
            },
            id: 0,
        }]
    );
    assert_eq!(blocks(&left, cx), []);
    // The block's rows come before X's.
    assert_eq!(
        diff.read_with(cx, |diff, _| inline::screen_row(diff.diff(), 1)),
        3
    );
}

#[gpui::test]
fn toggle_keeps_place(app: &mut TestAppContext) {
    let left = rows(100);
    let mut right: Vec<String> = (0..100).map(|row| format!("row {row}")).collect();
    right[10] = "changed".into();
    right.remove(60);
    right.insert(20, "added".into());
    let right = right.join("\n");
    let (diff, cx) = diff_view(&left, &right, 1200., app);
    let (left_view, right_view) = (side(&diff, 0, cx), side(&diff, 1, cx));

    set_caret(&right_view, 40, cx);
    right_view.update(cx, |view, cx| view.set_scroll_top(35., cx));
    draw(cx);
    let top_row = |cx: &mut VisualTestContext| {
        right_view.read_with(cx, |view, _| view.viewport_rows().unwrap().0)
    };
    let (row, head) = (
        top_row(cx),
        right_view.read_with(cx, |v, _| v.model().primary_selection()),
    );

    toggle(&diff, cx);
    assert_eq!(caret(&right_view, cx), 40);
    assert_eq!(
        right_view.read_with(cx, |v, _| v.model().primary_selection()),
        head
    );
    assert_eq!(top_row(cx), row);
    assert!(focused(&right_view, cx));

    toggle(&diff, cx);
    assert!(!diff.read_with(cx, |diff, _| diff.is_inline()));
    assert_eq!(caret(&right_view, cx), 40);
    assert_eq!(
        right_view.read_with(cx, |v, _| v.model().primary_selection()),
        head
    );
    assert_eq!(top_row(cx), row);
    assert!(!blocks(&left_view, cx).is_empty());

    // With the left side focused, the caret moves to the paired right row.
    cx.update(|window, cx| window.focus(&left_view.focus_handle(cx), cx));
    set_caret(&left_view, 70, cx);
    toggle(&diff, cx);
    // Left row 70 comes after the removed row 60 and the added row 20, and
    // one modified row: right row 70 is left row 70.
    let expected = diff.read_with(cx, |diff, _| inline::paired_right_row(diff.diff(), 70));
    assert_eq!(expected, 70);
    assert_eq!(caret(&right_view, cx), 70);
    assert!(focused(&right_view, cx));
    toggle(&diff, cx);
    assert!(focused(&right_view, cx));
}

#[gpui::test]
fn narrow_start(app: &mut TestAppContext) {
    let (left, right) = (text(&["a", "b", "c", "d"]), text(&["a", "B", "c", "D"]));
    let (diff, cx) = diff_view(&left, &right, 800., app);
    assert!(diff.read_with(cx, |diff, _| diff.is_inline()));
    toggle(&diff, cx);
    assert!(!diff.read_with(cx, |diff, _| diff.is_inline()));
    // The toggle wins from then on.
    draw(cx);
    assert!(!diff.read_with(cx, |diff, _| diff.is_inline()));

    let (wide, cx) = diff_view(&left, &right, 1200., app);
    assert!(!wide.read_with(cx, |diff, _| diff.is_inline()));
    toggle(&wide, cx);

    // Navigation works on the right side inline.
    let right = side(&wide, 1, cx);
    cx.update(|window, cx| wide.update(cx, |diff, cx| diff.next_change(window, cx)));
    draw(cx);
    assert_eq!(caret(&right, cx), 1);
    cx.update(|window, cx| wide.update(cx, |diff, cx| diff.next_change(window, cx)));
    draw(cx);
    assert_eq!(caret(&right, cx), 3);
    cx.update(|window, cx| wide.update(cx, |diff, cx| diff.previous_change(window, cx)));
    draw(cx);
    assert_eq!(caret(&right, cx), 1);
}

/// A buffer's text is LF only, but the diff's rows split on `\r` and `\r\n`
/// too: the block's lines are the diff's rows.
#[test]
fn removed_rows_split_on_any_ending() {
    let (left, right) = ("a\rb\r\nc\nd", "a\rX\nd");
    let diff = TextDiff::new(left, right);
    let blocks = inline::removed_blocks(&diff, left, &Words::default());
    let HostBlockKind::Lines { lines, .. } = &blocks[0].kind else {
        panic!("{:?}", blocks[0].kind)
    };
    assert_eq!(lines, &["b", "c"]);
}

#[path = "diff_view_inline_navigation_tests.rs"]
mod inline_navigation_tests;
