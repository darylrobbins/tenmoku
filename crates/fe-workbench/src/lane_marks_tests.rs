//! Editor lane marks ([`super`]): the dotted agent lane in the change bar
//! on an agent's rows, the agent band behind a pending hunk's old rows, and
//! the lane chip in each proposal block's header row.

use std::{
    cell::RefCell,
    ops::Range,
    rc::Rc,
    time::{Duration, SystemTime},
};

use fe_buffer::{HunkStatus, Origin, ProposedChange};
use fe_config::settings::{EditorSettings, WrapMode};
use fe_config::{EditorPalette, themes};
use fe_editor::{
    decorations::Tint,
    lane::{Backing, Lane, LaneId, LaneKind, LaneState},
};
use fe_gpui::{DrawnProposalHeader, EditorView};
use gpui::{Bounds, Entity, Hsla, Pixels, TestAppContext, VisualTestContext, rgba};
use kiln::core::theme::{Color, ThemeData};

use crate::{
    Workbench,
    lanes::LaneStore,
    tests::find_bar::{draw, editor, opened, pane},
};

const TEXT: &str = "one\ntwo\nthree\nfour\nfive\nsix\n";

fn ai(agent: &str, session: &str) -> Origin {
    Origin::Ai {
        agent: agent.into(),
        session: session.into(),
    }
}

/// Lane 2's agent, and lane 3's.
fn lane_2() -> Origin {
    ai("claude-code", "s2")
}

fn lane_3() -> Origin {
    ai("codex", "s3")
}

fn lane(number: u32, agent: &str, session: &str) -> Lane {
    Lane {
        id: LaneId::from_number(number).unwrap(),
        name: agent.into(),
        agent: agent.into(),
        sessions: vec![session.into()],
        kind: LaneKind::External { host: "acp".into() },
        backing: Backing::Shared,
        state: LaneState::Working,
        started: SystemTime::now(),
        revision: None,
        pending: 0,
    }
}

/// A workbench showing [`TEXT`], with lanes 2 and 3 besides the chat's.
fn open(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    open_with(cx, TEXT)
}

/// As [`open`], showing `text`.
fn open_with<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    let (dir, workbench, cx) = opened(cx, &[("a.txt", text)]);
    let tab = editor(&pane(&workbench, cx), cx);
    workbench.update(cx, |workbench, cx| {
        workbench.lanes.update(cx, |store, cx| {
            let record = vec![lane(2, "claude-code", "s2"), lane(3, "codex", "s3")];
            *store = LaneStore::restore(record, Some(4));
            cx.notify();
        });
    });
    draw(cx);
    (dir, workbench, tab, cx)
}

fn edit(
    tab: &Entity<EditorView>,
    range: Range<usize>,
    text: &str,
    origin: Origin,
    cx: &mut VisualTestContext,
) {
    tab.update(cx, |tab, cx| {
        let text = text.to_owned();
        tab.model_mut().buffer_mut().edit([(range, text)], origin);
        cx.notify();
    });
    draw(cx);
}

/// Lets the edits rest, and draws.
fn rest(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(400));
    draw(cx);
}

/// A proposal of `origin`'s replacing each range with its text, shown with
/// those the editor shows already.
fn propose(
    tab: &Entity<EditorView>,
    edits: Vec<(Range<usize>, &str)>,
    origin: Origin,
    cx: &mut VisualTestContext,
) -> Rc<RefCell<ProposedChange>> {
    let proposal = tab.update(cx, |tab, cx| {
        let edits: Vec<_> = (edits.into_iter())
            .map(|(range, text)| (range, text.to_owned()))
            .collect();
        let proposal = ProposedChange::from_edits(tab.model().snapshot(), edits, origin);
        let proposal = Rc::new(RefCell::new(proposal));
        let mut shown = tab.proposals().to_vec();
        shown.push(proposal.clone());
        tab.set_proposals(shown, cx);
        proposal
    });
    draw(cx);
    proposal
}

/// Bytes `rows` of [`TEXT`]-shaped text: from the first's start to the
/// end's.
fn rows(tab: &Entity<EditorView>, rows: Range<u32>, cx: &mut VisualTestContext) -> Range<usize> {
    tab.read_with(cx, |tab, _| {
        let text = tab.model().text();
        text.row_start(rows.start)..text.row_start(rows.end)
    })
}

/// The buffer rows any of `marks` is drawn beside: more than half of it, or
/// of the row, is beside the row.
fn rows_beside(
    tab: &Entity<EditorView>,
    marks: &[Bounds<Pixels>],
    cx: &mut VisualTestContext,
) -> Vec<u32> {
    tab.update(cx, |tab, _| {
        let count = tab.model().text().row_count();
        (0..count)
            .filter(|&row| {
                let offset = tab.model().text().row_start(row);
                let Some(bounds) = tab.caret_bounds_at(offset) else {
                    return false;
                };
                (marks.iter()).any(|mark| {
                    let shared = mark.bottom().min(bounds.bottom()) - mark.top().max(bounds.top());
                    shared * 2. > mark.size.height.min(bounds.size.height)
                })
            })
            .collect()
    })
}

