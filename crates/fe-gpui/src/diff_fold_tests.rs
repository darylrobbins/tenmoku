//! A [`DiffView`]'s unchanged rows folded on both sides behind `N hidden
//! lines` rows: the blocks found from the hunks with three rows of context,
//! shown by their chevrons and labels on both sides at once, labelled with
//! the symbols around them, never over a hunk, shown when a caret gets in,
//! and toggled by `diff.toggle_folding`, side by side and inline.

use fe_buffer::Buffer;
use fe_command::Invocation;
use gpui::{
    Bounds, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, point, px,
};

use std::ops::RangeInclusive;

use super::{
    fold::{HEADER_IDS, Label, ScreenRows, hidden_blocks, reveal, row_count},
    *,
};
use crate::init;

/// `count` rows `r0`, `r1`, …, one a row, without a final line break.
fn rows(count: u32) -> Vec<String> {
    (0..count).map(|row| format!("r{row}")).collect()
}

/// `rows` with each of `changed` rows replaced.
fn changed(rows: &[String], changed: &[u32]) -> Vec<String> {
    let mut rows = rows.to_vec();
    for &row in changed {
        rows[row as usize] = format!("changed{row}");
    }
    rows
}

/// A focused git diff view (`Index` against `Working Tree`) of `left`
/// against `right`, the right side's path `path`, drawn.
fn diff_view<'a>(
    left: &str,
    right: &str,
    path: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<DiffView>, &'a mut VisualTestContext) {
    let right = EditorModel::new(Buffer::new(right), Some(path.into()));
    diff_view_of(left, right, cx)
}

/// [`diff_view`] of `left` against `right`'s model.
fn diff_view_of<'a>(
    left: &str,
    right: EditorModel,
    cx: &'a mut TestAppContext,
) -> (Entity<DiffView>, &'a mut VisualTestContext) {
    cx.update(init);
    let left = EditorModel::new(Buffer::new(left), None);
    let name = (right.path()).map_or("a.txt".to_owned(), |path| path.display().to_string());
    let (diff, cx) = cx.add_window_view(move |_, cx| {
        let left = DiffSide::new("Index", left).read_only();
        DiffView::new(name, left, DiffSide::new("Working Tree", right), cx)
    });
    cx.update(|window, cx| window.focus(&diff.focus_handle(cx), cx));
    draw(cx);
    (diff, cx)
}

