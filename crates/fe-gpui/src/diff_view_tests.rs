//! A [`DiffView`] in a headless window: rows lined up with fillers and
//! tinted by side, removed on the left and added on the right, the left
//! side read-only; an edit to the right side's
//! live buffer re-diffed within a frame; scrolling followed across the
//! sides without echo; and moving between hunks, wrapping at the ends.

use fe_buffer::Buffer;
use fe_command::Invocation;
use fe_editor::row_changes::{RowChange, RowChangeKind};
use gpui::{
    Modifiers, Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, point, px,
};

use super::*;
use crate::{EditorTheme, init};

/// `rows` joined into a text, one a row.
fn text(rows: &[&str]) -> String {
    rows.join("\n")
}

/// A focused diff view of `left`, read-only, against `right`, editable,
/// drawn.
fn diff_view<'a>(
    left: &str,
    right: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<DiffView>, &'a mut VisualTestContext) {
    let right = EditorModel::new(Buffer::new(right), Some("a.rs".into()));
    diff_view_of(left, right, cx)
}

fn diff_view_of<'a>(
    left: &str,
    right: EditorModel,
    cx: &'a mut TestAppContext,
) -> (Entity<DiffView>, &'a mut VisualTestContext) {
    cx.update(init);
    let left = EditorModel::new(Buffer::new(left), None);
    let (diff, cx) = cx.add_window_view(move |_, cx| {
        let left = DiffSide::new("Index", left).read_only();
        DiffView::new("a.rs", left, DiffSide::new("Working Tree", right), cx)
    });
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

fn filler(above: u32, rows: u32, id: u64) -> HostBlock {
    HostBlock {
        above,
        rows,
        kind: HostBlockKind::Filler,
        id,
    }
}

fn blocks(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<HostBlock> {
    view.read_with(cx, |view, _| view.blocks().to_vec())
}

/// The window y the last frame painted buffer row `row` at.
fn row_y(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Pixels {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let frame = &layout.frame;
        let shown = (frame.rows.iter())
            .find(|shown| shown.buffer_row == row)
            .expect("row in view");
        layout.origin.y + px(frame.row_top(shown.row))
    })
}

/// The rows each row background covers, in its tint, as the model hands
/// them to the painter.
fn tints(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(Range<u32>, Tint)> {
    view.read_with(cx, |view, _| {
        let model = view.model();
        let text = model.text();
        (model.decorations(0..text.len()).into_iter())
            .filter(|item| item.kind == DecorationKind::RowBackground)
            .map(|item| {
                let rows = item.covered_rows(text);
                (*rows.start()..rows.end() + 1, item.tint)
            })
            .collect()
    })
}

/// How many row backgrounds the last frame painted.
fn painted_tints(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        (layout.decorations.placed.iter())
            .filter(|placed| placed.kind == DecorationKind::RowBackground)
            .count()
    })
}

