//! The Agents panel and its rail item ([`super`]): the robot head after
//! Changes with its silver-blue count, the lanes' rows and their buttons,
//! clearing, and the agent's colour in both default themes.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    time::SystemTime,
};

use fe_buffer::{Buffer, HunkStatus, Origin, ProposedChange};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::{
    Counts,
    lane::{Backing, Lane, LaneCounts, LaneEvent, LaneId, LaneKind, LaneState},
};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_ui::{BadgeTone, Icon};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln::{
    core::theme::ThemeData,
    ui::{ActiveTheme as _, Theme, ThemeMode, set_themes},
};
use kiln_shell::{PanelId, dock::RailBadge};

use super::{AgentsPanel, LaneAction};
use crate::{
    DockPanel, Launch, Workbench,
    lanes::{LaneStore, Tally, chat_lane},
    tests::open,
};

const AGENT: &str = "acp.claude-agent";
const AGENTS: PanelId = PanelId("agents");

fn lane(number: u32) -> LaneId {
    LaneId::from_number(number).unwrap()
}

/// An external agent's lane `number`, saved as `state`.
fn external(number: u32, state: LaneState) -> Lane {
    Lane {
        id: lane(number),
        name: format!("agent {number}").into(),
        agent: AGENT.into(),
        sessions: vec![format!("s{number}").into()],
        kind: LaneKind::External { host: "acp".into() },
        backing: Backing::Shared,
        state,
        started: SystemTime::UNIX_EPOCH,
        revision: None,
        pending: 0,
    }
}

/// A folder holding `a.rs`, under the real temp folder.
fn project() -> tempfile::TempDir {
    let temp = dunce::canonicalize(std::env::temp_dir()).unwrap();
    let dir = tempfile::tempdir_in(temp).unwrap();
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    dir
}

/// A workbench on `root`, showing its `a.rs`.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext, PathBuf) {
    let path = root.join("a.rs");
    let launch = Launch {
        root: Some(root.to_owned()),
        file: Some((path.clone(), Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx, path)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn lanes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<LaneStore> {
    workbench.read_with(cx, |workbench, _| workbench.lanes.clone())
}

fn panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<AgentsPanel> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).agents().clone()
    })
}

/// Puts the record `saved` in place of the window's lanes, as a restore
/// would, lane 1 the chat.
fn restore(workbench: &Entity<Workbench>, saved: Vec<Lane>, cx: &mut VisualTestContext) {
    let next = saved.iter().map(|lane| lane.id.get() + 1).max();
    lanes(workbench, cx).update(cx, |store, cx| {
        *store = LaneStore::restore(saved, next);
        cx.notify();
    });
    draw(cx);
}

/// Lane `id` counted as `tally`, as the workbench counts it.
fn recount(workbench: &Entity<Workbench>, id: LaneId, tally: Tally, cx: &mut VisualTestContext) {
    lanes(workbench, cx).update(cx, |store, cx| {
        store.recount(id, tally);
        cx.notify();
    });
    draw(cx);
}

/// Lane `id`'s turn starts or ends, with `pending` hunks undecided.
fn turn(
    workbench: &Entity<Workbench>,
    id: LaneId,
    event: LaneEvent,
    pending: usize,
    cx: &mut VisualTestContext,
) {
    let tally = Tally {
        pending,
        ..Tally::default()
    };
    lanes(workbench, cx).update(cx, |store, cx| {
        store.turn(id, event, tally);
        cx.notify();
    });
    draw(cx);
}

fn pending(n: usize) -> Tally {
    Tally {
        pending: n,
        ..Tally::default()
    }
}

/// The badges the rail drew: Agents' alone, as nothing else is badged.
fn badges(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(PanelId, RailBadge)> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).state().drawn().rail_badges()
    })
}

fn counts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> LaneCounts {
    lanes(workbench, cx).read_with(cx, |store, _| store.counts())
}