/// [`diff_view`] of `count` rows with `rows` changed on the right.
fn changed_view<'a>(
    count: u32,
    rows_changed: &[u32],
    cx: &'a mut TestAppContext,
) -> (Entity<DiffView>, &'a mut VisualTestContext) {
    let left = rows(count);
    let right = changed(&left, rows_changed);
    diff_view(&left.join("\n"), &right.join("\n"), "a.txt", cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn side(diff: &Entity<DiffView>, ix: usize, cx: &mut VisualTestContext) -> Entity<EditorView> {
    diff.read_with(cx, |diff, _| diff.views[ix].clone())
}

fn hidden(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> Vec<HiddenBlock> {
    diff.read_with(cx, |diff, _| diff.hidden_blocks().to_vec())
}

/// Side `ix`'s blocks hidden, as left rows.
fn hidden_left(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> Vec<Range<u32>> {
    hidden(diff, cx)
        .into_iter()
        .map(|block| block.left)
        .collect()
}

/// One block's rows, as [`hidden_left`] lists them.
fn only(rows: Range<u32>) -> Vec<Range<u32>> {
    vec![rows]
}

/// A side's folds, as `start..=end`.
fn folds(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<RangeInclusive<u32>> {
    view.read_with(cx, |view, _| {
        (view.model().display().folds().iter())
            .map(|fold| fold.start..=fold.end)
            .collect()
    })
}

/// The buffer rows the last frame drew, in order.
fn drawn_rows(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    view.read_with(cx, |view, _| {
        let frame = &view.layout.as_ref().expect("drawn").frame;
        frame.rows.iter().map(|row| row.buffer_row).collect()
    })
}

/// The window y the last frame drew buffer row `row` at.
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

/// The label rows' blocks of a side, as the rows they're above: the hunk
/// headers' left out.
fn label_blocks(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    view.read_with(cx, |view, _| {
        (view.blocks().iter())
            .filter(|block| block.kind == HostBlockKind::Reserve && block.id < HEADER_IDS)
            .map(|block| block.above)
            .collect()
    })
}

fn labels(diff: &Entity<DiffView>, ix: usize, cx: &mut VisualTestContext) -> Vec<Label> {
    diff.read_with(cx, |diff, cx| diff.labels(ix, cx))
}

/// Where the last frame drew what the debug selector names.
fn drawn(selector: &str, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    // The window keeps selectors for good.
    cx.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
}

/// Clicks what the debug selector names, drawn this frame.
fn click(selector: &str, cx: &mut VisualTestContext) {
    let bounds = drawn(selector, cx).unwrap_or_else(|| panic!("no {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
    draw(cx);
}

fn dispatch(command: &str, cx: &mut VisualTestContext) {
    let invocation = Invocation::of(command).unwrap();
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    draw(cx);
}

fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    view.read_with(cx, |view, _| caret_row(view.model()))
}

fn scroll_top(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    view.read_with(cx, |view, _| view.scroll_top())
}

/// Scrolls the wheel over `view` by `rows` rows.
fn wheel(view: &Entity<EditorView>, rows: f32, cx: &mut VisualTestContext) {
    let at = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        layout.bounds(layout.frame.text_bounds()).center()
    });
    wheel_at(at, rows, cx);
}

/// Scrolls the wheel at window position `at` by `rows` rows.
fn wheel_at(at: gpui::Point<Pixels>, rows: f32, cx: &mut VisualTestContext) {
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Lines(point(0., -rows)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    draw(cx);
    draw(cx);
}

/// Both sides fold the same rows, each its own, and their label rows sit
/// below them, but for a first block's, over the first row: the sides line
/// up.
fn assert_aligned(diff: &Entity<DiffView>, cx: &mut VisualTestContext) {
    let (left, right) = (side(diff, 0, cx), side(diff, 1, cx));
    let blocks = hidden(diff, cx);
    let fold = |rows: &Range<u32>| rows.start.saturating_sub(1)..=rows.end - 1;
    let expected = |pick: fn(&HiddenBlock) -> &Range<u32>| -> Vec<_> {
        (blocks.iter())
            .map(|block| fold(pick(block)))
            .filter(|fold| fold.start() < fold.end())
            .collect()
    };
    assert_eq!(folds(&left, cx), expected(|block| &block.left));
    assert_eq!(folds(&right, cx), expected(|block| &block.right));
    let ends = |pick: fn(&HiddenBlock) -> &Range<u32>| -> Vec<_> {
        (blocks.iter())
            .filter(|block| pick(block).start > 0)
            .map(|block| pick(block).end)
            .collect()
    };
    assert_eq!(label_blocks(&left, cx), ends(|block| &block.left));
    assert_eq!(label_blocks(&right, cx), ends(|block| &block.right));
}

#[test]
fn context() {
    // 80 rows, row 40 (of 1 to 80) changed: rows 37 to 43 shown, 36 and
    // 37 hidden above and below.
    let left = rows(80);
    let right = changed(&left, &[39]);
    let (left, right) = (left.join("\n"), right.join("\n"));
    let diff = TextDiff::new(&left, &right);
    let counts = [row_count(&left), row_count(&right)];
    assert_eq!(counts, [80, 80]);
    let blocks = hidden_blocks(&diff, counts, CONTEXT_ROWS, &[]);
    assert_eq!(
        blocks,
        [
            HiddenBlock {
                left: 0..36,
                right: 0..36
            },
            HiddenBlock {
                left: 43..80,
                right: 43..80
            },
        ]
    );
    assert_eq!(
        blocks.iter().map(HiddenBlock::len).collect::<Vec<_>>(),
        [36, 37]
    );

    // Changes six rows apart or fewer stay one run shown; seven apart, a
    // row hides between them.
    for (gap, between) in [(5, None), (6, None), (7, Some(21..22))] {
        let right = changed(&rows(80), &[17, 18 + gap]);
        let diff = TextDiff::new(&rows(80).join("\n"), &right.join("\n"));
        let blocks = hidden_blocks(&diff, [80, 80], CONTEXT_ROWS, &[]);
        let middle: Vec<_> = (blocks.iter())
            .map(|block| block.left.clone())
            .filter(|rows| rows.start > 0 && rows.end < 80)
            .collect();
        assert_eq!(middle, Vec::from_iter(between), "{gap} rows apart");
    }

    // Each side's rows: five rows added on the right at row 40.
    let left = rows(80);
    let mut right = left.clone();
    for row in 0..5 {
        right.insert(40, format!("new{row}"));
    }
    let diff = TextDiff::new(&left.join("\n"), &right.join("\n"));
    let blocks = hidden_blocks(&diff, [80, 85], CONTEXT_ROWS, &[]);
    assert_eq!(
        blocks,
        [
            HiddenBlock {
                left: 0..37,
                right: 0..37
            },
            HiddenBlock {
                left: 43..80,
                right: 48..85
            },
        ]
    );

    // Rows shown stay shown: in left rows, whatever the right's.
    let mut revealed = Vec::new();
    reveal(&mut revealed, 50..60);
    reveal(&mut revealed, 55..70);
    reveal(&mut revealed, 0..2);
    assert_eq!(revealed, [0..2, 50..70]);
    let blocks = hidden_blocks(&diff, [80, 85], CONTEXT_ROWS, &revealed);
    let pairs: Vec<_> = blocks
        .iter()
        .map(|b| (b.left.clone(), b.right.clone()))
        .collect();
    assert_eq!(pairs, [(2..37, 2..37), (43..50, 48..55), (70..80, 75..85)]);

    // The same texts, or none compared, hide nothing.
    let same = TextDiff::new("a\nb", "a\nb");
    assert_eq!(hidden_blocks(&same, [2, 2], CONTEXT_ROWS, &[]), []);
    assert_eq!(row_count("a\r\nb\rc\n"), 4);
}

#[gpui::test]
fn context_drawn(cx: &mut TestAppContext) {
    let (diff, cx) = changed_view(80, &[39], cx);
    assert_eq!(hidden_left(&diff, cx), [0..36, 43..80]);
    assert_aligned(&diff, cx);
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    let counts: Vec<_> = (labels(&diff, 1, cx).iter())
        .map(|label| label.hidden)
        .collect();
    assert_eq!(counts, [36, 37]);
    // A fold can't hide a text's first row: the first label is drawn over
    // it, the rest folded behind it; rows 37 to 43 follow, then the last
    // label.
    let shown: Vec<u32> = [0].into_iter().chain(36..43).collect();
    assert_eq!(drawn_rows(&right, cx), shown);
    for (ix, view) in [(0, &left), (1, &right)] {
        let label = drawn(&format!("diff-fold:{ix}:0"), cx).expect("the first label");
        assert_eq!(label.top(), row_y(view, 0, cx));
        assert_eq!(label.size.height, row_y(view, 36, cx) - row_y(view, 0, cx));
        let editor = view.read_with(cx, |view, _| view.layout.as_ref().unwrap().origin.x);
        assert_eq!(label.left(), editor);
    }
    // Each label row is drawn, over its block.
    for n in 0..2 {
        for ix in 0..2 {
            assert!(drawn(&format!("diff-fold-label:{ix}:{n}"), cx).is_some());
        }
    }
    let last = drawn("diff-fold:1:1", cx).unwrap();
    assert_eq!(last.top(), row_y(&right, 42, cx) + last.size.height);
    // The first has rows below it to show, the last rows above.
    assert!(drawn("diff-fold-up:1:0", cx).is_some());
    assert!(drawn("diff-fold-down:1:0", cx).is_none());
    assert!(drawn("diff-fold-up:1:1", cx).is_none());
    assert!(drawn("diff-fold-down:1:1", cx).is_some());
}

#[gpui::test]
fn expand(cx: &mut TestAppContext) {
    // 300 rows, two changed: 147 hidden above, 93 between and 47 below.
    let (diff, cx) = changed_view(300, &[150, 250], cx);
    assert_eq!(hidden_left(&diff, cx), [0..147, 154..247, 254..300]);
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));

    // The middle block's down chevron shows its first 20 rows, under the
    // rows shown above it; its up chevron its last 20, over those below.
    click("diff-fold-down:1:1", cx);
    assert_eq!(hidden_left(&diff, cx), [0..147, 174..247, 254..300]);
    assert_aligned(&diff, cx);
    click("diff-fold-up:0:1", cx);
    assert_eq!(hidden_left(&diff, cx), [0..147, 174..227, 254..300]);
    assert_aligned(&diff, cx);
    // Those shown are drawn, after the change, on both sides alike.
    for row in [150, 160, 173] {
        assert_eq!(row_y(&left, row, cx), row_y(&right, row, cx), "row {row}");
    }

    // The label shows it all.
    click("diff-fold-label:1:1", cx);
    assert_eq!(hidden_left(&diff, cx), [0..147, 254..300]);
    assert_aligned(&diff, cx);

    // A chevron near the end shows what's left of a short block: the
    // last, below the view, as its chevron would.
    assert!(drawn("diff-fold-down:1:1", cx).is_none());
    for start in [254, 274, 294] {
        assert_eq!(hidden_left(&diff, cx)[1], start..300);
        diff.update(cx, |diff, cx| diff.expand(start, Expand::Top, cx));
        draw(cx);
    }
    assert_eq!(hidden_left(&diff, cx), only(0..147));
    assert_aligned(&diff, cx);

    // The scroll stays in step, and the rows line up.
    wheel(&right, 30., cx);
    assert!(scroll_top(&right, cx) > 0.);
    assert_eq!(scroll_top(&left, cx), scroll_top(&right, cx));
    let drawn = drawn_rows(&right, cx);
    assert!(drawn[0] > 150, "{drawn:?}");
    for row in drawn {
        assert_eq!(row_y(&left, row, cx), row_y(&right, row, cx), "row {row}");
    }
    wheel(&left, -30., cx);
    assert_eq!(scroll_top(&right, cx), 0.);
    assert_eq!(scroll_top(&left, cx), 0.);

    // The first block's up chevron, steps of 20 from its end.
    click("diff-fold-up:1:0", cx);
    assert_eq!(hidden_left(&diff, cx), only(0..127));
    assert_aligned(&diff, cx);

    // Opening a block's fold on one side, as by its marker, shows it on
    // both.
    left.update(cx, |view, cx| {
        view.model_mut().unfold_rows(0..=0);
        cx.notify();
    });
    draw(cx);
    assert_eq!(hidden_left(&diff, cx), []);
    assert_aligned(&diff, cx);
    assert_eq!(folds(&right, cx), []);
}

#[gpui::test]
fn crumb(cx: &mut TestAppContext) {
    // A function in a module, long enough to hide rows in it, changed at
    // its end, and a function after it changed.
    let mut left = vec!["mod outer {".to_owned(), "    fn inner() {".to_owned()];
    left.extend((0..40).map(|row| format!("        let x{row} = {row};")));
    left.extend(["    }".to_owned(), "}".to_owned()]);
    left.extend((0..20).map(|row| format!("// {row}")));
    left.push("fn last() {".to_owned());
    left.extend((0..11).map(|row| format!("    let y{row} = {row};")));
    left.push("}".to_owned());
    let mut right = left.clone();
    right[40] = "        let changed = 1;".into();
    right[70] = "    let changed = 1;".into();
    let (left, right) = (left.join("\n"), right.join("\n"));
    let (diff, cx) = diff_view(&left, &right, "a.rs", cx);
    cx.run_until_parked();
    draw(cx);
    // Hidden: rows 0 to 36, in inner; the comments and last's first rows;
    // and last's last rows, the last row its own.
    assert_eq!(hidden_left(&diff, cx), [0..37, 44..67, 74..77]);
    for ix in 0..2 {
        let crumbs: Vec<_> = (labels(&diff, ix, cx).into_iter())
            .map(|label| label.crumb.map(|crumb| crumb.to_string()))
            .collect();
        assert_eq!(
            crumbs,
            [
                Some("outer › inner".to_owned()),
                Some("last".to_owned()),
                Some("last".to_owned())
            ],
            "side {ix}"
        );
        assert!(drawn(&format!("diff-fold-crumb:{ix}:0"), cx).is_some());
    }

    // Plain text has no symbols.
    let (plain, cx) = diff_view(&left, &right, "a.txt", cx);
    cx.run_until_parked();
    draw(cx);
    assert_eq!(hidden_left(&plain, cx).len(), 3);
    let crumbs: Vec<_> = labels(&plain, 1, cx)
        .into_iter()
        .map(|label| label.crumb)
        .collect();
    assert_eq!(crumbs, [None, None, None]);
    assert!(drawn("diff-fold-crumb:1:0", cx).is_none());
}

#[gpui::test]
fn navigation(cx: &mut TestAppContext) {
    let (diff, cx) = changed_view(300, &[50, 150, 250], cx);
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    let blocks = hidden_left(&diff, cx);
    assert_eq!(blocks, [0..47, 54..147, 154..247, 254..300]);

    // Every hunk is shown: moving between them opens nothing.
    for (forward, row) in [
        (true, 50),
        (true, 150),
        (true, 250),
        (true, 50),
        (false, 250),
    ] {
        cx.update(|window, cx| {
            diff.update(cx, |diff, cx| match forward {
                true => diff.next_change(window, cx),
                false => diff.previous_change(window, cx),
            })
        });
        draw(cx);
        assert_eq!((caret(&left, cx), caret(&right, cx)), (row, row));
        assert_eq!(hidden_left(&diff, cx), blocks);
        assert!(drawn_rows(&right, cx).contains(&row));
    }

    // A jump into a hidden block shows it, on both sides.
    right.update(cx, |view, cx| {
        let at = view.model().text().row_start(100);
        view.select_and_reveal(at..at, cx);
    });
    draw(cx);
    assert_eq!(hidden_left(&diff, cx), [0..47, 154..247, 254..300]);
    assert_aligned(&diff, cx);
    assert_eq!(caret(&right, cx), 100);

    // As does a caret put there on the left, which opens no fold.
    left.update(cx, |view, cx| {
        let at = view.model().text().row_start(200);
        view.model_mut().set_cursor(at);
        cx.notify();
    });
    draw(cx);
    assert_eq!(hidden_left(&diff, cx), [0..47, 254..300]);
    assert_aligned(&diff, cx);
    assert_eq!(caret(&left, cx), 200);

    // And vim's G, to the last row.
    cx.update(|_, cx| crate::set_vim_enabled(cx, true));
    cx.update(|window, cx| window.focus(&right.focus_handle(cx), cx));
    cx.simulate_keystrokes("G");
    draw(cx);
    assert_eq!(caret(&right, cx), 299);
    assert_eq!(hidden_left(&diff, cx), only(0..47));
    assert_aligned(&diff, cx);
}

#[gpui::test]
fn toggle(cx: &mut TestAppContext) {
    let (diff, cx) = changed_view(80, &[39], cx);
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    // On for a git diff.
    assert!(diff.read_with(cx, |diff, _| diff.is_folding()));
    click("diff-fold-label:1:1", cx);
    assert_eq!(hidden_left(&diff, cx), only(0..36));

    // Off: every row, no labels.
    dispatch(TOGGLE_FOLDING, cx);
    assert!(!diff.read_with(cx, |diff, _| diff.is_folding()));
    assert_eq!(hidden_left(&diff, cx), []);
    for view in [&left, &right] {
        assert_eq!(folds(view, cx), []);
        assert_eq!(label_blocks(view, cx), [0u32; 0]);
    }
    assert_eq!(drawn_rows(&right, cx)[..3], [0, 1, 2]);

    // On again: all hidden again, what was shown too.
    dispatch(TOGGLE_FOLDING, cx);
    assert_eq!(hidden_left(&diff, cx), [0..36, 43..80]);
    assert_aligned(&diff, cx);

    // Inline, the right side folds the same, its labels with it.
    dispatch(TOGGLE_INLINE, cx);
    assert!(diff.read_with(cx, |diff, _| diff.is_inline()));
    assert_eq!(folds(&right, cx), [0..=35, 42..=79]);
    assert_eq!(label_blocks(&right, cx), [80]);
    assert_eq!(label_blocks(&left, cx), [0u32; 0]);
    let shown: Vec<u32> = [0].into_iter().chain(36..43).collect();
    assert_eq!(drawn_rows(&right, cx), shown);
    assert!(drawn("diff-fold-label:1:0", cx).is_some());
    click("diff-fold-up:1:0", cx);
    assert_eq!(hidden_left(&diff, cx), [0..16, 43..80]);
    assert_eq!(folds(&right, cx), [0..=15, 42..=79]);
    dispatch(TOGGLE_FOLDING, cx);
    assert_eq!(folds(&right, cx), []);
    assert_eq!(label_blocks(&right, cx), [0u32; 0]);
}

#[gpui::test]
fn compare_two_files_starts_unfolded(cx: &mut TestAppContext) {
    cx.update(init);
    let left = rows(80);
    let right = changed(&left, &[39]);
    let left = EditorModel::new(Buffer::new(&left.join("\n")), None);
    let right = EditorModel::new(Buffer::new(&right.join("\n")), Some("b.txt".into()));
    let (diff, cx) = cx.add_window_view(move |_, cx| {
        let left = DiffSide::new("a.txt", left).read_only();
        DiffView::new("b.txt", left, DiffSide::new("b.txt", right), cx).with_folding(false)
    });
    cx.update(|window, cx| window.focus(&diff.focus_handle(cx), cx));
    draw(cx);
    assert!(!diff.read_with(cx, |diff, _| diff.is_folding()));
    assert_eq!(hidden_left(&diff, cx), []);
    for ix in 0..2 {
        assert_eq!(folds(&side(&diff, ix, cx), cx), []);
    }
    dispatch(TOGGLE_FOLDING, cx);
    assert_eq!(hidden_left(&diff, cx), [0..36, 43..80]);
    assert_aligned(&diff, cx);
}

#[gpui::test]
fn other_folds_are_undone(cx: &mut TestAppContext) {
    let (diff, cx) = changed_view(80, &[39], cx);
    let right = side(&diff, 1, cx);
    // A fold of the rows shown, as zf would make: undone, the blocks'
    // kept.
    right.update(cx, |view, cx| {
        view.model_mut().fold_rows(37..=41);
        view.model_mut().set_soft_wrap(true);
        cx.notify();
    });
    draw(cx);
    assert_eq!(hidden_left(&diff, cx), [0..36, 43..80]);
    assert_aligned(&diff, cx);
    assert!(!right.read_with(cx, |view, _| view.model().soft_wrap()));
}

#[gpui::test]
fn an_edit_keeps_what_was_shown(cx: &mut TestAppContext) {
    let (diff, cx) = changed_view(200, &[100], cx);
    click("diff-fold-label:1:0", cx);
    assert_eq!(hidden_left(&diff, cx), only(104..200));
    // A row typed into the right, below what's shown: the diff lands with
    // a new hunk, rows shown stay shown in the left's rows.
    let right = side(&diff, 1, cx);
    right.update(cx, |view, cx| {
        let at = view.model().text().row_start(150);
        view.model_mut()
            .buffer_mut()
            .edit([(at..at, "new\n")], fe_buffer::Origin::User);
        cx.notify();
    });
    draw(cx);
    let blocks = hidden(&diff, cx);
    let pairs: Vec<_> = blocks
        .iter()
        .map(|b| (b.left.clone(), b.right.clone()))
        .collect();
    assert_eq!(pairs, [(104..147, 104..147), (153..200, 154..201)]);
    assert_aligned(&diff, cx);
}

#[test]
fn screen_rows() {
    let filler = |above, rows| HostBlock {
        above,
        rows,
        kind: HostBlockKind::Filler,
        id: 0,
    };
    // Rows 1 to 9 folded under 0, a label above 10; two fillers above 20.
    let screen = ScreenRows::new(&[filler(10, 1), filler(20, 2)], only(1..10));
    let rows = [(0, 0), (10, 2), (19, 11), (20, 14), (25, 19)];
    for (row, at) in rows {
        assert_eq!(screen.screen_row(row), at, "row {row}");
        assert_eq!(screen.row_at_screen(at), (row, false), "screen {at}");
    }
    // A hidden row is on its fold's header.
    assert_eq!(screen.screen_row(5), 0);
    // On a block's row, the row below it.
    assert_eq!(screen.row_at_screen(1), (10, true));
    assert_eq!(screen.row_at_screen(12), (20, true));
    assert_eq!(screen.row_at_screen(13), (20, true));
}

#[gpui::test]
fn inline_keeps_the_top_row(cx: &mut TestAppContext) {
    let (diff, cx) = changed_view(300, &[150, 250], cx);
    let right = side(&diff, 1, cx);
    click("diff-fold-label:1:1", cx);
    wheel(&right, 12., cx);
    let top = right.read_with(cx, |view, _| view.top_buffer_line());
    assert!(top.is_some_and(|top| top > 150.), "{top:?}");
    for _ in 0..2 {
        dispatch(TOGGLE_INLINE, cx);
        draw(cx);
        assert_eq!(right.read_with(cx, |view, _| view.top_buffer_line()), top);
    }
}

/// Puts side `view`'s carets at the starts of `rows`, the first primary,
/// as the model would, opening no fold.
fn set_carets(view: &Entity<EditorView>, rows: &[u32], cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let text = view.model().text().clone();
        let carets = rows.iter().map(|&row| {
            let at = text.row_start(row);
            fe_buffer::ResolvedSelection {
                range: at..at,
                reversed: false,
            }
        });
        view.model_mut().set_selections(carets.collect::<Vec<_>>());
        cx.notify();
    });
    draw(cx);
}

/// Side `view`'s carets' rows, sorted.
fn caret_rows(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    view.read_with(cx, |view, _| {
        let model = view.model();
        let text = model.text();
        (model.selections().iter())
            .map(|selection| text.offset_to_point(selection.head()).row)
            .collect()
    })
}

#[gpui::test]
fn several_carets(cx: &mut TestAppContext) {
    let (diff, cx) = changed_view(300, &[50, 150, 250], cx);
    let right = side(&diff, 1, cx);
    assert_eq!(hidden_left(&diff, cx), [0..47, 54..147, 154..247, 254..300]);

    // Carets besides the primary put in two hidden blocks show both.
    set_carets(&right, &[50, 100, 200], cx);
    assert_eq!(hidden_left(&diff, cx), [0..47, 254..300]);
    assert_aligned(&diff, cx);
    assert_eq!(caret_rows(&right, cx), [50, 100, 200]);

    // Folding turned on with carets in rows it would hide shows their
    // blocks, the carets where they were.
    dispatch(TOGGLE_FOLDING, cx);
    set_carets(&right, &[20, 150, 280], cx);
    dispatch(TOGGLE_FOLDING, cx);
    assert_eq!(hidden_left(&diff, cx), [54..147, 154..247]);
    assert_aligned(&diff, cx);
    assert_eq!(caret_rows(&right, cx), [20, 150, 280]);
}

#[gpui::test]
fn a_second_caret_on_the_first_row(cx: &mut TestAppContext) {
    let (diff, cx) = changed_view(80, &[39], cx);
    let right = side(&diff, 1, cx);
    // Folding turned on with carets on rows 0 and 39: the first block,
    // whose label would cover row 0, stays shown.
    dispatch(TOGGLE_FOLDING, cx);
    set_carets(&right, &[39, 0], cx);
    dispatch(TOGGLE_FOLDING, cx);
    assert_eq!(hidden_left(&diff, cx), only(43..80));
    assert_eq!(caret_rows(&right, cx), [0, 39]);
    // Nor does it hide while they stay.
    dispatch(TOGGLE_FOLDING, cx);
    dispatch(TOGGLE_FOLDING, cx);
    draw(cx);
    assert_eq!(hidden_left(&diff, cx), only(43..80));
    // A lone caret on row 0 that isn't at its start is no untouched one.
    set_carets(&right, &[39], cx);
    dispatch(TOGGLE_FOLDING, cx);
    right.update(cx, |view, cx| {
        view.model_mut().set_cursor(1);
        cx.notify();
    });
    dispatch(TOGGLE_FOLDING, cx);
    assert_eq!(hidden_left(&diff, cx), only(43..80));
}

#[gpui::test]
fn a_caret_moved_onto_the_first_row(cx: &mut TestAppContext) {
    let (diff, cx) = changed_view(80, &[39], cx);
    let right = side(&diff, 1, cx);
    // Opened there, under the first label: it stays hidden.
    assert_eq!(caret(&right, cx), 0);
    assert_eq!(hidden_left(&diff, cx), [0..36, 43..80]);
    // Moved off and back: shown.
    set_carets(&right, &[39], cx);
    assert_eq!(hidden_left(&diff, cx), [0..36, 43..80]);
    set_carets(&right, &[0], cx);
    assert_eq!(hidden_left(&diff, cx), only(43..80));
    assert_aligned(&diff, cx);

    // As by vim's gg.
    set_carets(&right, &[39], cx);
    dispatch(TOGGLE_FOLDING, cx);
    dispatch(TOGGLE_FOLDING, cx);
    assert_eq!(hidden_left(&diff, cx), [0..36, 43..80]);
    cx.update(|_, cx| crate::set_vim_enabled(cx, true));
    cx.update(|window, cx| window.focus(&right.focus_handle(cx), cx));
    cx.simulate_keystrokes("g g");
    draw(cx);
    assert_eq!(caret(&right, cx), 0);
    assert_eq!(hidden_left(&diff, cx), only(43..80));
}

#[gpui::test]
fn a_caret_moved_to_offset_zero_stays_shown(cx: &mut TestAppContext) {
    let (diff, cx) = changed_view(80, &[39], cx);
    let right = side(&diff, 1, cx);
    // Untouched, under the first label: hidden.
    assert_eq!(hidden_left(&diff, cx), [0..36, 43..80]);
    // Moved off and back to offset 0: shown, and still after refolds.
    set_carets(&right, &[39], cx);
    set_carets(&right, &[0], cx);
    assert_eq!(hidden_left(&diff, cx), only(43..80));
    dispatch(TOGGLE_FOLDING, cx);
    dispatch(TOGGLE_FOLDING, cx);
    assert_eq!(caret(&right, cx), 0);
    assert_eq!(hidden_left(&diff, cx), only(43..80));
    assert_aligned(&diff, cx);
}

#[gpui::test]
fn a_caret_moved_while_a_diff_is_pending_stays_touched(cx: &mut TestAppContext) {
    let left = rows(80);
    let right = changed(&left, &[39]).join("\n");
    let model = EditorModel::new(Buffer::new(&right), Some("a.txt".into()));
    let mut other = model.split();
    let (diff, cx) = diff_view_of(&left.join("\n"), model, cx);
    let right = side(&diff, 1, cx);
    assert_eq!(hidden_left(&diff, cx), [0..36, 43..80]);

    // An edit through another model leaves the diff pending; the caret
    // moves off row 0 and back across frames meanwhile.
    other
        .buffer_mut()
        .edit([(0..0, "x")], fe_buffer::Origin::User);
    set_carets(&right, &[39], cx);
    draw(cx);
    set_carets(&right, &[0], cx);
    draw(cx);
    cx.executor().advance_clock(POLL);
    draw(cx);
    draw(cx);
    assert_eq!(caret_rows(&right, cx), [0]);
    assert!(hidden_left(&diff, cx).iter().all(|rows| !rows.contains(&0)));
    assert_aligned(&diff, cx);
}

/// The crumbs side `ix`'s labels show.
fn crumbs(diff: &Entity<DiffView>, ix: usize, cx: &mut VisualTestContext) -> Vec<Option<String>> {
    (labels(diff, ix, cx).into_iter())
        .map(|label| label.crumb.map(|crumb| crumb.to_string()))
        .collect()
}

#[gpui::test]
fn crumbs_follow_the_language(cx: &mut TestAppContext) {
    let mut left = vec!["fn inner() {".to_owned()];
    left.extend((0..40).map(|row| format!("    let x{row} = {row};")));
    left.push("}".to_owned());
    let mut right = left.clone();
    right[30] = "    let changed = 1;".into();
    let (left, right) = (left.join("\n"), right.join("\n"));
    let (diff, cx) = diff_view(&left, &right, "a.rs", cx);
    cx.run_until_parked();
    draw(cx);
    let inner = || vec![Some("inner".to_owned()), Some("inner".to_owned())];
    assert_eq!(crumbs(&diff, 1, cx), inner());
    let right = side(&diff, 1, cx);

    // Plain text, the text as it was: no crumbs, the left's kept.
    right.update(cx, |view, cx| view.set_language(None, cx));
    draw(cx);
    assert_eq!(crumbs(&diff, 1, cx), [None, None]);
    assert_eq!(crumbs(&diff, 0, cx), inner());
    // And back.
    right.update(cx, |view, cx| view.set_language(Some("rust"), cx));
    cx.run_until_parked();
    draw(cx);
    assert_eq!(crumbs(&diff, 1, cx), inner());

    // Another language with an outline, the text as it was: its outline,
    // which has nothing in it.
    right.update(cx, |view, cx| view.set_language(Some("python"), cx));
    cx.run_until_parked();
    draw(cx);
    assert_eq!(
        right.read_with(cx, |view, _| view
            .syntax()
            .map(|syntax| syntax.language().name())),
        Some("python")
    );
    assert_eq!(crumbs(&diff, 1, cx), [None, None]);
    right.update(cx, |view, cx| view.set_language(Some("rust"), cx));
    cx.run_until_parked();
    draw(cx);
    assert_eq!(crumbs(&diff, 1, cx), inner());

    // An outline still being found when the language goes isn't kept.
    diff.update(cx, |diff, cx| {
        diff.outlines[1] = None;
        diff.sync_outlines(cx);
        assert!(diff.outlining[1].is_some());
    });
    right.update(cx, |view, cx| view.set_language(None, cx));
    diff.update(cx, |diff, cx| diff.sync_outlines(cx));
    cx.run_until_parked();
    assert!(diff.read_with(cx, |diff, _| diff.outlines[1].is_none()));
    draw(cx);
    assert_eq!(crumbs(&diff, 1, cx), [None, None]);
}

#[gpui::test]
fn the_wheel_over_a_label_scrolls(cx: &mut TestAppContext) {
    let (diff, cx) = changed_view(300, &[150, 250], cx);
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    // Over a label, a chevron and the rest of the row, either side.
    let parts = [
        ("diff-fold-label:1:1", &right, 3.),
        ("diff-fold-up:0:1", &left, -2.),
        ("diff-fold:1:1", &right, 1.),
    ];
    let mut top = 0.;
    for (part, scrolled, rows) in parts {
        let bounds = drawn(part, cx).unwrap_or_else(|| panic!("no {part}"));
        wheel_at(bounds.center(), rows, cx);
        let now = scroll_top(scrolled, cx);
        assert_eq!(now, top + rows, "over {part}");
        assert_eq!(scroll_top(&left, cx), scroll_top(&right, cx), "over {part}");
        top = now;
    }
    assert_eq!(hidden_left(&diff, cx), [0..147, 154..247, 254..300]);
}

#[gpui::test]
fn a_caret_moved_by_an_edit_elsewhere(cx: &mut TestAppContext) {
    // The same 80 rows on both sides, the right caret on row 39.
    let text = rows(80).join("\n");
    let right = EditorModel::new(Buffer::new(&text), Some("a.txt".into()));
    let mut other = right.split();
    let (diff, cx) = diff_view_of(&text, right, cx);
    let right = side(&diff, 1, cx);
    set_carets(&right, &[39], cx);
    assert_eq!(hidden_left(&diff, cx), []);

    // A hundred rows put in at the start through another model, which
    // tells neither side: the caret, now on row 139, keeps its row shown.
    let new = "new\n".repeat(100);
    other
        .buffer_mut()
        .edit([(0..0, new.as_str())], fe_buffer::Origin::User);
    cx.executor().advance_clock(POLL);
    draw(cx);
    draw(cx);
    assert_eq!(caret(&right, cx), 139);
    assert_eq!(hidden_left(&diff, cx), []);
    assert_aligned(&diff, cx);
}

#[gpui::test]
fn set_number_options_leave_a_diff_sides_numbers_absolute(cx: &mut TestAppContext) {
    let (diff, cx) = changed_view(6, &[2], cx);
    let sides = diff.read_with(cx, |diff, _| diff.views.clone());
    for side in &sides {
        side.update(cx, |view, _| {
            view.model_mut().set_number(false);
            view.model_mut().set_relative_number(true);
            view.model_mut().set_line_numbers(fe_display::LineNumbers {
                number: false,
                relative: true,
            });
        });
        let numbers = side.read_with(cx, |view, _| view.model().line_numbers());
        assert_eq!(numbers, fe_display::LineNumbers::default());
    }
}