#[gpui::test]
fn aligned(app: &mut TestAppContext) {
    let (diff, cx) = diff_view(
        &text(&["a", "b", "c", "d"]),
        &text(&["a", "X", "Y", "Z", "d"]),
        app,
    );
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    assert!(left.read_with(cx, |view, _| view.is_read_only()));
    assert!(!right.read_with(cx, |view, _| view.is_read_only()));
    assert_eq!(
        diff.read_with(cx, |diff, cx| diff.tab_title(cx)),
        "a.rs (Index ↔ Working Tree)"
    );

    // b and c become X, Y and Z: the left's two modified rows get one
    // filler row after them.
    assert_eq!(blocks(&left, cx), [filler(3, 1, 0)]);
    assert_eq!(blocks(&right, cx), []);
    assert_eq!(row_y(&left, 3, cx), row_y(&right, 4, cx));
    assert_eq!(tints(&left, cx), [(1..3, Tint::DiffRemoved)]);
    assert_eq!(tints(&right, cx), [(1..4, Tint::DiffAdded)]);
    assert_eq!(painted_tints(&left, cx), 2);
    assert_eq!(painted_tints(&right, cx), 3);

    // One modified row against three: two filler rows after it.
    let (diff, cx) = diff_view(
        &text(&["a", "b", "d"]),
        &text(&["a", "X", "Y", "Z", "d"]),
        app,
    );
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    assert_eq!(blocks(&left, cx), [filler(2, 2, 0)]);
    assert_eq!(row_y(&left, 2, cx), row_y(&right, 4, cx));
    assert_eq!(tints(&left, cx), [(1..2, Tint::DiffRemoved)]);
    assert_eq!(tints(&right, cx), [(1..4, Tint::DiffAdded)]);

    // Rows only one side has: removed on the left, added on the right,
    // each across from a filler.
    let (diff, cx) = diff_view(
        &text(&["a", "gone", "b", "c", "d"]),
        &text(&["a", "b", "c", "new", "d"]),
        app,
    );
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    assert_eq!(blocks(&left, cx), [filler(4, 1, 0)]);
    assert_eq!(blocks(&right, cx), [filler(1, 1, 0)]);
    assert_eq!(tints(&left, cx), [(1..2, Tint::DiffRemoved)]);
    assert_eq!(tints(&right, cx), [(3..4, Tint::DiffAdded)]);
    for (left_row, right_row) in [(0, 0), (2, 1), (3, 2), (4, 4)] {
        assert_eq!(row_y(&left, left_row, cx), row_y(&right, right_row, cx));
    }
}

#[gpui::test]
fn live_right_side(cx: &mut TestAppContext) {
    let head = text(&["a", "X", "Y", "Z", "d"]);
    // A file's editor, its change bars against `HEAD`, the same text.
    let mut file = EditorModel::new(Buffer::new(&head), Some("a.rs".into()));
    file.set_diff_base(Some(head.as_str().into()));
    let right = file.split();
    let (diff, cx) = diff_view_of(&text(&["a", "b", "c", "d"]), right, cx);
    let other =
        cx.update(|_, cx| cx.new(|cx| EditorView::new(file, EditorTheme::default_dark(cx), cx)));
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    assert_eq!(blocks(&left, cx), [filler(3, 1, 0)]);

    // A new row above X, typed in the right side.
    right.update(cx, |view, _| {
        let at = view.model().text().row_start(1);
        view.model_mut().set_cursor(at);
    });
    cx.simulate_input("N");
    cx.simulate_keystrokes("enter");
    // One frame later, b and c are across from N, X, Y and Z.
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(blocks(&left, cx), [filler(3, 2, 0)]);
    assert_eq!(tints(&right, cx), [(1..5, Tint::DiffAdded)]);
    assert_eq!(painted_tints(&right, cx), 4);
    assert_eq!(row_y(&left, 3, cx), row_y(&right, 5, cx));

    // The file's editor shows the row, its change bars still against
    // `HEAD`.
    let (shown, changes) = other.update(cx, |view, _| {
        let model = view.model_mut();
        model.sync_display();
        (model.text().to_string(), model.row_changes())
    });
    assert_eq!(shown, text(&["a", "N", "X", "Y", "Z", "d"]));
    assert_eq!(
        *changes,
        [RowChange {
            rows: 1..2,
            kind: RowChangeKind::Added
        }]
    );
    // The left side's base is the right side's text.
    let left_changes = left.read_with(cx, |view, _| view.model().row_changes());
    assert!(!left_changes.is_empty());
    // And the tab is dirty, as the file is.
    assert!(diff.read_with(cx, |diff, cx| diff.is_dirty(cx)));
}

/// The window position in the middle of a side's text.
fn middle(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Point<Pixels> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        layout.bounds(layout.frame.text_bounds()).center()
    })
}

