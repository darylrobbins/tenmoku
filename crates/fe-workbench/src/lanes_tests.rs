//! Lane 1 through the chat ([`super`]): replies a [`ScriptedProvider`]'s,
//! Apply and Review Changes pressed on them, hunks decided in the editors
//! and the AI Changes panel, and the lane's state read from the
//! workbench's [`LaneStore`]; and the store's counts on their own.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::SystemTime,
};

use fe_buffer::{HunkStatus, Origin};
use fe_chat::{BlockKind, ChatEvent, ChatProvider, ScriptedProvider, StopReason};
use fe_chat_view::SendMessage;
use fe_command::Invocation;
use fe_editor::{
    Counts,
    lane::{Backing, Lane, LaneCounts, LaneEvent, LaneId, LaneKind, LaneState},
};
use fe_gpui::EditorView;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::core::secrets::MemoryStore;
use serde_json::json;

use super::{LaneStore, Tally, chat_lane};
use crate::{
    Launch, Workbench,
    ai_changes_panel::AiChangesOp,
    chat_panel::{ChatPanel, KeySource},
    tests::open,
};

const MAIN: &str = "func main() {\n    x := 1\n}\n";
const LIB: &str = "package main\n\nfunc a() int {\n    return 1\n}\n";

/// Blocks for both files: Review Changes shows for it.
const TWO: &str = "Both:\n\n```go path=src/main.go\nfunc main() {\n    x := 2\n}\n```\n\n\
```go path=src/lib.go\npackage main\n\nfunc a() int {\n    return 2\n}\n```\n";

/// Blocks for lib.go and a new file.
const LIB_AND_NEW: &str = "Then:\n\n```go path=src/lib.go\npackage main\n\nfunc a() int {\n    return 3\n}\n```\n\n\
```go path=src/new.go\npackage main\n```\n";

/// One block for main.go.
const ONE: &str = "```go path=src/main.go\nfunc main() {\n    x := 2\n}\n```\n";

fn reply(text: &str) -> Vec<ChatEvent> {
    vec![
        ChatEvent::Started {
            model: "claude-opus-5-5".to_owned(),
            message_id: "msg".to_owned(),
        },
        ChatEvent::BlockStarted {
            index: 0,
            kind: BlockKind::Text,
        },
        ChatEvent::TextDelta {
            index: 0,
            text: text.to_owned(),
        },
        ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "text", "text": text}),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        },
    ]
}

/// A reply cut off before it finished: it fails.
fn cut_off(text: &str) -> Vec<ChatEvent> {
    let mut events = reply(text);
    events.truncate(3);
    events
}

/// A folder holding `src/main.go` and `src/lib.go`, under the real temp
/// folder.
fn project() -> tempfile::TempDir {
    let temp = dunce::canonicalize(std::env::temp_dir()).unwrap();
    let dir = tempfile::tempdir_in(temp).unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.go"), MAIN).unwrap();
    fs::write(dir.path().join("src/lib.go"), LIB).unwrap();
    dir
}

fn run_store(cx: &mut VisualTestContext) {
    loop {
        cx.run_until_parked();
        let ran = cx.update(|_, cx| {
            let worker = cx.try_global::<crate::chat_panel::chat_store_worker::WorkerHandle>();
            worker.map_or(0, |handle| handle.0.run_pending())
        });
        if ran == 0 {
            break;
        }
    }
}

fn draw(cx: &mut VisualTestContext) {
    run_store(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    run_store(cx);
}

fn opened<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx)
}

/// The chat, open, over a provider that streams `replies` in turn, with
/// nothing sent yet.
fn chat_with(
    workbench: &Entity<Workbench>,
    replies: &[&str],
    cx: &mut VisualTestContext,
) -> Entity<ChatPanel> {
    let replies = replies.iter().map(|text| reply(text)).collect();
    chat_streaming(workbench, replies, cx)
}

/// The chat, open, over a provider that streams `replies` in turn, with
/// nothing sent yet.
fn chat_streaming(
    workbench: &Entity<Workbench>,
    replies: Vec<Vec<ChatEvent>>,
    cx: &mut VisualTestContext,
) -> Entity<ChatPanel> {
    let provider = Arc::new(ScriptedProvider::new(replies));
    cx.update(|_, cx| {
        ChatPanel::set_provider_factory(
            cx,
            Rc::new(move |_, _| provider.clone() as Arc<dyn ChatProvider>),
        );
        cx.set_global(KeySource {
            env: Some("key".to_owned()),
            store: Arc::new(MemoryStore::new()),
        });
    });
    let run = Invocation::of("chat.open").unwrap();
    cx.update(|window, cx| {
        kiln::platform::command_actions::dispatch_invocation(run, window, cx).unwrap()
    });
    draw(cx);
    workbench
        .read_with(cx, |workbench, cx| {
            workbench.dock().read(cx).chat().cloned()
        })
        .expect("made")
}