/// Runs `agent.show_panel` as an unbound key would.
fn show(cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(super::SHOW_COMMAND)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn click(selector: &str, cx: &mut VisualTestContext) {
    let selector: &'static str = selector.to_owned().leak();
    let at = cx.debug_bounds(selector).expect(selector).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

#[gpui::test]
fn the_rail_shows_agents_after_changes_with_a_count_of_working_and_pending_lanes(
    cx: &mut TestAppContext,
) {
    let dir = project();
    let (workbench, cx, _) = opened(cx, dir.path());
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    // In the top group, after Changes, the robot head.
    assert_eq!(
        DockPanel::ALL,
        [
            DockPanel::Explorer,
            DockPanel::Search,
            DockPanel::SourceControl,
            DockPanel::Agents,
            DockPanel::Chat,
        ]
    );
    let (ids, icons) = dock.read_with(cx, |dock, _| {
        let drawn = dock.state().drawn();
        (drawn.rail_ids(), drawn.rail_icons())
    });
    assert_eq!(ids[2..4], [DockPanel::SourceControl.id(), AGENTS]);
    assert_eq!(icons[3], Icon::Bot);
    assert_eq!(
        (icons.iter()).filter(|icon| **icon == Icon::Bot).count(),
        1,
        "the robot head is the Agents item's alone: {icons:?}"
    );
    // Lane 1, Accepted with nothing pending: no badge.
    assert_eq!(counts(&workbench, cx), LaneCounts::default());
    assert_eq!(badges(&workbench, cx), []);

    // Lane 1 Working, nothing pending yet, and lane 2 Abandoned with 3
    // hunks undecided: 2, a lane Working counted once.
    restore(&workbench, vec![external(2, LaneState::Working)], cx);
    turn(&workbench, chat_lane(), LaneEvent::TurnStarted, 0, cx);
    recount(&workbench, lane(2), pending(3), cx);
    let shown = counts(&workbench, cx);
    assert_eq!(
        shown,
        LaneCounts {
            working: 1,
            pending: 2
        },
        "LaneCounts::pending holds the Working lanes too"
    );
    let badge = RailBadge::count(2, BadgeTone::Agent, "2 agent lanes working or to review");
    assert_eq!(badges(&workbench, cx), [(AGENTS, badge)]);

    // Bottom-right of the robot head, inside its own button.
    let button = dock.read_with(cx, |dock, _| dock.rail_button_bounds(DockPanel::Agents));
    let button = button.expect("drawn");
    let drawn = cx.debug_bounds("badge").expect("drawn");
    let icon = cx
        .debug_bounds("rail-icon:dock-rail-3")
        .expect("Agents' icon");
    assert!(button.contains(&drawn.origin), "{drawn:?} in {button:?}");
    assert!(drawn.bottom() <= button.bottom());
    assert!(drawn.right() > icon.right() && drawn.right() < button.right());
    assert!(drawn.center().y > icon.center().y);
    assert!(drawn.center().x > icon.center().x);

    // Lane 1's turn ends with 2 pending: PendingReview, still counted.
    turn(&workbench, chat_lane(), LaneEvent::TurnEnded, 2, cx);
    let lane_1 = lanes(&workbench, cx).read_with(cx, |store, _| store.lane(chat_lane()).cloned());
    assert_eq!(lane_1.unwrap().state, LaneState::PendingReview);
    assert_eq!(badges(&workbench, cx)[0].1.count, Some(2));

    // Lane 2's hunks decided: Rejected, out of the count.
    recount(&workbench, lane(2), Tally::default(), cx);
    let badge = RailBadge::count(1, BadgeTone::Agent, "1 agent lane working or to review");
    assert_eq!(badges(&workbench, cx), [(AGENTS, badge)]);

    // Lane 1's too: none at zero.
    recount(&workbench, chat_lane(), Tally::default(), cx);
    assert_eq!(counts(&workbench, cx).pending, 0);
    assert_eq!(badges(&workbench, cx), []);
}

#[gpui::test]
fn the_panel_lists_each_lane_and_keeps_ended_ones_dimmed_until_cleared(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx, path) = opened(cx, dir.path());
    restore(
        &workbench,
        vec![
            external(2, LaneState::Working),
            external(3, LaneState::Accepted),
            external(4, LaneState::Rejected),
        ],
        cx,
    );
    turn(&workbench, chat_lane(), LaneEvent::TurnStarted, 0, cx);
    recount(&workbench, chat_lane(), pending(1), cx);
    let files = vec![(
        path.clone(),
        Counts {
            hunks: 3,
            pending: 2,
            stale: 1,
            ..Counts::default()
        },
    )];
    let tally = Tally {
        files,
        pending: 3,
        stale: 1,
        ..Tally::default()
    };
    recount(&workbench, lane(2), tally, cx);
    show(cx);
    let panel = panel(&workbench, cx);
    assert_eq!(
        workbench.read_with(cx, |workbench, cx| workbench.dock().read(cx).active()),
        DockPanel::Agents
    );

    let drawn = |cx: &mut VisualTestContext| {
        panel.read_with(cx, |panel, _| {
            (panel.test.rows.iter())
                .map(|row| {
                    (
                        row.number().to_string(),
                        row.summary.name.to_string(),
                        row.state().to_string(),
                        row.counts().to_string(),
                        row.dimmed,
                        row.actions.clone(),
                    )
                })
                .collect::<Vec<_>>()
        })
    };
    let row = |number: u32, name: &str, state: &str, counts: &str| {
        (
            format!("lane {number}"),
            name.to_owned(),
            state.to_owned(),
            counts.to_owned(),
        )
    };
    let expected = [
        (
            row(1, "Chat", "Working", "0 files · 1 pending · 0 conflicts"),
            false,
        ),
        (
            row(2, "agent 2", "Abandoned", "1 file · 3 pending · 1 conflict"),
            true,
        ),
        (
            row(
                3,
                "agent 3",
                "Accepted",
                "0 files · 0 pending · 0 conflicts",
            ),
            true,
        ),
        (
            row(
                4,
                "agent 4",
                "Rejected",
                "0 files · 0 pending · 0 conflicts",
            ),
            true,
        ),
    ];
    let shown = drawn(cx);
    assert_eq!(shown.len(), 4, "{shown:?}");
    for (shown, ((number, name, state, counts), dimmed)) in shown.iter().zip(&expected) {
        assert_eq!(
            (&shown.0, &shown.1, &shown.2, &shown.3, shown.4),
            (number, name, state, counts, *dimmed)
        );
    }
    // Lane 1 can be reviewed and stopped, never cleared; the ended lanes
    // cleared.
    let actions: Vec<_> = shown.iter().map(|row| row.5.clone()).collect();
    assert_eq!(
        actions,
        [
            vec![LaneAction::Review, LaneAction::Stop],
            vec![LaneAction::Clear],
            vec![LaneAction::Clear],
            vec![LaneAction::Clear],
        ]
    );
    for number in 1..=4 {
        assert!(
            cx.debug_bounds(format!("agents-row-{number}").leak())
                .is_some()
        );
    }
    assert!(cx.debug_bounds("agents-clear-1").is_none());

    // An Accepted lane clears at once, its number still spent; the
    // Rejected one stays listed, dimmed.
    click("agents-clear-3", cx);
    assert!(!cx.has_pending_prompt());
    let shown = drawn(cx);
    let numbers: Vec<_> = shown.iter().map(|row| row.0.as_str()).collect();
    assert_eq!(numbers, ["lane 1", "lane 2", "lane 4"]);
    assert!(shown[2].4, "lane 4 still dimmed");
    let next = lanes(&workbench, cx).read_with(cx, |store, _| store.lanes().next_number());
    assert_eq!(next, Some(5));
    click("agents-clear-4", cx);
    assert_eq!(drawn(cx).len(), 2);
}

/// `editor`'s text with a proposal of lane 2's agent, adopted into its
/// lane as an Apply's is.
fn propose_for_lane_2(
    workbench: &Entity<Workbench>,
    editor: &Entity<EditorView>,
    path: PathBuf,
    cx: &mut VisualTestContext,
) -> Rc<RefCell<ProposedChange>> {
    let origin = Origin::Ai {
        agent: AGENT.into(),
        session: "s2".into(),
    };
    let (proposal, buffer) = editor.update(cx, |editor, cx| {
        let snapshot = editor.model().buffer().snapshot();
        let proposal = ProposedChange::from_edits(&snapshot, [(3..4, "b")], origin);
        let proposal = Rc::new(RefCell::new(proposal));
        editor.set_proposals(vec![proposal.clone()], cx);
        (proposal, snapshot.id())
    });
    workbench.update(cx, |workbench, cx| {
        workbench.adopt_proposal(buffer, Some(path), proposal.clone(), cx)
    });
    draw(cx);
    proposal
}

fn statuses(
    proposal: &Rc<RefCell<ProposedChange>>,
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<HunkStatus> {
    editor.read_with(cx, |editor, _| {
        let snapshot = editor.model().buffer().snapshot();
        (proposal.borrow().hunks(&snapshot).into_iter())
            .map(|hunk| hunk.status)
            .collect()
    })
}

#[gpui::test]
fn clearing_an_abandoned_lane_with_hunks_pending_asks_then_rejects_them(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx, path) = opened(cx, dir.path());
    restore(&workbench, vec![external(2, LaneState::Working)], cx);
    let editor = workbench.read_with(cx, |workbench, cx| {
        (workbench.open_editors(cx).into_iter())
            .find(|editor| editor.read(cx).model().path() == Some(path.as_path()))
            .expect("open")
    });
    let proposal = propose_for_lane_2(&workbench, &editor, path, cx);
    let lane_2 = |cx: &mut VisualTestContext| {
        lanes(&workbench, cx).read_with(cx, |store, _| {
            store.lane(lane(2)).map(|lane| (lane.state, lane.pending))
        })
    };
    assert_eq!(lane_2(cx), Some((LaneState::Abandoned, 1)));
    show(cx);

    // Asked, and cancelled: nothing changes.
    click("agents-clear-2", cx);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert_eq!(lane_2(cx), Some((LaneState::Abandoned, 1)));
    assert_eq!(statuses(&proposal, &editor, cx), [HunkStatus::Pending]);

    // Asked again, and told to: its hunk rejected, the text untouched, and
    // the lane gone from the panel and the rail's count.
    click("agents-clear-2", cx);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Reject and Clear");
    draw(cx);
    assert_eq!(statuses(&proposal, &editor, cx), [HunkStatus::Rejected]);
    let text = editor.read_with(cx, |editor, _| {
        editor.model().buffer().snapshot().text().to_string()
    });
    assert_eq!(text, "fn a() {}\n");
    assert_eq!(lane_2(cx), None);
    let panel = panel(&workbench, cx);
    let rows = panel.read_with(cx, |panel, _| panel.test.rows.len());
    assert_eq!(rows, 1, "lane 1 alone");
    assert_eq!(badges(&workbench, cx), []);
}

#[gpui::test]
fn lane_1_is_never_cleared(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx, _) = opened(cx, dir.path());
    show(cx);
    let panel = panel(&workbench, cx);
    let row = panel.read_with(cx, |panel, _| panel.test.rows[0].clone());
    assert_eq!(row.summary.id, chat_lane());
    assert_eq!(row.summary.state, LaneState::Accepted);
    assert!(row.dimmed, "decided, so dimmed");
    assert_eq!(row.actions, []);
    for confirmed in [false, true] {
        workbench.update(cx, |workbench, cx| {
            workbench.clear_lane(chat_lane(), confirmed, cx)
        });
    }
    draw(cx);
    let kept = lanes(&workbench, cx).read_with(cx, |store, _| store.lane(chat_lane()).is_some());
    assert!(kept);
    assert_eq!(panel.read_with(cx, |panel, _| panel.test.rows.len()), 1);
}

