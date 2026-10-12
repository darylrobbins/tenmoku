//! Proposed changes shown as diff blocks ([`super::proposal`]): each
//! pending or stale hunk's new text in a block under the text it replaces,
//! its old rows banded, a stale one with its reason; none for a hunk a fold
//! hides; clicks on the rows around the blocks; and proposals shared by
//! the views of one buffer.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{Buffer, HunkId, HunkStatus, Origin, Outcome, ProposedChange};
use fe_display::layout::Rect;
use fe_editor::EditorModel;
use gpui::{
    AppContext as _, Bounds, Context, Entity, Focusable as _, IntoElement, Modifiers, MouseButton,
    ParentElement as _, Pixels, Point, Render, Styled as _, TestAppContext, VisualTestContext,
    Window, div, px,
};

use crate::*;

/// Rows `r0` to `r9`.
const TEXT: &str = "r0\nr1\nr2\nr3\nr4\nr5\nr6\nr7\nr8\nr9\n";
/// [`TEXT`] with row 2 replaced by two rows and row 6 deleted.
const PROPOSED: &str = "r0\nr1\na\n\tb\nr3\nr4\nr5\nr7\nr8\nr9\n";

/// A focused editor on [`TEXT`], vim off, the caret at the start.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(TEXT), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, false));
    cx.run_until_parked();
    (view, cx)
}

/// [`PROPOSED`] against the view's text, by lines.
fn proposal(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Rc<RefCell<ProposedChange>> {
    view.read_with(cx, |view, _| {
        let snapshot = view.model().snapshot();
        let ai = Origin::Ai {
            agent: "agent".into(),
            session: "session".into(),
        };
        let proposal = ProposedChange::from_text(snapshot, 0..TEXT.len(), PROPOSED, ai);
        Rc::new(RefCell::new(proposal))
    })
}

/// The ids of `proposal`'s hunks, in text order.
fn hunk_ids(
    proposal: &Rc<RefCell<ProposedChange>>,
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<HunkId> {
    view.read_with(cx, |view, _| {
        let hunks = proposal.borrow().hunks(view.model().snapshot());
        hunks.iter().map(|hunk| hunk.id).collect()
    })
}

fn show(
    view: &Entity<EditorView>,
    proposals: Vec<Rc<RefCell<ProposedChange>>>,
    cx: &mut VisualTestContext,
) {
    view.update(cx, |view, cx| view.set_proposals(proposals, cx));
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

type Painted = (HunkId, HunkStatus, String, Bounds<Pixels>);

/// The proposals' blocks a fresh frame paints.
fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Painted> {
    draw(cx);
    view.read_with(cx, |view, _| view.painted_proposals.clone())
}

/// The bands behind old rows the last frame laid out, in window
/// coordinates.
fn removed(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let removed = layout.proposals.removed.iter();
        removed.map(|&band| layout.bounds(band)).collect()
    })
}

fn line_height(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.line_height
    })
}

/// The window bounds of display row `row` across the frame in the last
/// frame.
fn row_bounds(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let frame = &layout.frame;
        layout.bounds(Rect {
            x: 0.,
            y: frame.row_top(row),
            width: frame.width,
            height: frame.line_height,
        })
    })
}

/// The window position a little into the text on display row `row`.
fn on_row(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Point<Pixels> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let frame = &layout.frame;
        let rect = Rect {
            x: frame.text_left + 3.,
            y: frame.row_top(row),
            width: 1.,
            height: frame.line_height,
        };
        layout.bounds(rect).center()
    })
}

fn click(at: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

fn head(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |view, _| view.model().primary_selection().head())
}

#[gpui::test]
fn blocks_under_hunks(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    draw(cx);
    let line_height = line_height(&view, cx);
    let (row_2, row_3, row_6, row_7) = (
        row_bounds(&view, 2, cx),
        row_bounds(&view, 3, cx),
        row_bounds(&view, 6, cx),
        row_bounds(&view, 7, cx),
    );
    assert_eq!(painted(&view, cx), []);
    assert_eq!(removed(&view, cx), []);

    let proposal = proposal(&view, cx);
    let ids = hunk_ids(&proposal, &view, cx);
    assert_eq!(ids.len(), 2, "a replacement and a deletion");
    show(&view, vec![proposal.clone()], cx);
    assert_eq!(view.read_with(cx, |view, _| view.proposals().len()), 1);
    let painted = painted(&view, cx);

    // A header and two rows of new text under row 2, which it replaces,
    // and a header alone under row 6, which the other deletes.
    assert_eq!(painted.len(), 2);
    let (replaced, deleted) = (&painted[0], &painted[1]);
    let header = "Proposed change".to_string();
    assert_eq!(
        (replaced.0, replaced.1, &replaced.2),
        (ids[0], HunkStatus::Pending, &header)
    );
    assert_eq!(
        (deleted.0, deleted.1, &deleted.2),
        (ids[1], HunkStatus::Pending, &header)
    );
    assert_eq!(replaced.3.size.height, px(3. * line_height));
    assert_eq!(deleted.3.size.height, px(line_height));
    assert_eq!(replaced.3.top(), row_bounds(&view, 2, cx).bottom());
    assert_eq!(deleted.3.top(), row_bounds(&view, 6, cx).bottom());
    // Rows 2 to 6 don't move; those after each block are pushed down by it.
    assert_eq!(row_bounds(&view, 2, cx), row_2);
    assert_eq!(
        row_bounds(&view, 6, cx).top(),
        row_6.top() + px(3. * line_height)
    );
    assert_eq!(
        row_bounds(&view, 3, cx).top(),
        row_3.top() + px(3. * line_height)
    );
    assert_eq!(
        row_bounds(&view, 7, cx).top(),
        row_7.top() + px(4. * line_height)
    );

    // The new text, tabs expanded, under each header.
    let texts = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().proposals.texts()
    });
    assert_eq!(
        texts,
        [
            vec![header.clone(), "a".into(), "    b".into()],
            vec![header]
        ]
    );

    // The old rows are banded across the frame, under their blocks' text.
    let bands = removed(&view, cx);
    assert_eq!(bands, [row_bounds(&view, 2, cx), row_bounds(&view, 6, cx)]);
}