/// Shows the chat, in place of the AI Changes panel a Review showed.
fn show_chat(cx: &mut VisualTestContext) {
    let run = Invocation::of("chat.open").unwrap();
    cx.update(|window, cx| {
        kiln::platform::command_actions::dispatch_invocation(run, window, cx).unwrap()
    });
    draw(cx);
}

/// Sends a message, its reply streamed in to the end.
fn send(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) {
    show_chat(cx);
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |view, cx| view.set_composer_text("Change it", cx));
    let focus = panel.read_with(cx, gpui::Focusable::focus_handle);
    cx.update(|window, cx| window.focus(&focus, cx));
    draw(cx);
    cx.dispatch_action(SendMessage);
    draw(cx);
}

/// Presses a chat button, the chat shown first.
fn press(selector: String, cx: &mut VisualTestContext) {
    show_chat(cx);
    let button = cx.debug_bounds(selector.leak()).expect("drawn");
    cx.simulate_click(button.center(), Default::default());
    draw(cx);
}

/// Lane 1's states, as an observer of the store saw them.
struct Seen {
    last: LaneState,
    /// Those it went to since last asked, in order.
    since: Vec<LaneState>,
}

/// The lanes' store, and every state lane 1 goes to from now on, in order.
fn watch(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Entity<LaneStore>, Rc<RefCell<Seen>>) {
    let lanes = workbench.read_with(cx, |workbench, _| workbench.lanes.clone());
    let state = lanes.read_with(cx, |lanes, _| lanes.lane(chat_lane()).unwrap().state);
    let seen = Rc::new(RefCell::new(Seen {
        last: state,
        since: Vec::new(),
    }));
    let subscription = cx.update(|_, cx| {
        let seen = seen.clone();
        cx.observe(&lanes, move |lanes, cx| {
            let state = lanes.read(cx).lane(chat_lane()).unwrap().state;
            let mut seen = seen.borrow_mut();
            if seen.last != state {
                seen.last = state;
                seen.since.push(state);
            }
        })
    });
    subscription.detach();
    (lanes, seen)
}

/// Lane 1's state, and the states it went through since last asked.
fn moved(
    lanes: &Entity<LaneStore>,
    seen: &Rc<RefCell<Seen>>,
    cx: &mut VisualTestContext,
) -> (LaneState, Vec<LaneState>) {
    let state = lanes.read_with(cx, |lanes, _| lanes.lane(chat_lane()).unwrap().state);
    (state, std::mem::take(&mut seen.borrow_mut().since))
}

fn pending(lanes: &Entity<LaneStore>, cx: &mut VisualTestContext) -> usize {
    lanes.read_with(cx, |lanes, _| lanes.lane(chat_lane()).unwrap().pending)
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

/// Accepts or rejects every pending hunk `editor` shows, by its own
/// buttons' calls.
fn decide_in(editor: &Entity<EditorView>, accept: bool, cx: &mut VisualTestContext) {
    let ids: Vec<_> = editor.read_with(cx, |editor, _| {
        let snapshot = editor.model().buffer().snapshot();
        (editor.proposals().iter())
            .flat_map(|proposal| proposal.borrow().hunks(&snapshot))
            .filter(|hunk| matches!(hunk.status, HunkStatus::Pending | HunkStatus::Stale))
            .map(|hunk| hunk.id)
            .collect()
    });
    for id in ids {
        editor
            .update(cx, |editor, cx| match accept {
                true => editor.accept_hunk(id, cx).map(drop),
                false => editor.reject_hunk(id, cx),
            })
            .ok();
    }
    draw(cx);
}

/// Types `text` over `range` of `editor`'s text, as the user.
fn edit(
    editor: &Entity<EditorView>,
    range: std::ops::Range<usize>,
    text: &str,
    cx: &mut VisualTestContext,
) {
    editor.update(cx, |editor, cx| {
        let mut buffer = editor.model_mut().buffer_mut();
        buffer.edit([(range, text)], Origin::User);
        drop(buffer);
        cx.notify();
    });
    draw(cx);
}

/// Has the workbench count its lanes again, as any listing does.
fn sync(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    workbench.update(cx, |workbench, cx| workbench.sync_lanes(cx));
    draw(cx);
}

fn panel_op(workbench: &Entity<Workbench>, op: AiChangesOp, cx: &mut VisualTestContext) {
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_ai_changes_op(op, window, cx)
    });
    draw(cx);
}