/// The rows the last frame drew the agent lane's dots beside.
fn dotted(tab: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    let dots = tab.read_with(cx, |tab, _| tab.drawn_gutter().unwrap().agent_dots.clone());
    rows_beside(tab, &dots, cx)
}

/// The rows the last frame drew a git change bar beside.
fn barred(tab: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    let bars: Vec<_> = tab.read_with(cx, |tab, _| {
        let bars = &tab.drawn_gutter().unwrap().bars;
        bars.iter().map(|(bounds, _)| *bounds).collect()
    });
    rows_beside(tab, &bars, cx)
}

/// The rows the last frame banded in the agent's tint, and the colours.
fn agent_banded(tab: &Entity<EditorView>, cx: &mut VisualTestContext) -> (Vec<u32>, Vec<Hsla>) {
    let bands: Vec<_> = tab.read_with(cx, |tab, _| {
        (tab.drawn_row_backgrounds().into_iter())
            .filter(|(_, tint, _)| *tint == Tint::Agent)
            .collect()
    });
    let bounds: Vec<_> = bands.iter().map(|(bounds, ..)| *bounds).collect();
    let mut colors: Vec<Hsla> = bands.iter().map(|(.., color)| *color).collect();
    colors.dedup();
    (rows_beside(tab, &bounds, cx), colors)
}