#[gpui::test]
fn the_badge_and_the_lane_numbers_take_agent_base_not_the_accent_in_both_themes(
    cx: &mut TestAppContext,
) {
    let dir = project();
    let (workbench, cx, _) = opened(cx, dir.path());
    let theme = |title: &str| {
        let theme = fe_config::themes::find(title).expect("bundled");
        Theme::new(ThemeData::from_file(&theme.file()))
    };
    let (yohen, hakuji) = (theme("Yohen"), theme("Hakuji"));
    turn(&workbench, chat_lane(), LaneEvent::TurnStarted, 0, cx);
    show(cx);
    let panel = panel(&workbench, cx);
    for mode in [ThemeMode::Dark, ThemeMode::Light] {
        cx.update(|_, cx| set_themes(cx, mode, yohen.clone(), hakuji.clone()));
        draw(cx);
        let (agent, accent) = cx.update(|_, cx| {
            let c = &cx.theme().colors;
            (c.agent.base, c.accent.base)
        });
        let badge = badges(&workbench, cx);
        assert_eq!(badge.len(), 1, "{mode:?}: lane 1 Working");
        let (fill, _) = cx.update(|_, cx| badge[0].1.tone.colors(&cx.theme().colors));
        assert_eq!(fill, agent, "{mode:?}: the badge is agent.base");
        assert_ne!(fill, accent, "{mode:?}: never the accent");
        let numbers = panel.read_with(cx, |panel, _| panel.test.number_colors.clone());
        assert!(!numbers.is_empty());
        for number in numbers {
            assert_eq!(number, agent, "{mode:?}: a lane's number is agent.base");
            assert_ne!(number, accent, "{mode:?}");
        }
    }
    // The two themes' agents differ: each test read its own.
    let (dark, light) = (yohen.colors.agent.base, hakuji.colors.agent.base);
    assert_ne!(dark, light);
}