fn chat_id(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> String {
    panel.read_with(cx, |panel, cx| {
        panel.view().read(cx).conversation().id.to_string()
    })
}

#[test]
fn lane_1_is_the_chat_made_first() {
    let store = LaneStore::new();
    let lane = store.lane(chat_lane()).expect("made");
    assert_eq!(lane.kind, LaneKind::Chat);
    assert_eq!(lane.backing, Backing::Shared);
    assert_eq!(&*lane.agent, "fastedit.chat");
    assert_eq!(&*lane.name, "Chat");
    assert_eq!(lane.state, LaneState::Accepted, "nothing pending");
    assert_eq!(store.lanes().len(), 1);
    assert_eq!(store.lanes().next_number(), Some(2));
    assert_eq!(store.counts(), LaneCounts::default());
}

fn external(id: u32, state: LaneState) -> Lane {
    Lane {
        id: LaneId::from_number(id).unwrap(),
        name: "claude-code".into(),
        agent: "acp.claude-agent".into(),
        sessions: vec![format!("s{id}").into()],
        kind: LaneKind::External { host: "acp".into() },
        backing: Backing::Worktree,
        state,
        started: SystemTime::UNIX_EPOCH,
        revision: None,
        pending: 3,
    }
}

#[test]
fn restored_external_lanes_with_none_pending_count_nothing() {
    let store = LaneStore::restore(
        vec![
            external(2, LaneState::Working),
            external(3, LaneState::PendingReview),
        ],
        Some(4),
    );
    for id in [2, 3] {
        let lane = store.lane(LaneId::from_number(id).unwrap()).unwrap();
        assert_eq!(
            lane.state,
            LaneState::Abandoned,
            "lane {id}: its agent is gone"
        );
        assert_eq!(lane.pending, 0, "lane {id}");
    }
    assert_eq!(store.counts(), LaneCounts::default());
    assert_eq!(store.rail_count(), 0);
    let chat = store.lane(chat_lane()).expect("lane 1 made");
    assert_eq!(chat.kind, LaneKind::Chat);
}

#[test]
fn a_restored_record_gives_1_to_the_chat_only() {
    let store = LaneStore::restore(vec![external(1, LaneState::Accepted)], Some(2));
    let lane = store.lane(chat_lane()).unwrap();
    assert_eq!(lane.kind, LaneKind::Chat);
    assert_eq!(store.lanes().len(), 1);
    assert_eq!(store.lanes().next_number(), Some(2));
}

#[test]
fn a_working_lane_with_pending_hunks_counts_once_in_the_rail() {
    let mut store = LaneStore::new();
    store.turn(chat_lane(), LaneEvent::TurnStarted, Tally::default());
    let tally = Tally {
        pending: 2,
        ..Tally::default()
    };
    store.recount(chat_lane(), tally);
    let lane = store.lane(chat_lane()).unwrap();
    assert_eq!((lane.state, lane.pending), (LaneState::Working, 2));
    assert_eq!(
        store.counts(),
        LaneCounts {
            working: 1,
            pending: 1
        }
    );
    assert_eq!(store.rail_count(), 1, "not 2");
}

#[gpui::test]
fn lane_1_moves_through_its_rows_with_the_chat(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, dir.path());
    let panel = chat_with(&workbench, &[TWO, "Done.", "Done again."], cx);
    let (lanes, seen) = watch(&workbench, cx);
    assert_eq!(moved(&lanes, &seen, cx).0, LaneState::Accepted, "as made");

    // The first send starts a turn; one that made nothing ends Rejected.
    send(&panel, cx);
    use LaneState::*;
    assert_eq!(
        moved(&lanes, &seen, cx),
        (Rejected, vec![Working, Rejected])
    );
    let chat = chat_id(&panel, cx);
    let claimed = lanes.read_with(cx, |lanes, _| {
        lanes.lane(chat_lane()).unwrap().sessions.clone()
    });
    assert!(claimed.is_empty(), "nothing proposed yet");

    // Review Changes on the finished reply: hunks added outside a turn.
    press("chat-review-changes-0".into(), cx);
    assert_eq!(
        moved(&lanes, &seen, cx),
        (PendingReview, vec![PendingReview])
    );
    assert_eq!(pending(&lanes, cx), 2);
    let claimed = lanes.read_with(cx, |lanes, _| {
        lanes.lane(chat_lane()).unwrap().sessions.clone()
    });
    assert_eq!(claimed, [Arc::<str>::from(chat.as_str())]);

    // The next send, and its end with hunks still pending.
    send(&panel, cx);
    assert_eq!(
        moved(&lanes, &seen, cx),
        (PendingReview, vec![Working, PendingReview])
    );
    assert_eq!(pending(&lanes, cx), 2);

    // The last pending hunk decided, one accepted.
    panel_op(&workbench, AiChangesOp::AcceptAll, cx);
    assert_eq!(moved(&lanes, &seen, cx), (Accepted, vec![Accepted]));
    assert_eq!(pending(&lanes, cx), 0);

    // A persistent lane starts another turn, which ends with none pending.
    send(&panel, cx);
    assert_eq!(
        moved(&lanes, &seen, cx),
        (Accepted, vec![Working, Accepted])
    );
    assert_eq!(
        lanes.read_with(cx, |lanes, _| lanes.counts()),
        LaneCounts::default()
    );
}

