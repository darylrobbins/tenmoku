//! Agent toasts: a lane going `PendingReview` says it is done with files to
//! review, its icon and Review changes button in the agent's colour.

use std::{cell::RefCell, fs, path::Path, rc::Rc, time::SystemTime};

use fe_buffer::{Origin, ProposedChange};
use fe_config::themes;
use fe_editor::lane::{Backing, Lane, LaneEvent, LaneId, LaneKind, LaneState};
use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use fe_ui::{Theme, ThemeData};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{action_tint, notification_inbox::NotificationSource};
use crate::{Launch, Workbench, lanes::LaneStore, workbench::notification_inbox_view::source_icon};

fn lane_2() -> LaneId {
    LaneId::from_number(2).unwrap()
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A workbench over three files, with lane 2, `claude-code`, Working.
fn open<'a>(
    dir: &Path,
    cx: &'a mut TestAppContext,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    for file in ["a.rs", "b.rs", "c.rs"] {
        fs::write(dir.join(file), "fn one() {}\nfn two() {}\n").unwrap();
    }
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    workbench.update(cx, |workbench, cx| {
        workbench.lanes.update(cx, |store, cx| {
            let lane = Lane {
                id: lane_2(),
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
        // Restoring leaves an agent's lane ended: it starts a turn.
        workbench.lane_turn(lane_2(), LaneEvent::TurnStarted, cx);
    });
    (workbench, cx)
}

/// Lane 2 proposes a change to `file`, as Apply's is.
fn propose(workbench: &Entity<Workbench>, dir: &Path, file: &str, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = dir.join(file);
    let open = path.clone();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(open, window, cx)));
    draw(cx);
    let editor: Entity<EditorView> = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor");
    let origin = Origin::Ai {
        agent: "claude-code".into(),
        session: "s2".into(),
    };
    let (buffer, proposal) = editor.update(cx, |editor, cx| {
        let edits = vec![(0..2, "FN".to_owned())];
        let proposal = ProposedChange::from_edits(editor.model().snapshot(), edits, origin);
        let proposal = Rc::new(RefCell::new(proposal));
        let mut shown = editor.proposals().to_vec();
        shown.push(proposal.clone());
        editor.set_proposals(shown, cx);
        (editor.model().buffer().id(), proposal)
    });
    workbench.update(cx, |workbench, cx| {
        workbench.adopt_proposal(buffer, Some(path), proposal, cx)
    });
    draw(cx);
}

/// The toasts shown, in the active window: each with its message, action
/// label and command, and its Later button.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<super::Toast> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        layer
            .queue()
            .shown()
            .iter()
            .map(|e| e.toast.clone())
            .collect()
    })
}

#[gpui::test]
fn a_lane_going_pending_review_says_it_is_done_with_files_to_review(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = open(dir.path(), cx);
    for file in ["a.rs", "b.rs", "c.rs"] {
        propose(&workbench, dir.path(), file, cx);
    }
    // Still working: nothing yet.
    assert_eq!(shown(&workbench, cx), []);
    workbench.update(cx, |workbench, cx| {
        workbench.lane_turn(lane_2(), LaneEvent::TurnEnded, cx)
    });
    draw(cx);
    let state = workbench.read_with(cx, |w, cx| w.lanes.read(cx).lane(lane_2()).unwrap().state);
    assert_eq!(state, LaneState::PendingReview);
    let toasts = shown(&workbench, cx);
    let [toast] = toasts.as_slice() else {
        panic!("one toast: {toasts:?}");
    };
    assert_eq!(
        toast.message,
        "lane 2 · claude-code is done: 3 files to review"
    );
    assert_eq!(toast.source, Some(NotificationSource::Agent));
    let (label, command) = toast.action.clone().expect("an action");
    assert_eq!(
        (label.as_str(), command.as_str()),
        ("Review changes", "agent.review_lane")
    );
    assert_eq!(toast.action_args, Some(serde_json::json!({"lane": 2})));
    let choices = toast.choices.as_ref().expect("a question");
    assert_eq!(choices.titles(), ["Later"]);

    // Recounting with the lane still pending review says nothing again.
    workbench.update(cx, |workbench, cx| workbench.sync_lanes(cx));
    draw(cx);
    assert_eq!(shown(&workbench, cx).len(), 1);
}

#[gpui::test]
fn an_agent_toast_takes_the_agent_token_not_the_accent_in_both_themes(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (_workbench, cx) = open(dir.path(), cx);
    let mut agents = Vec::new();
    for name in [themes::DEFAULT_DARK, themes::DEFAULT_LIGHT] {
        let data = ThemeData::from_file(&themes::find(name).unwrap().file());
        cx.update(|window, cx| {
            fe_ui::set_theme(cx, Theme::new(data));
            window.refresh();
        });
        let colors = cx.update(|_, cx| fe_ui::ActiveTheme::theme(cx).colors);
        let source = Some(NotificationSource::Agent);
        let (_, icon) = source_icon(NotificationSource::Agent, super::Level::Info, &colors);
        let action = action_tint(source, &colors).expect("tinted");
        assert_eq!(icon, colors.agent.base, "{name}");
        assert_eq!(action, colors.agent.base, "{name}");
        assert_ne!(icon, colors.accent.base, "{name}");
        assert_ne!(action, colors.accent.base, "{name}");
        // Another toast's button stays plain.
        assert_eq!(action_tint(None, &colors), None, "{name}");
        agents.push(colors.agent.base);
    }
    assert_ne!(agents[0], agents[1], "each theme's own");
}