#[gpui::test]
fn edited_hunk_shows_stale(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let proposal = proposal(&view, cx);
    let ids = hunk_ids(&proposal, &view, cx);
    show(&view, vec![proposal.clone()], cx);
    let before = painted(&view, cx);
    assert_eq!(before[0].1, HunkStatus::Pending);

    // Typing inside row 2, the first hunk's old text.
    view.update(cx, |view, _| {
        view.model_mut().set_cursor(TEXT.find("r2").unwrap() + 1)
    });
    cx.simulate_input("x");
    assert!(view.read_with(cx, |view, _| {
        view.model().text().to_string().contains("rx2")
    }));
    let after = painted(&view, cx);

    let reason = view.read_with(cx, |view, _| {
        let dry_run = proposal.borrow().dry_run(view.model().snapshot());
        let outcome = dry_run
            .hunks()
            .iter()
            .find(|hunk| hunk.id == ids[0])
            .unwrap();
        match &outcome.outcome {
            Outcome::Conflict(conflict) => conflict.to_string(),
            other => panic!("{other:?}"),
        }
    });
    assert_eq!(reason, "the text under this change was edited");
    assert_eq!(after.len(), 2);
    assert_eq!(
        (after[0].0, after[0].1, &after[0].2),
        (ids[0], HunkStatus::Stale, &reason)
    );
    // The block keeps its place and size; the other hunk is untouched.
    assert_eq!(after[0].3, before[0].3);
    assert_eq!(after[1], before[1]);
}

#[gpui::test]
fn folded_hunks_hide(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    draw(cx);
    let line_height = line_height(&view, cx);
    let proposal = proposal(&view, cx);
    let ids = hunk_ids(&proposal, &view, cx);
    show(&view, vec![proposal], cx);
    let shown = painted(&view, cx);
    assert_eq!(shown.len(), 2);

    // Rows 6 and 7 behind row 5: the deletion of row 6 goes, block and band.
    view.update(cx, |view, cx| {
        view.model_mut().fold_rows(5..=7);
        cx.notify();
    });
    let folded = painted(&view, cx);
    assert_eq!(
        folded.iter().map(|block| block.0).collect::<Vec<_>>(),
        [ids[0]]
    );
    assert_eq!(removed(&view, cx), [row_bounds(&view, 2, cx)]);
    // Row 8, display row 6 now, is below the first block alone.
    let row_8 = row_bounds(&view, 6, cx);
    assert_eq!(row_8.top(), shown[0].3.bottom() + px(3. * line_height));

    // Unfolded, it's back where it was.
    view.update(cx, |view, cx| {
        view.model_mut().unfold_rows(5..=7);
        cx.notify();
    });
    assert_eq!(painted(&view, cx), shown);
    assert_eq!(removed(&view, cx).len(), 2);

    // A fold whose header is the replaced row: the row stays banded, and
    // its block follows the fold.
    view.update(cx, |view, cx| {
        view.model_mut().fold_rows(2..=4);
        cx.notify();
    });
    let headed = painted(&view, cx);
    assert_eq!(headed.len(), 2);
    assert_eq!(headed[0].3.top(), row_bounds(&view, 2, cx).bottom());
    assert_eq!(removed(&view, cx)[0], row_bounds(&view, 2, cx));
    // Row 5, display row 3 now, is under it.
    assert_eq!(row_bounds(&view, 3, cx).top(), headed[0].3.bottom());
}