fn wheel(at: Point<Pixels>, rows: f32, cx: &mut VisualTestContext) {
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Lines(point(0., -rows)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
}

fn scroll_top(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    view.read_with(cx, |view, _| view.scroll_top())
}

/// `count` rows, `r0` on, with row 5 changed and three rows added before
/// row 20 on the right.
fn long_texts(count: usize) -> (String, String) {
    let left: Vec<String> = (0..count).map(|row| format!("r{row}")).collect();
    let mut right = left.clone();
    right[5] = "changed".into();
    for row in 0..3 {
        right.insert(20, format!("new{row}"));
    }
    (left.join("\n"), right.join("\n"))
}

/// [`long_texts`] with rows of about 200 bytes, so a few rows cross a byte
/// threshold.
fn wide_texts(count: usize) -> (String, String) {
    let pad = "x".repeat(200);
    let left: Vec<String> = (0..count).map(|row| format!("r{row}{pad}")).collect();
    let mut right = left.clone();
    right[5] = "changed".into();
    for row in 0..3 {
        right.insert(20, format!("new{row}"));
    }
    (left.join("\n"), right.join("\n"))
}

#[gpui::test]
fn scroll_synced(cx: &mut TestAppContext) {
    let (left_text, right_text) = long_texts(500);
    let (diff, cx) = diff_view(&left_text, &right_text, cx);
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    let sets = |cx: &mut VisualTestContext| diff.read_with(cx, |diff, _| diff.scroll_sets);

    wheel(middle(&right, cx), 40., cx);
    draw(cx);
    draw(cx);
    assert_eq!(scroll_top(&right, cx), 40.);
    assert_eq!(scroll_top(&left, cx), 40.);
    assert_eq!(sets(cx), [1, 0]);
    // Past the filler, the rows line up: left row 37 is right row 40.
    assert_eq!(row_y(&left, 37, cx), row_y(&right, 40, cx));

    // And back, from the left.
    wheel(middle(&left, cx), -40., cx);
    draw(cx);
    draw(cx);
    assert_eq!(scroll_top(&left, cx), 0.);
    assert_eq!(scroll_top(&right, cx), 0.);
    assert_eq!(sets(cx), [1, 1]);
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

/// Runs `command` by its key ([`bind_navigation`]), and lets the scroll
/// it makes reach the other side.
fn run(command: &str, cx: &mut VisualTestContext) {
    let keys = match command {
        NEXT_CHANGE => "f7",
        PREVIOUS_CHANGE => "shift-f7",
        _ => unreachable!("{command}"),
    };
    cx.simulate_keystrokes(keys);
    draw(cx);
    draw(cx);
}

/// Runs `command` as the action its binding dispatches.
fn dispatch(command: &str, cx: &mut VisualTestContext) {
    let invocation = Invocation::of(command).unwrap();
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    draw(cx);
}

fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    view.read_with(cx, |view, _| caret_row(view.model()))
}

fn message(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> Option<String> {
    diff.read_with(cx, |diff, _| diff.message().map(ToString::to_string))
}

#[gpui::test]
fn navigation(cx: &mut TestAppContext) {
    // Row 10 changed, three rows added before row 50 and row 80 removed
    // on the right.
    let left: Vec<String> = (0..120).map(|row| format!("r{row}")).collect();
    let mut right = left.clone();
    right[10] = "changed".into();
    right.remove(80);
    for row in 0..3 {
        right.insert(50, format!("new{row}"));
    }
    let (diff, cx) = diff_view(&left.join("\n"), &right.join("\n"), cx);
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    let carets = |cx: &mut VisualTestContext| (caret(&left, cx), caret(&right, cx));
    bind_navigation(cx);

    // From the top, the right side focused.
    run(NEXT_CHANGE, cx);
    assert_eq!(carets(cx), (10, 10));
    assert_eq!(message(&diff, cx), None);
    run(NEXT_CHANGE, cx);
    assert_eq!(carets(cx), (50, 50));
    // The removed row's hunk: where it was on the left, the row after
    // where it would be on the right. Far down, so the right scrolled and
    // the left followed.
    run(NEXT_CHANGE, cx);
    assert_eq!(carets(cx), (80, 83));
    let top = scroll_top(&right, cx);
    assert!(top > 0.);
    assert_eq!(scroll_top(&left, cx), top);
    // Past the last, back to the first.
    run(NEXT_CHANGE, cx);
    assert_eq!(carets(cx), (10, 10));
    assert_eq!(message(&diff, cx).as_deref(), Some(WRAPPED_TO_FIRST));
    assert_eq!(scroll_top(&left, cx), scroll_top(&right, cx));

    // Before the first, back to the last.
    run(PREVIOUS_CHANGE, cx);
    assert_eq!(carets(cx), (80, 83));
    assert_eq!(message(&diff, cx).as_deref(), Some(WRAPPED_TO_LAST));
    run(PREVIOUS_CHANGE, cx);
    assert_eq!(carets(cx), (50, 50));
    assert_eq!(message(&diff, cx), None);

    // With the left focused, its rows are the ones counted.
    cx.update(|window, cx| window.focus(&left.focus_handle(cx), cx));
    left.update(cx, |view, _| {
        let at = view.model().text().row_start(70);
        view.model_mut().set_cursor(at);
    });
    run(NEXT_CHANGE, cx);
    assert_eq!(carets(cx), (80, 83));
    run(PREVIOUS_CHANGE, cx);
    assert_eq!(carets(cx), (50, 50));
}

/// Runs `forward`'s method on the view, as a host would.
fn call(diff: &Entity<DiffView>, forward: bool, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        diff.update(cx, |diff, cx| match forward {
            true => diff.next_change(window, cx),
            false => diff.previous_change(window, cx),
        })
    });
    draw(cx);
}