#[gpui::test]
fn the_last_hunk_rejected_leaves_lane_1_rejected(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, dir.path());
    let panel = chat_with(&workbench, &[TWO], cx);
    let (lanes, seen) = watch(&workbench, cx);
    send(&panel, cx);
    press("chat-review-changes-0".into(), cx);
    use LaneState::*;
    assert_eq!(moved(&lanes, &seen, cx).0, PendingReview);
    panel_op(&workbench, AiChangesOp::RejectAll, cx);
    assert_eq!(moved(&lanes, &seen, cx), (Rejected, vec![Rejected]));
}

#[gpui::test]
fn apply_after_a_finished_reply_goes_back_to_pending_review(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let panel = chat_with(&workbench, &[ONE], cx);
    let (lanes, seen) = watch(&workbench, cx);
    send(&panel, cx);
    use LaneState::*;
    assert_eq!(moved(&lanes, &seen, cx).0, Rejected, "the reply's over");

    press("chat-apply-0-0".into(), cx);
    assert_eq!(
        moved(&lanes, &seen, cx),
        (PendingReview, vec![PendingReview])
    );
    assert_eq!(pending(&lanes, cx), 1, "Apply's proposal is lane 1's");
    assert_eq!(lanes.read_with(cx, |lanes, _| lanes.rail_count()), 1);
    let main = root.join("src/main.go");
    let files = lanes.read_with(cx, |lanes, _| lanes.summaries()[0].files.clone());
    assert_eq!(
        files
            .iter()
            .map(|(path, counts)| (path.clone(), counts.pending))
            .collect::<Vec<_>>(),
        [(main.clone(), 1)]
    );

    // Decided in its editor: a ProposalEvent, recounting the lane.
    decide_in(&editor_of(&workbench, &main, cx), true, cx);
    assert_eq!(moved(&lanes, &seen, cx), (Accepted, vec![Accepted]));
    assert_eq!(pending(&lanes, cx), 0);
    assert!(
        !lanes.read_with(cx, |lanes, _| lanes.has_work()),
        "a decided proposal leaves the lane's work"
    );
}