/// A folder holding `a.rs` and `b.rs`, and a workbench on it showing
/// both, `b.rs` active.
fn two_files<'a>(
    dir: &Path,
    cx: &'a mut TestAppContext,
) -> (Entity<Workbench>, &'a mut VisualTestContext, [PathBuf; 2]) {
    fs::write(dir.join("b.rs"), "fn b() {}\n").unwrap();
    let (workbench, cx, a) = opened(cx, dir);
    let b = dir.join("b.rs");
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.open(b.clone(), window, cx));
    });
    draw(cx);
    (workbench, cx, [a, b])
}

fn editor_of(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.open_editors(cx).into_iter())
            .find(|editor| editor.read(cx).model().path() == Some(path))
            .expect("open")
    })
}

/// A proposal of `origin`'s on `editor`'s text: its function renamed
/// `z`, a hunk at offset 3.
fn proposal(
    editor: &Entity<EditorView>,
    origin: Origin,
    cx: &mut VisualTestContext,
) -> (fe_buffer::BufferId, Rc<RefCell<ProposedChange>>) {
    editor.read_with(cx, |editor, _| {
        let snapshot = editor.model().buffer().snapshot();
        let proposal = ProposedChange::from_edits(&snapshot, [(3..4, "z")], origin);
        (snapshot.id(), Rc::new(RefCell::new(proposal)))
    })
}

