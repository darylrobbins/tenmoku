//! Accepting and rejecting a proposal's hunks ([`super::proposal`]): the
//! Accept and Reject buttons in each block's header row, Reject alone on a
//! stale hunk; an accept as the user's own undoable edit, apart from vim's
//! insert session; the events the host hears; resolutions shared by the
//! views of one buffer; and frames that read only the new text in view.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{Buffer, HunkId, HunkStatus, Origin, ProposalError, ProposedChange};
use fe_command::Invocation;
use fe_display::layout::MAX_SHAPED_ROW_BYTES;
use fe_editor::EditorModel;
use gpui::{
    AppContext as _, Bounds, Context, Entity, Focusable as _, IntoElement, Modifiers, MouseButton,
    ParentElement as _, Pixels, Render, Styled as _, TestAppContext, VisualTestContext, Window,
    div, px,
};

use super::ProposalButton;
use crate::{commands::RunCommand, *};

/// Rows `r0` to `r9`.
const TEXT: &str = "r0\nr1\nr2\nr3\nr4\nr5\nr6\nr7\nr8\nr9\n";
/// [`TEXT`] with row 2 replaced by two rows and row 6 deleted.
const PROPOSED: &str = "r0\nr1\na\n\tb\nr3\nr4\nr5\nr7\nr8\nr9\n";
/// [`TEXT`] with the first hunk of [`PROPOSED`] accepted.
const FIRST_ACCEPTED: &str = "r0\nr1\na\n\tb\nr3\nr4\nr5\nr6\nr7\nr8\nr9\n";

fn ai() -> Origin {
    Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    }
}

/// A focused editor on `text`, vim off, the caret at `caret`.
fn editor_on<'a>(
    text: &str,
    caret: usize,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_string();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_cursor(caret);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, false));
    cx.run_until_parked();
    (view, cx)
}

/// A focused editor on [`TEXT`], vim off, the caret on row 8.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    editor_on(TEXT, TEXT.find("r8").unwrap() + 1, cx)
}