fn headers(tab: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<DrawnProposalHeader> {
    tab.read_with(cx, |tab, _| tab.drawn_proposal_headers())
}

/// Each header's text, its chip's, and its buttons', having checked the
/// chip sits after the text and before the buttons.
fn header_texts(
    tab: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<(String, Option<String>, Vec<String>)> {
    (headers(tab, cx).into_iter())
        .map(|header| {
            if let Some((_, chip, ..)) = &header.chip {
                let first = header.buttons.first().expect("a button");
                assert!(
                    chip.right() <= first.1.left(),
                    "the chip is before the buttons"
                );
            }
            (
                header.text,
                header.chip.map(|(text, ..)| text.to_string()),
                header
                    .buttons
                    .into_iter()
                    .map(|(label, _)| label.to_string())
                    .collect(),
            )
        })
        .collect()
}

fn hsla(color: Color) -> Hsla {
    rgba(color.to_rgba_u32()).into()
}

#[gpui::test]
fn agent_rows_draw_the_dotted_lane_in_place_of_the_git_bar(cx: &mut TestAppContext) {
    let (_dir, _workbench, tab, cx) = open(cx);
    tab.update(cx, |tab, _| {
        tab.model_mut().set_diff_base(Some(TEXT.into()))
    });
    // Row 0 the user's, row 2 lane 2's accepted text, row 4 lane 2's
    // pending hunk.
    edit(&tab, 3..3, "!", Origin::User, cx);
    let three = rows(&tab, 2..3, cx);
    edit(&tab, three.end - 1..three.end - 1, "z", lane_2(), cx);
    let five = rows(&tab, 4..5, cx);
    propose(&tab, vec![(five, "FIVE\n")], lane_2(), cx);
    rest(cx);
    assert_eq!(dotted(&tab, cx), [2, 4]);
    // The git bar shows on every other row it would.
    assert_eq!(barred(&tab, cx), [0]);

    // The user rewrites row 2 whole: it's theirs, and its git bar is back.
    let three = rows(&tab, 2..3, cx);
    edit(&tab, three.start..three.end - 1, "THREE", Origin::User, cx);
    rest(cx);
    assert_eq!(dotted(&tab, cx), [4]);
    assert_eq!(barred(&tab, cx), [0, 2]);

    // Rows the user wrote beside an agent's: a row stays the agent's while
    // any of its text is.
    let four = rows(&tab, 3..4, cx);
    edit(&tab, four.start..four.start, "x", lane_3(), cx);
    edit(&tab, four.start..four.start, "you ", Origin::User, cx);
    rest(cx);
    assert_eq!(dotted(&tab, cx), [3, 4]);
    assert_eq!(barred(&tab, cx), [0, 2]);
}

#[gpui::test]
fn a_word_replaced_in_a_wrapped_row_marks_every_piece_in_view(cx: &mut TestAppContext) {
    let long = format!("{}\ntwo\nthree\n", "word ".repeat(80));
    let (_dir, _workbench, tab, cx) = open_with(cx, &long);
    tab.update(cx, |tab, cx| {
        let settings = EditorSettings {
            soft_wrap: true,
            wrap_mode: WrapMode::Column,
            wrap_column: 20,
            ..EditorSettings::default()
        };
        tab.model_mut().set_soft_wrap(true);
        tab.set_wrap_settings(&settings);
        cx.notify();
    });
    draw(cx);
    // Lane 2 replaces the long row's first word only.
    propose(&tab, vec![(0..4, "WORD")], lane_2(), cx);
    let marks = |cx: &mut VisualTestContext| {
        let dots = tab.read_with(cx, |tab, _| tab.drawn_gutter().unwrap().agent_dots.len());
        let bands = tab.read_with(cx, |tab, _| {
            (tab.drawn_row_backgrounds().into_iter())
                .filter(|(_, tint, _)| *tint == Tint::Agent)
                .count()
        });
        (dots, bands)
    };
    let (dots, bands) = marks(cx);
    assert!(dots > 0 && bands > 1, "{dots} dots, {bands} bands");
    // Scrolled past the word, the pieces of its row in view keep both.
    tab.update(cx, |tab, cx| tab.set_scroll_top(8., cx));
    draw(cx);
    // The frame lays out none of the word, but more of its row.
    let laid = tab.update(cx, |tab, _| {
        (tab.caret_bounds_at(0), tab.caret_bounds_at(300))
    });
    assert!(laid.0.is_none() && laid.1.is_some(), "{laid:?}");
    let (dots, bands) = marks(cx);
    assert!(dots > 0 && bands > 0, "{dots} dots, {bands} bands");
}

#[gpui::test]
fn the_rows_a_pending_hunk_replaces_get_the_agent_band_under_the_diff_bands(
    cx: &mut TestAppContext,
) {
    let (_dir, _workbench, tab, cx) = open(cx);
    let old = rows(&tab, 1..3, cx);
    let proposal = propose(&tab, vec![(old, "TWO\n")], lane_2(), cx);
    let theme = tab.read_with(cx, |tab, _| tab.theme().clone());
    let (banded, colors) = agent_banded(&tab, cx);
    assert_eq!(banded, [1, 2]);
    assert_eq!(colors, [theme.agent_row]);
    // The block and the old rows keep their diff colours.
    let tints = tab.read_with(cx, |tab, _| tab.drawn_proposal_tints());
    assert_eq!(
        tints,
        Some([
            theme.diff_added_row,
            theme.diff_removed_row,
            theme.diff_added_word,
            theme.diff_removed_word,
        ])
    );
    assert_ne!(theme.agent_row, theme.diff_removed_row);

    // Accepted, the rows keep the lane but lose the band.
    let snapshot = tab.read_with(cx, |tab, _| tab.model().snapshot().clone());
    let id = proposal.borrow().hunks(&snapshot)[0].id;
    tab.update(cx, |tab, cx| tab.accept_hunk(id, cx).unwrap());
    draw(cx);
    rest(cx);
    assert_eq!(agent_banded(&tab, cx).0, [] as [u32; 0]);
    assert_eq!(dotted(&tab, cx), [1]);
}

#[gpui::test]
fn the_header_shows_the_lane_chip_before_accept_and_reject(cx: &mut TestAppContext) {
    let (_dir, _workbench, tab, cx) = open(cx);
    let (two, five) = (rows(&tab, 1..2, cx), rows(&tab, 4..5, cx));
    propose(&tab, vec![(two, "TWO\n"), (five, "FIVE\n")], lane_2(), cx);
    let pending = |chip: &str| {
        (
            "Proposed change".to_owned(),
            Some(chip.to_owned()),
            vec!["Accept".to_owned(), "Reject".to_owned()],
        )
    };
    assert_eq!(
        header_texts(&tab, cx),
        [pending("lane 2 · pending"), pending("lane 2 · pending")]
    );

    // The user edits the first hunk's old text: it's stale, a conflict.
    let two = rows(&tab, 1..2, cx);
    edit(&tab, two.start..two.start + 1, "T", Origin::User, cx);
    rest(cx);
    let shown = header_texts(&tab, cx);
    assert_eq!(shown.len(), 2);
    assert_eq!(shown[0].1.as_deref(), Some("lane 2 · 1 conflict"));
    assert_eq!(shown[0].2, ["Reject"]);
    assert_eq!(shown[1], pending("lane 2 · 1 conflict"));

    // And the second's: every hunk is stale.
    let five = rows(&tab, 4..5, cx);
    edit(&tab, five.start..five.start + 1, "F", Origin::User, cx);
    rest(cx);
    let chips: Vec<_> = (header_texts(&tab, cx).into_iter())
        .map(|(_, chip, _)| chip)
        .collect();
    assert_eq!(
        chips,
        [
            Some("lane 2 · stale".to_owned()),
            Some("lane 2 · stale".to_owned())
        ]
    );
    let stale = headers(&tab, cx).into_iter().all(|header| {
        let (_, _, _, color) = header.chip.unwrap();
        color.a < 1.
    });
    assert!(stale, "a stale chip is dimmed");
}

#[gpui::test]
fn two_lanes_pending_on_one_text_each_say_also_changed_by_the_other(cx: &mut TestAppContext) {
    let (_dir, _workbench, tab, cx) = open(cx);
    let two = rows(&tab, 1..2, cx);
    let first = propose(&tab, vec![(two.clone(), "TWO\n")], lane_2(), cx);
    propose(&tab, vec![(two, "deux\n")], lane_3(), cx);
    let texts: Vec<_> = (header_texts(&tab, cx).into_iter())
        .map(|(text, chip, _)| (text, chip))
        .collect();
    assert_eq!(
        texts,
        [
            (
                "Proposed change · also changed by lane 3".to_owned(),
                Some("lane 2 · pending".to_owned())
            ),
            (
                "Proposed change · also changed by lane 2".to_owned(),
                Some("lane 3 · pending".to_owned())
            ),
        ]
    );

    // Lane 2's goes in: lane 3's old text is gone, so it's stale, and says
    // so instead.
    let snapshot = tab.read_with(cx, |tab, _| tab.model().snapshot().clone());
    let id = first.borrow().hunks(&snapshot)[0].id;
    tab.update(cx, |tab, cx| tab.accept_hunk(id, cx).unwrap());
    draw(cx);
    rest(cx);
    let shown = header_texts(&tab, cx);
    assert_eq!(shown.len(), 1);
    assert!(!shown[0].0.contains("also changed"), "{:?}", shown[0].0);
    assert_eq!(shown[0].1.as_deref(), Some("lane 3 · stale"));
}

#[gpui::test]
fn an_inline_edit_draws_the_lane_and_no_chip(cx: &mut TestAppContext) {
    let (_dir, _workbench, tab, cx) = open(cx);
    let inline = ai("fastedit.inline_edit", "inline-1");
    let two = rows(&tab, 1..2, cx);
    let proposal = propose(&tab, vec![(two, "TWO\n")], inline, cx);
    assert_eq!(dotted(&tab, cx), [1]);
    assert_eq!(
        header_texts(&tab, cx),
        [(
            "Proposed change".to_owned(),
            None,
            vec!["Accept".to_owned(), "Reject".to_owned()]
        )]
    );
    // Accepted, its text is still the agent's, in the lane.
    let snapshot = tab.read_with(cx, |tab, _| tab.model().snapshot().clone());
    let id = proposal.borrow().hunks(&snapshot)[0].id;
    tab.update(cx, |tab, cx| tab.accept_hunk(id, cx).unwrap());
    rest(cx);
    assert_eq!(dotted(&tab, cx), [1]);
    assert_eq!(headers(&tab, cx), []);
    assert_eq!(proposal.borrow().status_count(HunkStatus::Accepted), 1);
}

#[gpui::test]
fn lane_marks_take_the_agent_tokens_not_the_accent_in_both_themes(cx: &mut TestAppContext) {
    let (_dir, _workbench, tab, cx) = open(cx);
    let two = rows(&tab, 1..2, cx);
    propose(&tab, vec![(two, "TWO\n")], lane_2(), cx);
    // Both themes, and every other that sets the editor's agent keys.
    let names = [
        themes::DEFAULT_DARK,
        themes::DEFAULT_LIGHT,
        "tenmoku-dark",
        "tenmoku-light",
        "tenmoku-ember",
        "vscode-monokai",
    ];
    for name in names {
        let data = ThemeData::from_file(&themes::find(name).unwrap().file());
        let (palette, _) = EditorPalette::for_theme(&data);
        tab.update(cx, |tab, cx| tab.set_colors(&palette, cx));
        draw(cx);
        let (agent, soft) = (hsla(data.tokens.agent.base), hsla(data.tokens.agent.soft));
        let accent = hsla(data.tokens.accent.base);
        let cursor = tab.read_with(cx, |tab, _| tab.theme().cursor);
        assert_ne!(agent, accent, "{name}");
        assert_ne!(soft, accent, "{name}");
        assert_ne!(agent, cursor, "{name}");

        // The dotted lane.
        let color = tab.read_with(cx, |tab, _| tab.drawn_gutter().unwrap().agent_dot_color);
        assert_eq!(color, agent, "{name}");
        // The row band.
        assert_eq!(agent_banded(&tab, cx).1, [soft], "{name}");
        // The chip: the agent's colour on its soft ground.
        let header = headers(&tab, cx).remove(0);
        let (_, _, ground, text) = header.chip.unwrap();
        assert_eq!((ground, text), (soft, agent), "{name}");
    }
}