#[gpui::test]
fn clicks_land_on_text_rows(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let proposal = proposal(&view, cx);
    show(&view, vec![proposal], cx);
    let blocks = painted(&view, cx);
    let row_start = |row: usize| {
        TEXT.split_inclusive('\n')
            .take(row)
            .map(str::len)
            .sum::<usize>()
    };

    // Below each block, on rows 3 and 7.
    for row in [3, 7] {
        click(on_row(&view, row, cx), cx);
        assert_eq!(head(&view, cx), row_start(row as usize), "row {row}");
    }
    // Inside each block's rows: the end of the row above it.
    for (block, above) in [(&blocks[0], 2), (&blocks[1], 6)] {
        let bounds = block.3;
        for y in [
            bounds.top() + px(2.),
            bounds.center().y,
            bounds.bottom() - px(2.),
        ] {
            click(gpui::point(bounds.left() + px(5.), y), cx);
            assert_eq!(head(&view, cx), row_start(above) + 2, "under row {above}");
        }
    }
}

/// Two editors over one buffer, one above the other.
struct Split {
    top: Entity<EditorView>,
    bottom: Entity<EditorView>,
}

impl Render for Split {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().h(px(300.)).child(self.top.clone()))
            .child(div().h(px(300.)).child(self.bottom.clone()))
    }
}

#[gpui::test]
fn proposals_per_buffer(cx: &mut TestAppContext) {
    cx.update(init);
    let (split, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(TEXT), None);
        let other = model.split();
        let theme = EditorTheme::default_dark(cx);
        Split {
            top: cx.new(|cx| EditorView::new(model, theme.clone(), cx)),
            bottom: cx.new(|cx| EditorView::new(other, theme, cx)),
        }
    });
    let (top, bottom) = split.read_with(cx, |split, _| (split.top.clone(), split.bottom.clone()));

    // One made against another buffer, even with the same text, is left out.
    let elsewhere = {
        let buffer = Buffer::new(TEXT);
        let proposal =
            ProposedChange::from_text(&buffer.snapshot(), 0..TEXT.len(), PROPOSED, Origin::User);
        Rc::new(RefCell::new(proposal))
    };
    show(&top, vec![elsewhere.clone()], cx);
    assert!(top.read_with(cx, |view, _| view.proposals().is_empty()));
    assert_eq!(painted(&top, cx), []);

    // Both views show the one they share, beside the other's dropped.
    let shared = proposal(&top, cx);
    let ids = hunk_ids(&shared, &top, cx);
    show(&top, vec![elsewhere, shared.clone()], cx);
    show(&bottom, vec![shared.clone()], cx);
    for view in [&top, &bottom] {
        let proposals = view.read_with(cx, |view, _| view.proposals().to_vec());
        assert_eq!(proposals.len(), 1);
        assert!(Rc::ptr_eq(&proposals[0], &shared));
    }
    draw(cx);
    let blocks = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let painted = view.painted_proposals.iter();
            painted
                .map(|block| (block.0, block.1, block.3.size.height))
                .collect::<Vec<_>>()
        })
    };
    let line_height = line_height(&top, cx);
    let expected = vec![
        (ids[0], HunkStatus::Pending, px(3. * line_height)),
        (ids[1], HunkStatus::Pending, px(line_height)),
    ];
    assert_eq!(blocks(&top, cx), expected);
    assert_eq!(blocks(&bottom, cx), expected);

    // A hunk rejected through the proposal goes from both.
    shared.borrow_mut().reject(ids[1]).unwrap();
    top.update(cx, |_, cx| cx.notify());
    bottom.update(cx, |_, cx| cx.notify());
    draw(cx);
    assert_eq!(blocks(&top, cx), expected[..1]);
    assert_eq!(blocks(&bottom, cx), expected[..1]);
}

#[gpui::test]
fn tall_blocks_lay_out_rows_in_view(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    // Row 2 replaced by 100 rows, a block of 101 from screen row 3.
    let new: String = (0..100).map(|n| format!("n{n}\n")).collect();
    let new = TEXT.replace("r2\n", &new);
    let proposal = view.read_with(cx, |view, _| {
        let snapshot = view.model().snapshot();
        ProposedChange::from_text(snapshot, 0..TEXT.len(), &new, Origin::User)
    });
    show(&view, vec![Rc::new(RefCell::new(proposal))], cx);
    view.update(cx, |view, cx| view.set_scroll_top(13., cx));
    let painted = painted(&view, cx);
    assert_eq!(painted.len(), 1);
    let (line_height, height) = view.read_with(cx, |view, _| {
        let frame = &view.layout.as_ref().unwrap().frame;
        (frame.line_height, frame.height)
    });
    assert_eq!(painted[0].3.size.height, px(101. * line_height));

    // Screen row 13 is the block's row 10: new line 9, then those below
    // it to the frame's bottom, inside the block.
    let texts = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().proposals.texts()
    });
    let in_view = (height / line_height).ceil() as usize;
    assert!(9 + in_view < 100, "the block's end is out of view");
    let expected: Vec<String> = (9..9 + in_view).map(|n| format!("n{n}")).collect();
    assert_eq!(texts, [expected]);
}