#[gpui::test]
fn navigation_methods(cx: &mut TestAppContext) {
    let (diff, cx) = diff_view(
        &text(&["a", "b", "c", "d", "e"]),
        &text(&["a", "B", "c", "D", "e"]),
        cx,
    );
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    let carets = |cx: &mut VisualTestContext| (caret(&left, cx), caret(&right, cx));
    call(&diff, true, cx);
    assert_eq!(carets(cx), (1, 1));
    call(&diff, true, cx);
    assert_eq!(carets(cx), (3, 3));
    call(&diff, true, cx);
    assert_eq!(carets(cx), (1, 1));
    assert_eq!(message(&diff, cx).as_deref(), Some(WRAPPED_TO_FIRST));
    call(&diff, false, cx);
    assert_eq!(carets(cx), (3, 3));
    assert_eq!(message(&diff, cx).as_deref(), Some(WRAPPED_TO_LAST));
}

#[gpui::test]
fn no_changes(cx: &mut TestAppContext) {
    let same = text(&["a", "b"]);
    let (diff, cx) = diff_view(&same, &same, cx);
    dispatch(NEXT_CHANGE, cx);
    assert_eq!(message(&diff, cx).as_deref(), Some(NO_CHANGES));
    assert_eq!(blocks(&side(&diff, 0, cx), cx), []);
}