#[gpui::test]
fn an_older_turn_stays_pending_after_a_newer_review(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let panel = chat_with(&workbench, &[TWO, LIB_AND_NEW], cx);
    let (lanes, seen) = watch(&workbench, cx);
    let chat = chat_id(&panel, cx);
    send(&panel, cx);
    press("chat-review-changes-0".into(), cx);
    send(&panel, cx);
    press("chat-review-changes-1".into(), cx);
    use LaneState::*;
    assert_eq!(moved(&lanes, &seen, cx).0, PendingReview);
    let shown = workbench.read_with(cx, |workbench, _| {
        (workbench.ai_changes.as_ref()).map(|state| state.changeset.turn_id().to_owned())
    });
    assert_eq!(
        shown,
        Some(format!("{chat}:1")),
        "the newer turn takes the panel"
    );
    // Turn 0's two hunks, turn 1's lib.go hunk and new file.
    assert_eq!(pending(&lanes, cx), 4);

    // The newer turn decided: the older one is still pending, still
    // counted.
    panel_op(&workbench, AiChangesOp::RejectAll, cx);
    assert_eq!(moved(&lanes, &seen, cx), (PendingReview, vec![]));
    assert_eq!(pending(&lanes, cx), 2);
    assert_eq!(lanes.read_with(cx, |lanes, _| lanes.rail_count()), 1);
    let files = lanes.read_with(cx, |lanes, _| lanes.summaries()[0].files.clone());
    let pending_files: Vec<_> = (files.iter())
        .filter(|(_, counts)| counts.pending > 0)
        .map(|(path, _)| path.clone())
        .collect();
    assert_eq!(
        pending_files,
        [root.join("src/main.go"), root.join("src/lib.go")]
    );

    // Its hunks decided in their editors resolve it, and the lane.
    for file in ["src/main.go", "src/lib.go"] {
        decide_in(&editor_of(&workbench, &root.join(file), cx), true, cx);
    }
    assert_eq!(moved(&lanes, &seen, cx), (Accepted, vec![Accepted]));
    assert_eq!(pending(&lanes, cx), 0);
    assert!(
        !lanes.read_with(cx, |lanes, _| lanes.has_work()),
        "the older turn left"
    );
    // What it wrote stays attributed after it left, through later counts.
    sync(&workbench, cx);
    let written = lanes.read_with(cx, |lanes, _| lanes.written_files());
    assert_eq!(written.len(), 2, "{written:?}");
    for file in ["src/main.go", "src/lib.go"] {
        assert_eq!(
            written.get(&root.join(file)).map(Vec::as_slice),
            Some(&[chat_lane()][..])
        );
    }
}

#[test]
fn lane_1_restored_is_decided_by_its_hunks() {
    let saved = |state| {
        let mut lane = LaneStore::new().lane(chat_lane()).unwrap().clone();
        lane.state = state;
        LaneStore::restore(vec![lane], Some(2))
    };
    // Nothing is pending before its work is counted.
    use LaneState::*;
    for (state, restored) in [
        (Working, Rejected),
        (PendingReview, Rejected),
        (Accepted, Accepted),
        (Rejected, Rejected),
    ] {
        let store = saved(state);
        assert_eq!(
            store.lane(chat_lane()).unwrap().state,
            restored,
            "saved {state:?}"
        );
    }
}

#[test]
fn files_changing_under_an_unchanged_total_are_told() {
    let mut store = LaneStore::new();
    let counted = |path: &str| Tally {
        files: vec![(
            PathBuf::from(path),
            Counts {
                hunks: 1,
                pending: 1,
                ..Counts::default()
            },
        )],
        pending: 1,
        ..Tally::default()
    };
    assert!(store.recount(chat_lane(), counted("a.go")), "hunks added");
    assert!(!store.recount(chat_lane(), counted("a.go")), "nothing new");
    assert!(store.recount(chat_lane(), counted("b.go")), "another file");
    let stale = Tally {
        stale: 1,
        ..counted("b.go")
    };
    assert!(store.recount(chat_lane(), stale), "a conflict");
    assert_eq!(store.summaries()[0].conflicts, 1);
}

#[gpui::test]
fn a_failed_reply_ends_the_turn_and_a_rejected_lane_starts_another(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, dir.path());
    let panel = chat_streaming(&workbench, vec![cut_off("Halfway"), reply("Done.")], cx);
    let (lanes, seen) = watch(&workbench, cx);
    use LaneState::*;
    send(&panel, cx);
    assert_eq!(
        moved(&lanes, &seen, cx),
        (Rejected, vec![Working, Rejected])
    );
    // Rejected, then the chat's next send.
    send(&panel, cx);
    assert_eq!(
        moved(&lanes, &seen, cx),
        (Rejected, vec![Working, Rejected])
    );
}

#[gpui::test]
fn a_reverted_turn_rejected_leaves_lane_1_rejected(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, dir.path());
    let panel = chat_with(&workbench, &[TWO], cx);
    let (lanes, seen) = watch(&workbench, cx);
    send(&panel, cx);
    press("chat-review-changes-0".into(), cx);
    panel_op(&workbench, AiChangesOp::AcceptAll, cx);
    use LaneState::*;
    assert_eq!(moved(&lanes, &seen, cx).0, Accepted);
    // Reverting makes its hunks pending again, and no longer accepted.
    panel_op(&workbench, AiChangesOp::Revert, cx);
    assert_eq!(
        moved(&lanes, &seen, cx),
        (PendingReview, vec![PendingReview])
    );
    assert_eq!(pending(&lanes, cx), 2);
    panel_op(&workbench, AiChangesOp::RejectAll, cx);
    assert_eq!(moved(&lanes, &seen, cx), (Rejected, vec![Rejected]));
    assert!(
        lanes
            .read_with(cx, |lanes, _| lanes.written_files())
            .is_empty()
    );
}