/// `proposed` against the view's text, by lines, from an agent.
fn proposal_of(
    proposed: &str,
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Rc<RefCell<ProposedChange>> {
    view.read_with(cx, |view, _| {
        let snapshot = view.model().snapshot();
        let whole = 0..snapshot.text().len();
        let proposal = ProposedChange::from_text(snapshot, whole, proposed, ai());
        Rc::new(RefCell::new(proposal))
    })
}

/// [`PROPOSED`] against the view's text, shown in it, and its hunks' ids.
fn shown(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (Rc<RefCell<ProposedChange>>, Vec<HunkId>) {
    let proposal = proposal_of(PROPOSED, view, cx);
    let ids = hunk_ids(&proposal, view, cx);
    assert_eq!(ids.len(), 2, "a replacement and a deletion");
    view.update(cx, |view, cx| {
        view.set_proposals(vec![proposal.clone()], cx)
    });
    (proposal, ids)
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

/// The status of hunk `id` of `proposal` in the view's snapshot.
fn status(
    proposal: &Rc<RefCell<ProposedChange>>,
    id: HunkId,
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> HunkStatus {
    view.read_with(cx, |view, _| {
        let hunks = proposal.borrow().hunks(view.model().snapshot());
        hunks.iter().find(|hunk| hunk.id == id).unwrap().status
    })
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// The hunks whose blocks a fresh frame paints.
fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<HunkId> {
    draw(cx);
    view.read_with(cx, |view, _| {
        view.painted_proposals.iter().map(|block| block.0).collect()
    })
}

/// The buttons of hunk `id` a fresh frame paints, and their boxes.
fn buttons(
    view: &Entity<EditorView>,
    id: HunkId,
    cx: &mut VisualTestContext,
) -> Vec<(ProposalButton, Bounds<Pixels>)> {
    draw(cx);
    view.read_with(cx, |view, _| {
        let buttons = view.proposal_buttons.iter();
        let ours = buttons.filter(|button| button.0 == id);
        ours.map(|&(_, button, bounds)| (button, bounds)).collect()
    })
}

/// Clicks the middle of hunk `id`'s `button`.
fn press(
    view: &Entity<EditorView>,
    id: HunkId,
    button: ProposalButton,
    cx: &mut VisualTestContext,
) {
    let buttons = buttons(view, id, cx);
    let (_, bounds) = buttons.iter().find(|(shown, _)| *shown == button).unwrap();
    let at = bounds.center();
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

/// The events `view` emits from now on.
fn events(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Rc<RefCell<Vec<ProposalEvent>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    cx.update(|_, cx| {
        let push = move |_, event: &ProposalEvent, _: &mut gpui::App| {
            seen.borrow_mut().push(event.clone())
        };
        cx.subscribe(view, push).detach();
    });
    events
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn head(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |view, _| view.model().primary_selection().head())
}

fn version(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> fe_buffer::Version {
    view.read_with(cx, |view, _| view.model().buffer().current().version())
}

fn run(command: &str, cx: &mut VisualTestContext) {
    let invocation = Invocation::of(command).unwrap();
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    cx.run_until_parked();
}

#[gpui::test]
fn accept_button_applies_hunk(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let (proposal, ids) = shown(&view, cx);
    let events = events(&view, cx);
    for &id in &ids {
        let shown: Vec<_> = buttons(&view, id, cx).iter().map(|b| b.0).collect();
        assert_eq!(shown, [ProposalButton::Accept, ProposalButton::Reject]);
    }

    press(&view, ids[0], ProposalButton::Accept, cx);

    // The new text is in; the caret is on the same text, and no drag began.
    assert_eq!(text(&view, cx), FIRST_ACCEPTED);
    assert_eq!(head(&view, cx), FIRST_ACCEPTED.find("r8").unwrap() + 1);
    assert!(view.read_with(cx, |view, _| view.drag.is_none()));
    assert_eq!(status(&proposal, ids[0], &view, cx), HunkStatus::Accepted);
    // The event names the transaction undo takes back.
    let transaction = match events.borrow().as_slice() {
        [ProposalEvent::Accepted { hunk, transaction }] if *hunk == ids[0] => *transaction,
        other => panic!("{other:?}"),
    };
    // The next frame paints the other hunk's block alone.
    assert_eq!(painted(&view, cx), [ids[1]]);
    assert!(buttons(&view, ids[0], cx).is_empty());
    let undone = view.update(cx, |view, _| view.model_mut().undo());
    assert_eq!(undone, Ok(Some(transaction)));
    assert_eq!(text(&view, cx), TEXT);
}

#[gpui::test]
fn reject_button_keeps_text(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let (proposal, ids) = shown(&view, cx);
    let events = events(&view, cx);
    let (caret, before) = (head(&view, cx), version(&view, cx));

    press(&view, ids[0], ProposalButton::Reject, cx);

    assert_eq!(text(&view, cx), TEXT);
    assert_eq!(version(&view, cx), before);
    assert_eq!(head(&view, cx), caret);
    assert_eq!(status(&proposal, ids[0], &view, cx), HunkStatus::Rejected);
    assert_eq!(*events.borrow(), [ProposalEvent::Rejected { hunk: ids[0] }]);
    assert_eq!(painted(&view, cx), [ids[1]]);
}

#[gpui::test]
fn stale_hunk_offers_reject_only(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let (proposal, ids) = shown(&view, cx);
    // Typing inside row 2, the first hunk's old text.
    view.update(cx, |view, _| {
        view.model_mut().set_cursor(TEXT.find("r2").unwrap() + 1)
    });
    cx.simulate_input("x");
    let edited = text(&view, cx);
    assert!(edited.contains("rx2"));
    assert_eq!(status(&proposal, ids[0], &view, cx), HunkStatus::Stale);

    let shown = |id, cx: &mut VisualTestContext| -> Vec<ProposalButton> {
        buttons(&view, id, cx).iter().map(|b| b.0).collect()
    };
    assert_eq!(shown(ids[0], cx), [ProposalButton::Reject]);
    assert_eq!(
        shown(ids[1], cx),
        [ProposalButton::Accept, ProposalButton::Reject]
    );

    let events = events(&view, cx);
    let before = version(&view, cx);
    let accepted = view.update(cx, |view, cx| view.accept_hunk(ids[0], cx));
    assert_eq!(accepted, Err(ProposalError::Stale));
    assert_eq!(text(&view, cx), edited);
    assert_eq!(version(&view, cx), before);
    assert_eq!(*events.borrow(), []);
    assert_eq!(status(&proposal, ids[0], &view, cx), HunkStatus::Stale);
    assert_eq!(painted(&view, cx), ids);

    // A read-only view takes no edit: a pending hunk offers Reject alone.
    view.update(cx, |view, cx| view.set_read_only(true, cx));
    assert_eq!(shown(ids[1], cx), [ProposalButton::Reject]);
}

#[gpui::test]
fn cmd_z_undoes_an_accepted_hunk(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let (proposal, ids) = shown(&view, cx);
    // Typing on row 8, outside both hunks.
    cx.simulate_input("x");
    let typed = TEXT.replace("r8", "rx8");
    assert_eq!(text(&view, cx), typed);
    press(&view, ids[0], ProposalButton::Accept, cx);
    let both = FIRST_ACCEPTED.replace("r8", "rx8");
    assert_eq!(text(&view, cx), both);

    let accepted_and_gone = |cx: &mut VisualTestContext| {
        assert_eq!(status(&proposal, ids[0], &view, cx), HunkStatus::Accepted);
        assert_eq!(painted(&view, cx), [ids[1]]);
    };
    run("editor.undo", cx);
    assert_eq!(text(&view, cx), typed, "the hunk alone is undone");
    accepted_and_gone(cx);
    run("editor.redo", cx);
    assert_eq!(text(&view, cx), both);
    accepted_and_gone(cx);
    run("editor.undo", cx);
    assert_eq!(text(&view, cx), typed);
    accepted_and_gone(cx);
    run("editor.undo", cx);
    assert_eq!(text(&view, cx), TEXT);
    accepted_and_gone(cx);
    let ai_history = view.read_with(cx, |view, _| view.model().buffer().history_bytes(&ai()));
    assert_eq!(ai_history, 0, "no transaction is the agent's");

    // With vim on, the other hunk accepted inside an insert session on
    // row 8: `Esc` then `u` undoes the hunk alone, then the session.
    cx.update(|_, cx| set_vim_enabled(cx, true));
    view.update(cx, |view, _| {
        view.model_mut().set_cursor(TEXT.find("r8").unwrap() + 1)
    });
    cx.simulate_keystrokes("i");
    cx.simulate_input("y");
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("insert")
    );
    press(&view, ids[1], ProposalButton::Accept, cx);
    cx.simulate_input("z");
    let typed = TEXT.replace("r8", "ryz8");
    assert_eq!(text(&view, cx), typed.replace("r6\n", ""));
    cx.simulate_keystrokes("escape u");
    cx.run_until_parked();
    assert_eq!(text(&view, cx), typed, "the hunk alone is undone");
    assert_eq!(status(&proposal, ids[1], &view, cx), HunkStatus::Accepted);
    assert_eq!(painted(&view, cx), []);
    cx.simulate_keystrokes("u");
    cx.run_until_parked();
    assert_eq!(text(&view, cx), TEXT, "then the session's typing");
    assert_eq!(painted(&view, cx), []);
    let ai_history = view.read_with(cx, |view, _| view.model().buffer().history_bytes(&ai()));
    assert_eq!(ai_history, 0);
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
fn split_views_share_resolution(cx: &mut TestAppContext) {
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
    cx.update(|_, cx| set_vim_enabled(cx, false));
    let (top, bottom) = split.read_with(cx, |split, _| (split.top.clone(), split.bottom.clone()));
    let (shared, ids) = shown(&top, cx);
    bottom.update(cx, |view, cx| view.set_proposals(vec![shared.clone()], cx));
    let (top_events, bottom_events) = (events(&top, cx), events(&bottom, cx));
    assert_eq!(painted(&top, cx), ids);
    assert_eq!(painted(&bottom, cx), ids);

    // Accepted in the top one, it goes from the bottom one's next frame.
    press(&top, ids[0], ProposalButton::Accept, cx);
    assert_eq!(painted(&bottom, cx), [ids[1]]);
    assert_eq!(painted(&top, cx), [ids[1]]);
    assert_eq!(text(&bottom, cx), FIRST_ACCEPTED);
    assert!(matches!(
        top_events.borrow().as_slice(),
        [ProposalEvent::Accepted { hunk, .. }] if *hunk == ids[0]
    ));
    assert_eq!(*bottom_events.borrow(), []);

    // The host rejects the other through the bottom one, as its button
    // would: the text stays, both lose the block, and the bottom one tells.
    let before = version(&bottom, cx);
    let rejected = bottom.update(cx, |view, cx| view.reject_hunk(ids[1], cx));
    assert_eq!(rejected, Ok(()));
    assert_eq!(version(&bottom, cx), before);
    assert_eq!(text(&bottom, cx), FIRST_ACCEPTED);
    assert_eq!(
        *bottom_events.borrow(),
        [ProposalEvent::Rejected { hunk: ids[1] }]
    );
    assert_eq!(painted(&top, cx), []);
    assert_eq!(painted(&bottom, cx), []);

    // Resolved, or not shown, a hunk is no longer pending.
    for id in &ids {
        let again = top.update(cx, |view, cx| view.reject_hunk(*id, cx));
        assert_eq!(again, Err(ProposalError::NotPending));
        let again = top.update(cx, |view, cx| view.accept_hunk(*id, cx).map(drop));
        assert_eq!(again, Err(ProposalError::NotPending));
    }
    top.update(cx, |view, cx| view.set_proposals(Vec::new(), cx));
    let elsewhere = proposal_of(&TEXT.replace("r9", "x"), &top, cx);
    let id = hunk_ids(&elsewhere, &top, cx)[0];
    let unshown = top.update(cx, |view, cx| view.accept_hunk(id, cx).map(drop));
    assert_eq!(unshown, Err(ProposalError::NotPending));
    assert_eq!(top_events.borrow().len(), 1);
    assert_eq!(bottom_events.borrow().len(), 1);
}

/// An editor `width` wide.
struct Narrow {
    view: Entity<EditorView>,
    width: f32,
}

impl Render for Narrow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(div().w(px(self.width)).h(px(300.)).child(self.view.clone()))
    }
}

#[gpui::test]
fn narrow_pane_scrolls_to_stale_reason(cx: &mut TestAppContext) {
    cx.update(init);
    let (narrow, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(TEXT), None);
        let theme = EditorTheme::default_dark(cx);
        Narrow {
            view: cx.new(|cx| EditorView::new(model, theme, cx)),
            width: 200.,
        }
    });
    let view = narrow.read_with(cx, |narrow, _| narrow.view.clone());
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, false));
    let (proposal, ids) = shown(&view, cx);
    // Typing inside row 6, which the second hunk deletes: a stale deletion,
    // a header and no new text.
    view.update(cx, |view, _| {
        view.model_mut().set_cursor(TEXT.find("r6").unwrap() + 1)
    });
    cx.simulate_input("x");
    assert_eq!(status(&proposal, ids[1], &view, cx), HunkStatus::Stale);
    let stale_header = |cx: &mut VisualTestContext| {
        draw(cx);
        view.read_with(cx, |view, _| {
            let layout = view.layout.as_ref().unwrap();
            let headers = layout.proposals.headers();
            let (_, start, end) = *headers.iter().find(|header| header.0 == ids[1]).unwrap();
            (start, end, layout.frame.width)
        })
    };
    let (_, end, width) = stale_header(cx);
    assert!(
        end > width,
        "the reason runs past the pane: {end} > {width}"
    );

    // Scrolled as far right as it goes, the whole reason is in view, and
    // its Reject button.
    view.update(cx, |view, cx| {
        view.scroll.x = px(100_000.);
        view.autoscroll = false;
        cx.notify();
    });
    let (start, end, width) = stale_header(cx);
    assert!(start < 0., "scrolled: {start}");
    assert!(end <= width, "{end} <= {width}");
    let reject = buttons(&view, ids[1], cx);
    assert_eq!(reject.len(), 1);
    let (left, right) = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let frame = &layout.frame;
        let text = layout.bounds(frame.text_bounds());
        (text.left(), text.right())
    });
    let (_, bounds) = reject[0];
    assert!(bounds.left() > left && bounds.right() < right, "{bounds:?}");
    // Not clipped: as wide as when the frame shows it whole.
    let height = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.line_height
    });
    assert!(bounds.size.width > px(height), "{bounds:?}");
}