/// Closes `path`'s tab, its buffer let go.
fn close(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let editor = editor_of(workbench, path, cx);
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        let at = (pane.read(cx).tabs().iter())
            .position(|tab| tab.editor() == Some(&editor))
            .expect("a tab");
        pane.update(cx, |pane, cx| {
            pane.activate(at, window, cx);
            pane.close_active(window, cx);
        });
    });
    draw(cx);
    let open = workbench.read_with(cx, |workbench, cx| {
        (workbench.open_editors(cx).iter())
            .any(|editor| editor.read(cx).model().path() == Some(path))
    });
    assert!(!open, "{path:?} closed");
}

fn undecided(proposal: &Rc<RefCell<ProposedChange>>) -> usize {
    let proposal = proposal.borrow();
    proposal.status_count(HunkStatus::Pending) + proposal.status_count(HunkStatus::Stale)
}

#[gpui::test]
fn clearing_rejects_the_hunks_of_editors_closed_since(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx, [a, b]) = two_files(dir.path(), cx);
    restore(&workbench, vec![external(2, LaneState::Working)], cx);
    let origin = Origin::Ai {
        agent: AGENT.into(),
        session: "s2".into(),
    };
    // A loose proposal on a.rs, as an Apply's, and a turn on b.rs kept in
    // the lane's work.
    let (a_buffer, loose) = proposal(&editor_of(&workbench, &a, cx), origin.clone(), cx);
    (editor_of(&workbench, &a, cx)).update(cx, |editor, cx| {
        editor.set_proposals(vec![loose.clone()], cx)
    });
    workbench.update(cx, |workbench, cx| {
        workbench.adopt_proposal(a_buffer, Some(a.clone()), loose.clone(), cx)
    });
    let (b_buffer, kept) = proposal(&editor_of(&workbench, &b, cx), origin.clone(), cx);
    let mut changeset = fe_editor::Changeset::new("turn-2", origin);
    changeset.add_file(b.clone(), b_buffer, kept.clone());
    workbench.update_in(cx, |workbench, window, cx| {
        LaneStore::set_changeset(workbench, lane(2), changeset, None, window, cx)
    });
    draw(cx);
    let lane_2 = |cx: &mut VisualTestContext| {
        lanes(&workbench, cx).read_with(cx, |store, _| {
            store.lane(lane(2)).map(|lane| (lane.state, lane.pending))
        })
    };
    assert_eq!(lane_2(cx), Some((LaneState::Abandoned, 2)));

    // Both editors closed: the hunks are still the lane's, undecided.
    close(&workbench, &a, cx);
    close(&workbench, &b, cx);
    workbench.update(cx, |workbench, cx| workbench.sync_lanes(cx));
    draw(cx);
    assert_eq!(lane_2(cx), Some((LaneState::Abandoned, 2)));
    assert_eq!((undecided(&loose), undecided(&kept)), (1, 1));

    show(cx);
    click("agents-clear-2", cx);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Reject and Clear");
    draw(cx);
    for proposal in [&loose, &kept] {
        let proposal = proposal.borrow();
        assert_eq!(proposal.status_count(HunkStatus::Rejected), 1);
        assert_eq!(proposal.status_count(HunkStatus::Pending), 0);
    }
    assert_eq!(lane_2(cx), None, "cleared once nothing was left undecided");
}