#[gpui::test]
fn an_accepted_apply_stays_written_after_it_leaves(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let panel = chat_with(&workbench, &[ONE], cx);
    let (lanes, _) = watch(&workbench, cx);
    send(&panel, cx);
    press("chat-apply-0-0".into(), cx);
    let main = root.join("src/main.go");
    decide_in(&editor_of(&workbench, &main, cx), true, cx);
    sync(&workbench, cx);
    let written = lanes.read_with(cx, |lanes, _| lanes.written_files());
    assert_eq!(
        written.get(&main).map(Vec::as_slice),
        Some(&[chat_lane()][..])
    );
    assert_eq!(written.len(), 1);
}

#[gpui::test]
fn a_hunk_going_stale_tells_the_stores_observers(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let panel = chat_with(&workbench, &[TWO], cx);
    send(&panel, cx);
    press("chat-review-changes-0".into(), cx);
    let lanes = workbench.read_with(cx, |workbench, _| workbench.lanes.clone());
    let told = Rc::new(RefCell::new(0));
    let subscription = cx.update(|_, cx| {
        let told = told.clone();
        cx.observe(&lanes, move |_, _| *told.borrow_mut() += 1)
    });
    subscription.detach();
    let before = pending(&lanes, cx);
    // Typed over the text main.go's hunk replaces: stale, still pending.
    let main = editor_of(&workbench, &root.join("src/main.go"), cx);
    let at = MAIN.find("x := 1").unwrap();
    edit(&main, at..at + 6, "y := 9", cx);
    assert_eq!(
        pending(&lanes, cx),
        before,
        "a stale hunk is still to decide"
    );
    assert_eq!(
        lanes.read_with(cx, |lanes, _| lanes.summaries()[0].conflicts),
        1
    );
    assert!(*told.borrow() > 0, "observers told of the conflict");
}

#[gpui::test]
fn a_turn_accepted_then_reviewed_again_stays_written(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let panel = chat_with(&workbench, &[TWO], cx);
    let (lanes, seen) = watch(&workbench, cx);
    send(&panel, cx);
    press("chat-review-changes-0".into(), cx);
    panel_op(&workbench, AiChangesOp::AcceptAll, cx);
    // Its blocks change nothing now: the panel's turn just goes.
    press("chat-review-changes-0".into(), cx);
    sync(&workbench, cx);
    use LaneState::*;
    assert_eq!(moved(&lanes, &seen, cx).0, Accepted);
    let written = lanes.read_with(cx, |lanes, _| lanes.written_files());
    for file in ["src/main.go", "src/lib.go"] {
        assert_eq!(
            written.get(&root.join(file)).map(Vec::as_slice),
            Some(&[chat_lane()][..]),
            "{file}"
        );
    }
}

#[gpui::test]
fn a_turn_partly_accepted_then_reviewed_again_ends_accepted(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let panel = chat_with(&workbench, &[TWO], cx);
    let (lanes, seen) = watch(&workbench, cx);
    send(&panel, cx);
    press("chat-review-changes-0".into(), cx);
    let main = root.join("src/main.go");
    decide_in(&editor_of(&workbench, &main, cx), true, cx);
    // Again: main.go's block changes nothing now, lib.go's is proposed anew.
    press("chat-review-changes-0".into(), cx);
    use LaneState::*;
    assert_eq!(moved(&lanes, &seen, cx).0, PendingReview);
    assert_eq!(pending(&lanes, cx), 1);
    panel_op(&workbench, AiChangesOp::RejectAll, cx);
    assert_eq!(
        moved(&lanes, &seen, cx).0,
        Accepted,
        "main.go's accept still counts"
    );
    let written = lanes.read_with(cx, |lanes, _| lanes.written_files());
    assert_eq!(
        written.get(&main).map(Vec::as_slice),
        Some(&[chat_lane()][..])
    );
    assert!(!written.contains_key(&root.join("src/lib.go")));
}