/// What the last frame read of the proposals' new texts, and how many it
/// has indexed since the view was made.
fn read(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (usize, usize) {
    view.read_with(cx, |view, _| {
        let texts = &view.layout.as_ref().unwrap().proposal_texts;
        (texts.read, texts.indexed)
    })
}

#[gpui::test]
fn frames_read_a_slice_of_a_long_line(cx: &mut TestAppContext) {
    // Row 2 replaced by a line of 2 MB.
    let (view, cx) = editor_on(TEXT, 0, cx);
    let long = "0123456789abcdef".repeat(1 << 17);
    let proposal = proposal_of(&TEXT.replace("r2", &long), &view, cx);
    view.update(cx, |view, cx| view.set_proposals(vec![proposal], cx));
    draw(cx);
    let (first, indexed) = read(&view, cx);
    assert_eq!(indexed, 1);
    assert!(first <= 2 * MAX_SHAPED_ROW_BYTES, "{first}");
    // Each frame after, the caret moving or the view scrolled far along the
    // line, reads a slice of it, and doesn't index it again.
    for step in 1..4 {
        view.update(cx, |view, cx| {
            view.model_mut().set_cursor(step);
            cx.notify();
        });
        draw(cx);
        let (again, indexed) = read(&view, cx);
        assert_eq!(indexed, 1);
        assert!(again <= 2 * MAX_SHAPED_ROW_BYTES, "{again}");
    }
    view.update(cx, |view, cx| {
        view.scroll.x = px(1_000_000.);
        view.autoscroll = false;
        cx.notify();
    });
    draw(cx);
    let (scrolled, indexed) = read(&view, cx);
    assert_eq!(indexed, 1);
    assert!(scrolled <= 2 * MAX_SHAPED_ROW_BYTES, "{scrolled}");
    let texts = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().proposals.texts()
    });
    let shown = &texts[0][1];
    assert!(shown.len() <= MAX_SHAPED_ROW_BYTES && long.contains(shown.as_str()));
    assert!(view.read_with(cx, |view, _| view.scroll.x > px(100_000.)));
}

