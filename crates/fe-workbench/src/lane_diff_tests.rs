//! The lane diff ([`super`]): `agent.diff_lane_file`'s Before and
//! `After · lane N` sides, and the right header's agent colour.

use std::{cell::RefCell, rc::Rc, time::SystemTime};

use fe_buffer::{Origin, ProposedChange};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{EditorPalette, themes};
use fe_editor::lane::{Backing, Lane, LaneId, LaneKind, LaneState};
use fe_gpui::{DiffView, EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, Hsla, TestAppContext, VisualTestContext, rgba};
use kiln::core::theme::{Color, ThemeData};

use crate::{
    Workbench,
    lanes::LaneStore,
    tests::find_bar::{draw, editor, opened, pane},
};

const TEXT: &str = "one\ntwo\nthree\nfour\n";

fn lane_2() -> Origin {
    Origin::Ai {
        agent: "claude-code".into(),
        session: "s2".into(),
    }
}

fn hsla(color: Color) -> Hsla {
    rgba(color.to_rgba_u32()).into()
}

/// A workbench showing [`TEXT`] with lane 2, whose proposal changes row 2.
fn open(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let (dir, workbench, cx) = opened(cx, &[("a.txt", TEXT)]);
    let tab = editor(&pane(&workbench, cx), cx);
    workbench.update(cx, |workbench, cx| {
        workbench.lanes.update(cx, |store, cx| {
            let lane = Lane {
                id: LaneId::from_number(2).unwrap(),
                name: "claude-code".into(),
                agent: "claude-code".into(),
                sessions: vec!["s2".into()],
                kind: LaneKind::External { host: "acp".into() },
                backing: Backing::Shared,
                state: LaneState::Working,
                started: SystemTime::now(),
                revision: None,
                pending: 0,
            };
            *store = LaneStore::restore(vec![lane], Some(3));
            cx.notify();
        });
    });
    draw(cx);
    (dir, workbench, tab, cx)
}

fn propose(tab: &Entity<EditorView>, cx: &mut VisualTestContext) -> Rc<RefCell<ProposedChange>> {
    let proposal = tab.update(cx, |tab, cx| {
        let start = tab.model().text().row_start(1);
        let end = tab.model().text().row_start(2);
        let edits = vec![(start..end, "TWO\n".to_owned())];
        let proposal = ProposedChange::from_edits(tab.model().snapshot(), edits, lane_2());
        let proposal = Rc::new(RefCell::new(proposal));
        tab.set_proposals(vec![proposal.clone()], cx);
        proposal
    });
    draw(cx);
    let snapshot = tab.read_with(cx, |tab, _| tab.model().snapshot().clone());
    let dry = proposal.borrow().dry_run(&snapshot).text().to_string();
    assert_eq!(dry, "one\nTWO\nthree\nfour\n");
    proposal
}

fn run(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    // An action reaches the root's listeners only from focus within.
    cx.update(|window, cx| {
        if window.focused(cx).is_none() {
            window.focus(&workbench.focus_handle(cx), cx);
        }
    });
    let run = RunCommand(
        Invocation::of("agent.diff_lane_file")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn active_diff(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<DiffView>> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.active_item()?.downcast_ref::<DiffView>().cloned()
    })
}

#[gpui::test]
fn diff_lane_file_opens_before_against_after_lane_n(cx: &mut TestAppContext) {
    let (_dir, workbench, tab, cx) = open(cx);
    // The proposal is shown on the editor; the buffer is unchanged.
    propose(&tab, cx);
    run(&workbench, cx);
    let diff = active_diff(&workbench, cx).expect("a diff view opens");
    let (titles, left, right, read_only) = diff.read_with(cx, |diff, cx| {
        let text = |view: &Entity<EditorView>| view.read(cx).model().buffer().text().to_string();
        let right = diff.right().read(cx);
        (
            diff.titles(),
            text(diff.left()),
            text(diff.right()),
            right.is_read_only(),
        )
    });
    assert_eq!(
        titles.map(|title| title.to_string()),
        ["Before", "After · lane 2"]
    );
    assert_eq!(left, TEXT);
    assert_eq!(right, "one\nTWO\nthree\nfour\n");
    assert!(read_only);
    // Nothing was applied to the file's buffer.
    let buffer = tab.read_with(cx, |tab, _| tab.model().buffer().text().to_string());
    assert_eq!(buffer, TEXT);
}

#[gpui::test]
fn the_after_header_takes_the_agent_token_not_the_accent_in_both_themes(cx: &mut TestAppContext) {
    let (_dir, workbench, tab, cx) = open(cx);
    propose(&tab, cx);
    run(&workbench, cx);
    let diff = active_diff(&workbench, cx).expect("a diff view opens");
    for name in [themes::DEFAULT_DARK, themes::DEFAULT_LIGHT] {
        let data = ThemeData::from_file(&themes::find(name).unwrap().file());
        let (palette, _) = EditorPalette::for_theme(&data);
        let sides = diff.read_with(cx, |diff, _| [diff.left().clone(), diff.right().clone()]);
        for side in &sides {
            side.update(cx, |side, cx| side.set_colors(&palette, cx));
        }
        draw(cx);
        let (before, after) = diff.read_with(cx, |diff, cx| {
            (diff.header_color(0, cx), diff.header_color(1, cx))
        });
        let agent = hsla(data.tokens.agent.base);
        let accent = hsla(data.tokens.accent.base);
        assert_eq!(before, None, "{name}: Before is the header's usual");
        assert_eq!(after, Some(agent), "{name}");
        assert_ne!(after, Some(accent), "{name}");
    }
}

#[gpui::test]
fn diff_lane_file_applies_every_pending_proposal_of_the_lane(cx: &mut TestAppContext) {
    let (_dir, workbench, tab, cx) = open(cx);
    let proposals = tab.update(cx, |tab, cx| {
        let text = tab.model().text();
        let (two, four) = (
            text.row_start(1)..text.row_start(2),
            text.row_start(3)..text.row_start(4),
        );
        let proposals: Vec<_> = [(two, "TWO\n"), (four, "FOUR\n")]
            .into_iter()
            .map(|(range, new)| {
                let edits = vec![(range, new.to_owned())];
                let proposal = ProposedChange::from_edits(tab.model().snapshot(), edits, lane_2());
                Rc::new(RefCell::new(proposal))
            })
            .collect();
        tab.set_proposals(proposals.clone(), cx);
        proposals
    });
    draw(cx);
    assert_eq!(proposals.len(), 2);
    run(&workbench, cx);
    let diff = active_diff(&workbench, cx).expect("a diff view opens");
    let right = diff.read_with(cx, |diff, cx| {
        diff.right().read(cx).model().buffer().text().to_string()
    });
    assert_eq!(right, "one\nTWO\nthree\nFOUR\n");
}