/// The active editor's file and its caret.
fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (PathBuf, usize) {
    workbench.read_with(cx, |workbench, cx| {
        let editor = workbench.active_editor(cx).expect("an editor").read(cx);
        let path = editor.model().path().expect("a file").to_owned();
        (path, editor.model().primary_selection().head())
    })
}

/// Makes `path`'s tab the active one.
fn activate(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let path = path.to_owned();
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.open(path, window, cx));
    });
    draw(cx);
}

/// Lane 1's work on `path`: a changeset of one file, the chat's.
fn chat_turn(
    workbench: &Entity<Workbench>,
    turn: &str,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Rc<RefCell<ProposedChange>> {
    let origin = chat_origin();
    let (buffer, proposal) = proposal(&editor_of(workbench, path, cx), origin.clone(), cx);
    let mut changeset = fe_editor::Changeset::new(turn, origin);
    changeset.add_file(path.to_owned(), buffer, proposal.clone());
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_changeset(changeset, window, cx)
    });
    draw(cx);
    proposal
}

fn chat_origin() -> Origin {
    Origin::Ai {
        agent: crate::chat_panel::AGENT.into(),
        session: "chat-1".into(),
    }
}

#[gpui::test]
fn review_reaches_lane_1s_undecided_work_wherever_it_is(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx, [a, b]) = two_files(dir.path(), cx);
    lanes(&workbench, cx).update(cx, |store, _| store.claim(chat_lane(), "chat-1".into()));
    show(cx);
    let dock_shows = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| workbench.dock().read(cx).active())
    };

    // An Apply's proposal on a.rs, b.rs active: Review shows a.rs, the
    // caret on the hunk, though the AI Changes panel has nothing.
    let (buffer, applied) = proposal(&editor_of(&workbench, &a, cx), chat_origin(), cx);
    (editor_of(&workbench, &a, cx)).update(cx, |editor, cx| {
        editor.set_proposals(vec![applied.clone()], cx)
    });
    workbench.update(cx, |workbench, cx| {
        workbench.adopt_proposal(buffer, Some(a.clone()), applied.clone(), cx)
    });
    draw(cx);
    let state = lanes(&workbench, cx).read_with(cx, |store, _| store.lane(chat_lane()).cloned());
    assert_eq!(state.map(|lane| lane.state), Some(LaneState::PendingReview));
    assert_eq!(active(&workbench, cx).0, b);
    click("agents-review-1", cx);
    assert_eq!(active(&workbench, cx), (a.clone(), 3));
    let editor = editor_of(&workbench, &a, cx);
    let focused = cx.update(|window, cx| editor.focus_handle(cx).contains_focused(window, cx));
    assert!(focused, "the editor takes focus");
    assert_eq!(
        dock_shows(cx),
        DockPanel::Agents,
        "nothing on the panel to show"
    );
    (editor_of(&workbench, &a, cx)).update(cx, |editor, cx| {
        let snapshot = editor.model().buffer().snapshot();
        let id = applied.borrow().hunks(&snapshot)[0].id;
        editor.reject_hunk(id, cx).unwrap();
    });
    draw(cx);

    // A turn on b.rs, then a newer one on a.rs, decided: the older turn,
    // kept in the lane's work, is what Review shows, in b.rs.
    let older = chat_turn(&workbench, "chat-1:0", &b, cx);
    let newer = chat_turn(&workbench, "chat-1:1", &a, cx);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_ai_changes_op(crate::ai_changes_panel::AiChangesOp::RejectAll, window, cx)
    });
    workbench.update(cx, |workbench, cx| workbench.sync_lanes(cx));
    assert_eq!(undecided(&newer), 0);
    show(cx);
    activate(&workbench, &a, cx);
    assert_eq!(undecided(&older), 1);
    click("agents-review-1", cx);
    assert_eq!(active(&workbench, cx), (b.clone(), 3));

    // A refactor preview takes the panel: it stays, and Review still
    // reaches the older turn.
    let source =
        editor_of(&workbench, &a, cx).read_with(cx, |editor, _| editor.model().buffer().id());
    let other = dir.path().join("c.rs");
    fs::write(&other, "fn c() {}\n").unwrap();
    let edit = lsp_types::TextEdit {
        range: lsp_types::Range::default(),
        new_text: "// c\n".into(),
    };
    let edits = vec![fe_lsp::FileEdits::closed(
        other,
        vec![edit],
        fe_lsp::PositionEncoding::Utf16,
    )];
    workbench
        .update_in(cx, |workbench, window, cx| {
            workbench.preview_file_edits("Rename", source, edits, window, cx)
        })
        .expect("previewed");
    draw(cx);
    let shown = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, _| {
            (workbench.ai_changes.as_ref()).map(|state| state.changeset.turn_id().to_owned())
        })
    };
    assert_eq!(shown(cx).as_deref(), Some("Rename"));
    show(cx);
    activate(&workbench, &a, cx);
    click("agents-review-1", cx);
    assert_eq!(active(&workbench, cx), (b, 3));
    assert_eq!(shown(cx).as_deref(), Some("Rename"), "the preview stays");
}