#[gpui::test]
fn large_sides_diff_off_the_main_thread(cx: &mut TestAppContext) {
    // Over a mebibyte a side.
    let (left_text, right_text) = wide_texts(6_000);
    assert!(left_text.len() > ASYNC_BYTES);
    let (diff, cx) = diff_view(&left_text, &right_text, cx);
    let left = side(&diff, 0, cx);
    assert_eq!(blocks(&left, cx), [filler(20, 3, 0)]);
    // Row changes from the diff that landed, not diffed by the model on
    // the main thread.
    let changes = left.read_with(cx, |view, _| view.model().row_changes());
    assert_eq!(
        *changes,
        [
            RowChange {
                rows: 5..6,
                kind: RowChangeKind::Modified
            },
            RowChange {
                rows: 20..20,
                kind: RowChangeKind::Deleted
            }
        ]
    );

    // An edit keeps the last diff's fillers until the new one lands.
    let right = side(&diff, 1, cx);
    right.update(cx, |view, cx| {
        let model = view.model_mut();
        model.set_cursor(model.text().row_start(1));
        model.insert("more\n");
        cx.notify();
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(blocks(&left, cx), [filler(20, 3, 0)]);
    cx.run_until_parked();
    assert_eq!(blocks(&left, cx), [filler(1, 1, 0), filler(20, 3, 1)]);
}

#[gpui::test]
fn navigation_at_the_ends(app: &mut TestAppContext) {
    // The right lacks the left's last row: a hunk whose start on the
    // right is past its last row.
    let (diff, cx) = diff_view(&text(&["a", "b", "c", "d"]), &text(&["A", "b", "c"]), app);
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    let carets = |cx: &mut VisualTestContext| (caret(&left, cx), caret(&right, cx));
    right.update(cx, |view, _| view.model_mut().set_cursor(2));
    dispatch(NEXT_CHANGE, cx);
    // The right's caret on its last row, as near the removed row as it
    // gets.
    assert_eq!(carets(cx), (3, 2));
    assert_eq!(message(&diff, cx), None);
    // From there, on past the last hunk, not to it again.
    dispatch(NEXT_CHANGE, cx);
    assert_eq!(carets(cx), (0, 0));
    assert_eq!(message(&diff, cx).as_deref(), Some(WRAPPED_TO_FIRST));
    dispatch(NEXT_CHANGE, cx);
    assert_eq!(carets(cx), (3, 2));
    assert_eq!(message(&diff, cx), None);
    dispatch(PREVIOUS_CHANGE, cx);
    assert_eq!(carets(cx), (0, 0));
    dispatch(PREVIOUS_CHANGE, cx);
    assert_eq!(carets(cx), (3, 2));
    assert_eq!(message(&diff, cx).as_deref(), Some(WRAPPED_TO_LAST));

    // From the left, a row added at the right's end.
    let (diff, cx) = diff_view(&text(&["a", "b"]), &text(&["a", "b", "c"]), app);
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    let carets = |cx: &mut VisualTestContext| (caret(&left, cx), caret(&right, cx));
    cx.update(|window, cx| window.focus(&left.focus_handle(cx), cx));
    dispatch(NEXT_CHANGE, cx);
    assert_eq!(carets(cx), (1, 2));
    dispatch(NEXT_CHANGE, cx);
    assert_eq!(carets(cx), (1, 2));
    assert_eq!(message(&diff, cx).as_deref(), Some(WRAPPED_TO_FIRST));
}

#[gpui::test]
fn trailing_empty_row_tinted(app: &mut TestAppContext) {
    let (diff, cx) = diff_view("a", "a\nb\n", app);
    let right = side(&diff, 1, cx);
    assert_eq!(
        tints(&right, cx),
        [(1..2, Tint::DiffAdded), (2..3, Tint::DiffAdded)]
    );
    assert_eq!(painted_tints(&right, cx), 2);
    let (diff, cx) = diff_view("a\nb\n", "a", app);
    let left = side(&diff, 0, cx);
    assert_eq!(
        tints(&left, cx),
        [(1..2, Tint::DiffRemoved), (2..3, Tint::DiffRemoved)]
    );
    assert_eq!(painted_tints(&left, cx), 2);
}

#[gpui::test]
fn sides_stay_unwrapped_and_unfolded(cx: &mut TestAppContext) {
    let long = "word ".repeat(200);
    let (diff, cx) = diff_view(
        &text(&["a", &long, "b", "c", "d", "e"]),
        &text(&["a", &long, "b", "C", "d", "e"]),
        cx,
    );
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    right.update(cx, |view, cx| {
        view.model_mut().set_soft_wrap(true);
        cx.notify();
    });
    left.update(cx, |view, cx| {
        view.model_mut().fold_rows(2..=3);
        cx.notify();
    });
    draw(cx);
    draw(cx);
    for view in [&left, &right] {
        let (wraps, folds) = view.read_with(cx, |view, _| {
            let model = view.model();
            (model.soft_wrap(), model.display().folds().len())
        });
        assert_eq!((wraps, folds), (false, 0));
    }
    for row in 0..6 {
        assert_eq!(row_y(&left, row, cx), row_y(&right, row, cx));
    }
}

#[gpui::test]
fn too_large_to_compare(cx: &mut TestAppContext) {
    // Over fe-editor's diff limit, with a change.
    let (left_text, right_text) = wide_texts(26_000);
    assert!(left_text.len() > fe_editor::text_diff::MAX_DIFF_BYTES);
    let (diff, cx) = diff_view(&left_text, &right_text, cx);
    assert!(diff.read_with(cx, |diff, _| diff.diff().too_large()));
    let status = diff.read_with(cx, |diff, _| diff.status());
    assert_eq!(status.as_deref(), Some(TOO_LARGE));
    dispatch(NEXT_CHANGE, cx);
    assert_eq!(message(&diff, cx).as_deref(), Some(TOO_LARGE));
}

#[gpui::test]
fn edits_from_another_window(app: &mut TestAppContext) {
    app.update(init);
    let file = EditorModel::new(Buffer::new(&text(&["a", "X", "d"])), Some("a.rs".into()));
    let right = file.split();
    let window =
        app.add_window(move |_, cx| EditorView::new(file, EditorTheme::default_dark(cx), cx));
    let other = window.root(app).unwrap();
    let mut other_cx = VisualTestContext::from_window(window.into(), app);
    let (diff, cx) = diff_view_of(&text(&["a", "b", "c", "d"]), right, app);
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    assert_eq!(blocks(&right, cx), [filler(2, 1, 0)]);

    // Typed in the file's editor, in its window: within a poll, the diff
    // view takes it in, with no frame of its own.
    other_cx.update(|window, cx| window.focus(&other.focus_handle(cx), cx));
    other.update(&mut other_cx, |view, _| view.model_mut().set_cursor(2));
    other_cx.simulate_input("Y\n");
    other_cx.executor().advance_clock(POLL);
    other_cx.run_until_parked();
    assert_eq!(blocks(&right, cx), []);
    assert_eq!(blocks(&left, cx), []);
    assert!(diff.read_with(cx, |diff, cx| diff.is_dirty(cx)));

    // Typed in the diff view: the file's window redraws, showing it.
    let painted = |other_cx: &mut VisualTestContext| {
        other.read_with(other_cx, |view, _| view.model().text().to_string())
    };
    right.update(cx, |view, _| view.model_mut().set_cursor(0));
    cx.simulate_input("Z");
    draw(cx);
    other_cx.run_until_parked();
    assert_eq!(painted(&mut other_cx), text(&["Za", "Y", "X", "d"]));
}

#[gpui::test]
fn navigation_leaves_the_other_sides_visual_mode(cx: &mut TestAppContext) {
    let (diff, cx) = diff_view(
        &text(&["a", "b", "c", "d", "e"]),
        &text(&["a", "B", "c", "D", "e"]),
        cx,
    );
    cx.update(|_, cx| crate::set_vim_enabled(cx, true));
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    // The left in visual mode over its last rows, then the right focused.
    cx.update(|window, cx| window.focus(&left.focus_handle(cx), cx));
    cx.simulate_keystrokes("G v k");
    draw(cx);
    assert_eq!(caret(&left, cx), 3);
    cx.update(|window, cx| window.focus(&right.focus_handle(cx), cx));
    dispatch(NEXT_CHANGE, cx);
    draw(cx);
    assert_eq!((caret(&left, cx), caret(&right, cx)), (1, 1));
    let visual = left.read_with(cx, |view, _| {
        matches!(
            view.vim.as_ref().map(|vim| vim.mode()),
            Some(fe_vim::Mode::Visual(_))
        )
    });
    assert!(!visual);
    let selection = left.read_with(cx, |view, _| view.model().primary_selection().range);
    assert!(selection.is_empty());
    // And the next move counts from there.
    dispatch(NEXT_CHANGE, cx);
    assert_eq!((caret(&left, cx), caret(&right, cx)), (3, 3));
}

#[gpui::test]
fn messages_follow_the_diff(cx: &mut TestAppContext) {
    let same = text(&["a", "b"]);
    let (diff, cx) = diff_view(&same, &same, cx);
    let right = side(&diff, 1, cx);
    let status = |cx: &mut VisualTestContext| {
        diff.read_with(cx, |diff, _| diff.status().map(|status| status.to_string()))
    };
    dispatch(NEXT_CHANGE, cx);
    assert_eq!(status(cx).as_deref(), Some(NO_CHANGES));

    // Grown past the diff limit: the warning, not what navigation said
    // of the texts before.
    let big = "x".repeat(fe_editor::text_diff::MAX_DIFF_BYTES + 1);
    right.update(cx, |view, cx| {
        view.model_mut().insert(&big);
        cx.notify();
    });
    draw(cx);
    assert!(diff.read_with(cx, |diff, _| diff.diff().too_large()));
    assert_eq!(status(cx).as_deref(), Some(TOO_LARGE));
    assert_eq!(message(&diff, cx), None);

    // Back under it, compared again: nothing stale.
    right.update(cx, |view, cx| {
        view.model_mut().undo().unwrap();
        cx.notify();
    });
    draw(cx);
    assert!(!diff.read_with(cx, |diff, _| diff.diff().too_large()));
    assert_eq!(status(cx), None);

    // A message from navigation goes when an edit changes the diff.
    dispatch(NEXT_CHANGE, cx);
    assert_eq!(status(cx).as_deref(), Some(NO_CHANGES));
    right.update(cx, |view, cx| {
        view.model_mut().insert("c");
        cx.notify();
    });
    draw(cx);
    assert_eq!(status(cx), None);
}

#[gpui::test]
fn navigation_across_tall_hunks_keeps_the_sides_together(cx: &mut TestAppContext) {
    // A hundred rows added on the right, more than the view shows, and a
    // hundred removed from it further on.
    let left: Vec<String> = (0..600).map(|row| format!("r{row}")).collect();
    let mut right = left.clone();
    right.drain(400..500);
    for row in 0..100 {
        right.insert(200, format!("new{row}"));
    }
    let (diff, cx) = diff_view(&left.join("\n"), &right.join("\n"), cx);
    cx.update(|_, cx| crate::set_vim_enabled(cx, true));
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    let viewport = right.read_with(cx, |view, _| view.viewport_rows().unwrap().1);
    assert!(viewport < 100, "{viewport}");
    for focused in [&right, &left] {
        let peer = if focused == &right { &left } else { &right };
        for _ in 0..3 {
            // The peer in visual mode, as if left there.
            cx.update(|window, cx| window.focus(&peer.focus_handle(cx), cx));
            cx.simulate_keystrokes("v");
            cx.update(|window, cx| window.focus(&focused.focus_handle(cx), cx));
            dispatch(NEXT_CHANGE, cx);
            draw(cx);
            draw(cx);
            let (focused_top, peer_top) = (scroll_top(focused, cx), scroll_top(peer, cx));
            assert_eq!(focused_top, peer_top);
            let row = caret(focused, cx);
            let (first, rows) = focused.read_with(cx, |view, _| view.viewport_rows().unwrap());
            assert!(
                (first..first + rows).contains(&row),
                "{row} in {first}+{rows}"
            );
        }
    }
}

#[gpui::test]
fn a_save_elsewhere_redraws_the_tab(cx: &mut TestAppContext) {
    let (diff, cx) = diff_view(&text(&["a"]), &text(&["a"]), cx);
    let right = side(&diff, 1, cx);
    right.update(cx, |view, cx| {
        view.model_mut().insert("b");
        cx.notify();
    });
    draw(cx);
    assert!(diff.read_with(cx, |diff, cx| diff.is_dirty(cx)));
    let notified = Rc::new(std::cell::Cell::new(0));
    let _observing = cx.update(|_, cx| {
        let notified = notified.clone();
        cx.observe(&diff, move |_, _| notified.set(notified.get() + 1))
    });
    // Saved through another model, which tells neither side.
    right.update(cx, |view, _| view.model_mut().buffer_mut().did_save());
    cx.executor().advance_clock(POLL);
    cx.run_until_parked();
    assert!(notified.get() > 0);
    assert!(!diff.read_with(cx, |diff, cx| diff.is_dirty(cx)));
}

#[path = "diff_view_language_tests.rs"]
mod language_tests;