#[gpui::test]
fn frames_read_the_rows_of_a_tall_block_in_view(cx: &mut TestAppContext) {
    // Row 2 replaced by 100,000 rows, scrolled deep into its block: a frame
    // reads the rows in view.
    let (view, cx) = editor_on(TEXT, 0, cx);
    let tall: String = (0..100_000).map(|n| format!("n{n}\n")).collect();
    let proposal = proposal_of(&TEXT.replace("r2\n", &tall), &view, cx);
    view.update(cx, |view, cx| view.set_proposals(vec![proposal], cx));
    for top in [0., 90_000., 90_010., 50_000.] {
        view.update(cx, |view, cx| view.set_scroll_top(top, cx));
        draw(cx);
        let (read, indexed) = read(&view, cx);
        assert_eq!(indexed, 1);
        assert!(read < 1000, "{read} bytes at {top}");
    }
    let texts = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().proposals.texts()
    });
    // Screen row 50,000 is the block's row 49,997, under the header from
    // screen row 3.
    assert_eq!(texts[0][0], "n49996");
}

#[gpui::test]
fn accept_all_emits_accepted_no_change(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let at = TEXT.find("r6").unwrap();
    let proposal = view.read_with(cx, |view, _| {
        Rc::new(RefCell::new(ProposedChange::from_edits_keeping_noops(
            view.model().snapshot(),
            [(0..0, ""), (at..at + 2, "six")],
            ai(),
        )))
    });
    let ids = hunk_ids(&proposal, &view, cx);
    assert_eq!(ids.len(), 2);
    view.update(cx, |view, cx| {
        view.set_proposals(vec![proposal.clone()], cx)
    });
    let seen = events(&view, cx);
    assert_eq!(view.update(cx, |view, cx| view.accept_all(cx)), 2);
    assert!(text(&view, cx).contains("six"));
    for id in &ids {
        assert_eq!(status(&proposal, *id, &view, cx), HunkStatus::Accepted);
    }
    let seen = seen.borrow();
    assert!(matches!(
        seen[..],
        [
            ProposalEvent::AcceptedNoChange { hunk },
            ProposalEvent::Accepted { hunk: second, .. },
        ] if hunk == ids[0] && second == ids[1]
    ));

    // Only no-ops: no transaction, but still accepted and announced.
    let (view, cx) = editor(cx);
    let proposal = view.read_with(cx, |view, _| {
        Rc::new(RefCell::new(ProposedChange::from_edits_keeping_noops(
            view.model().snapshot(),
            [(0..0, "")],
            ai(),
        )))
    });
    view.update(cx, |view, cx| {
        view.set_proposals(vec![proposal.clone()], cx)
    });
    let seen = events(&view, cx);
    assert_eq!(view.update(cx, |view, cx| view.accept_all(cx)), 1);
    assert!(matches!(
        seen.borrow()[..],
        [ProposalEvent::AcceptedNoChange { .. }]
    ));
    assert_eq!(text(&view, cx), TEXT);
}