#[gpui::test]
fn stop_ends_lane_1s_turn_with_a_permission_prompt_waiting(cx: &mut TestAppContext) {
    use std::sync::Arc;

    use fe_chat::{ChatProvider, ReplyScript, ScriptedProvider, StopReason, TurnState};
    use kiln::core::secrets::MemoryStore;

    use crate::chat_panel::{ChatPanel, ChatStateDir, KeySource};

    // Text files only, so no language server starts once it's trusted.
    let dir = tempfile::tempdir_in(dunce::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
    fs::write(dir.path().join("notes.txt"), "plans\n").unwrap();
    let state = tempfile::tempdir().unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let input = serde_json::json!({"args": ["-c", "touch ran"], "cwd": null, "program": "sh"});
    let provider = Arc::new(ScriptedProvider::new([ReplyScript::new(
        "claude-opus-5-5",
        "msg_1",
    )
    .text("I'll run it.")
    .tool_use("call_1", "run_command", input)
    .stop(StopReason::ToolUse)]));
    cx.update(|_, cx| {
        cx.set_global(ChatStateDir(Some(state.path().to_owned())));
        cx.set_global(crate::chat_tool_loop::CommandPath("/usr/bin:/bin".into()));
        cx.set_global(KeySource {
            env: Some("key".to_owned()),
            store: Arc::new(MemoryStore::new()),
        });
        let provider = provider.clone();
        ChatPanel::set_provider_factory(
            cx,
            Rc::new(move |_, _| provider.clone() as Arc<dyn ChatProvider>),
        );
    });
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    let run = RunCommand(
        Invocation::of("chat.open")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let chat = workbench
        .read_with(cx, |workbench, cx| {
            workbench.dock().read(cx).chat().cloned()
        })
        .expect("made with the dock");
    let view = chat.read_with(cx, |chat, _| chat.view().clone());
    view.update(cx, |view, cx| view.set_composer_text("Make the file", cx));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            kiln::with_cx(window, cx, |cx| view.send_message(cx))
        })
    });
    draw(cx);

    // `run_command` asks first: its prompt waits, the turn running.
    let waiting = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            (view.is_streaming(), view.waiting_permissions())
        })
    };
    assert_eq!(waiting(cx), (true, vec![(0, "call_1".to_owned())]));
    let lane_1 = |cx: &mut VisualTestContext| {
        lanes(&workbench, cx).read_with(cx, |store, _| store.lane(chat_lane()).unwrap().state)
    };
    assert_eq!(lane_1(cx), LaneState::Working);

    // Stop, from the Agents panel: the prompt closed, the reply stopped,
    // the turn over, and lane 1 no longer Working.
    show(cx);
    let actions =
        panel(&workbench, cx).read_with(cx, |panel, _| panel.test.rows[0].actions.clone());
    assert!(actions.contains(&LaneAction::Stop), "{actions:?}");
    click("agents-stop-1", cx);
    assert_eq!(waiting(cx), (false, vec![]));
    let turn = view.read_with(cx, |view, _| view.conversation().turns[0].state.clone());
    assert_eq!(turn, TurnState::Stopped);
    assert_eq!(lane_1(cx), LaneState::Rejected, "it made nothing");
    assert!(!dir.path().join("ran").exists(), "the command never ran");
    assert_eq!(provider.requests().len(), 1, "nothing more was sent");
}
