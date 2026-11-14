//! The tool loop in a headless window (the agents record's § 12, "Trust
//! withdrawn"): a trusted temporary folder of `.txt` files (so no language
//! server starts), the Chat panel with a [`ScriptedProvider`] that plays a
//! reply calling tools, and the folder's trust withdrawn while a call waits
//! for permission, while a read is in flight and while a command runs.
//! Each ends the turn, kills or cancels the work and sends nothing; the
//! next request holds the `is_error` results, the user's text and the
//! `tool_removal` message last.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use fe_chat::{
    ChatEvent, ChatProvider, ChatRequest, ChatSettings, Conversation, NotRun, PROPOSE_EDIT,
    READ_FILE, RUN_COMMAND, ReplyScript, Role, ScriptedProvider, StopReason, ToolResult, TurnState,
    tool_set,
};
use fe_chat_anthropic::ApiKey;
use fe_chat_view::{ChatOptions, ChatView, PermissionAnswer, ToolStatus};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::SettingsStore;
use fe_gpui::commands::RunCommand;
use gpui::{AppContext as _, Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    core::secrets::MemoryStore,
    platform::config::{BatchSender, batch_channel},
};
use serde_json::{Value, json};

use super::{CommandPath, Held};
use crate::{
    Launch, Workbench,
    agent_audit::{AuditLog, file_for},
    chat_panel::{ChatPanel, ChatStateDir, KeySource},
    tests::open,
};

/// What every call of the withdrawn round is answered with.
const WITHDRAWN: &str = "Folder trust was withdrawn";

/// A workbench on `root`, focused as the app focuses it.
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

/// Lets the work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Runs workbench command `id`.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let stack = ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT));
    let run = RunCommand(Invocation::of(id).unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// A reply that says `text` and calls `name` with `input` as `call_1`.
fn calling(text: &str, name: &str, input: Value) -> Vec<ChatEvent> {
    ReplyScript::new("claude-opus-5-5", "msg_1")
        .text(text)
        .tool_use("call_1", name, input)
        .stop(StopReason::ToolUse)
}

/// A reply that says `text` and ends the turn.
fn saying(text: &str) -> Vec<ChatEvent> {
    ReplyScript::new("claude-opus-5-5", "msg_2")
        .text(text)
        .stop(StopReason::EndTurn)
}

/// The chat's panel in a trusted `root`, its audit trail in `state`, its
/// provider `provider`, its commands resolved on the system's `PATH`.
fn ready<'a>(
    root: &Path,
    state: &Path,
    provider: &Arc<ScriptedProvider>,
    cx: &'a mut TestAppContext,
) -> (
    Entity<Workbench>,
    Entity<ChatPanel>,
    &'a mut VisualTestContext,
) {
    let (workbench, cx) = opened(cx, root);
    let provider = provider.clone();
    cx.update(|_, cx| {
        cx.set_global(ChatStateDir(Some(state.to_owned())));
        cx.set_global(CommandPath(OsString::from("/usr/bin:/bin")));
        cx.set_global(KeySource {
            env: Some("key".to_owned()),
            store: Arc::new(MemoryStore::new()),
        });
        ChatPanel::set_provider_factory(
            cx,
            Rc::new(move |_: &ApiKey, _| provider.clone() as Arc<dyn ChatProvider>),
        );
    });
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    dispatch("chat.open", cx);
    let panel = workbench
        .read_with(cx, |workbench, cx| {
            workbench.dock().read(cx).chat().cloned()
        })
        .expect("made with the dock");
    (workbench, panel, cx)
}

/// Sends `question` from the composer.
fn ask(panel: &Entity<ChatPanel>, question: &str, cx: &mut VisualTestContext) {
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |view, cx| view.set_composer_text(question, cx));
    // As Send does, wherever focus went (the AI Changes panel takes it
    // once a turn's proposals are handed in).
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            kiln::with_cx(window, cx, |cx| view.send_message(cx))
        })
    });
    draw(cx);
}

fn shown(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> Conversation {
    panel.read_with(cx, |panel, cx| panel.view().read(cx).conversation().clone())
}

/// The status of call `call_1`'s row in turn 0.
fn status(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> Option<ToolStatus> {
    row(panel, 0, "call_1", cx).map(|row| row.status)
}

/// Call `id`'s row in turn `turn`.
fn row(
    panel: &Entity<ChatPanel>,
    turn: usize,
    id: &str,
    cx: &mut VisualTestContext,
) -> Option<fe_chat_view::ToolRow> {
    panel.read_with(cx, |panel, cx| {
        let view = panel.view().read(cx);
        (view.tool_rows(turn).iter())
            .find(|row| row.id == id)
            .cloned()
    })
}

/// Answers the prompt about call `id` of turn `turn`.
fn answer(
    panel: &Entity<ChatPanel>,
    turn: usize,
    id: &str,
    answer: PermissionAnswer,
    cx: &mut VisualTestContext,
) {
    let answered = panel.update(cx, |panel, cx| {
        (panel.view()).update(cx, |view, cx| view.answer_permission(turn, id, answer, cx))
    });
    assert!(answered, "a prompt waited about {id}");
    draw(cx);
}

/// kiln's settings service over a user settings file holding `text`.
fn settings(dir: &Path, text: &str, cx: &mut TestAppContext) -> BatchSender {
    let path = dir.join(fe_config::paths::SETTINGS_FILE);
    fs::write(&path, text).unwrap();
    let store = SettingsStore::load(Some(path), None);
    let (sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .settings_store(store.into_inner())
        .watch_with(batches);
    cx.update(|cx| assert_eq!(kiln::init(cx, config), []));
    cx.run_until_parked();
    sender
}

/// A reply that calls each of `calls`, `(id, tool, input)`.
fn calling_all(calls: &[(&str, &str, Value)]) -> Vec<ChatEvent> {
    let mut script = ReplyScript::new("claude-opus-5-5", "msg_1").text("On it.");
    for (id, name, input) in calls {
        script = script.tool_use(id, name, input.clone());
    }
    script.stop(StopReason::ToolUse)
}

/// A `propose_edit` input replacing `old` with `new` in `path`.
fn edit(path: &str, old: &str, new: &str) -> Value {
    json!({"edits": [{"new_text": new, "old_text": old}], "path": path})
}

/// A `run_command` input running `script` in `sh`.
fn shell(script: &str) -> Value {
    json!({"args": ["-c", script], "cwd": null, "program": "sh"})
}

/// The turn id of the changeset the AI Changes panel shows.
fn shown_review(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    workbench.read_with(cx, |workbench, _| {
        (workbench.ai_changes.as_ref()).map(|changes| changes.changeset.turn_id().to_owned())
    })
}

/// Waits until the pid file in `root` is written, and gives the pid.
fn started(root: &Path, cx: &mut VisualTestContext) -> i32 {
    let pid_file = root.join("pid");
    wait_until("the command to start", cx, |_| {
        fs::read_to_string(&pid_file).is_ok_and(|pid| pid.ends_with('\n'))
    });
    fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

/// Withdraws the folder's trust, as Untrust This Folder does.
fn untrust(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(false, cx));
    draw(cx);
}

/// Waits, in real time, until `done` holds, letting the app's work land.
fn wait_until(
    what: &str,
    cx: &mut VisualTestContext,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The folder's audit trail, as written so far.
fn audit_lines(root: &Path, cx: &mut VisualTestContext) -> String {
    let file: PathBuf = cx.update(|_, cx| {
        AuditLog::of(cx).run_pending();
        file_for(root, cx).expect("a state directory")
    });
    fs::read_to_string(file).unwrap_or_default()
}

/// The turn ended stopped, its one call answered "Folder trust was
/// withdrawn", the tools withdrawn, no prompt waiting, and nothing sent
/// after the first request.
fn assert_ended(
    panel: &Entity<ChatPanel>,
    provider: &ScriptedProvider,
    cx: &mut VisualTestContext,
) {
    let conversation = shown(panel, cx);
    let turn = &conversation.turns[0];
    assert_eq!(turn.state, TurnState::Stopped);
    assert!(conversation.tools_withdrawn);
    assert!(conversation.removal_pending());
    assert_eq!(turn.rounds.len(), 1, "the round is committed");
    assert_eq!(
        turn.rounds[0].results,
        [ToolResult::error("call_1", WITHDRAWN)]
    );
    let (streaming, waiting) = panel.read_with(cx, |panel, cx| {
        let view = panel.view().read(cx);
        (view.is_streaming(), view.waiting_permissions())
    });
    assert!(!streaming);
    assert!(waiting.is_empty(), "the prompt closed: {waiting:?}");
    assert_ne!(status(panel, cx), Some(ToolStatus::Done));
    assert_eq!(provider.requests().len(), 1, "nothing was sent");
}

/// The next send's request: the first request's messages unchanged, the
/// reply as it came, then one user message of the `is_error` result and
/// the user's text, and the `tool_removal` message last; the same tools.
fn assert_next_request(
    panel: &Entity<ChatPanel>,
    provider: &ScriptedProvider,
    cx: &mut VisualTestContext,
) {
    ask(panel, "Go on without it", cx);
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let (first, next): (&ChatRequest, &ChatRequest) = (&requests[0], &requests[1]);
    assert_eq!(next.tools, first.tools, "the tool set is frozen");
    assert_eq!(next.tools, tool_set());
    let messages = &next.messages;
    assert_eq!(messages.len(), 4, "{messages:#?}");
    assert_eq!(messages[..first.messages.len()], first.messages[..]);
    assert_eq!(messages[1].role, Role::Assistant);
    assert_eq!(messages[1].content.len(), 2, "the text and the call");
    assert_eq!(messages[1].content[1]["type"], "tool_use");
    assert_eq!(messages[1].content[1]["id"], "call_1");
    assert_eq!(messages[2].role, Role::User);
    let result = ToolResult::error("call_1", WITHDRAWN).block();
    let text = json!({"text": "Go on without it", "type": "text"});
    assert_eq!(messages[2].content, [result, text]);
    assert!(matches!(messages[3].role, Role::System { .. }));
    let tools = tool_set();
    let mut names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(messages[3].removed_tools(), names);
    // After it the chat has no tools: the reply finished the turn.
    let conversation = shown(panel, cx);
    assert!(!conversation.has_tools());
    assert_eq!(conversation.turns[1].state, TurnState::Done);
}

#[gpui::test]
fn trust_withdrawn_during_a_permission_wait(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::write(root.path().join("notes.txt"), "plans\n").unwrap();
    let input = json!({"args": ["-c", "touch ran"], "cwd": null, "program": "sh"});
    let provider = Arc::new(ScriptedProvider::new([
        calling("I'll run it.", "run_command", input),
        saying("Understood."),
    ]));
    let (workbench, panel, cx) = ready(root.path(), state.path(), &provider, cx);
    ask(&panel, "Make the file", cx);

    // `run_command` asks by default: its prompt waits.
    let waiting = panel.read_with(cx, |panel, cx| panel.view().read(cx).waiting_permissions());
    assert_eq!(waiting, [(0, "call_1".to_owned())]);
    let prompt = panel.read_with(cx, |panel, cx| {
        panel.view().read(cx).permission(0, "call_1").cloned()
    });
    let prompt = prompt.expect("the prompt");
    assert!(prompt.in_project);
    assert!(
        prompt.target.ends_with(" -c touch ran"),
        "{}",
        prompt.target
    );
    assert!(panel.read_with(cx, |panel, cx| panel.view().read(cx).is_streaming()));

    untrust(&workbench, cx);
    assert_ended(&panel, &provider, cx);
    assert_eq!(status(&panel, cx), Some(ToolStatus::Denied));
    // An answer to the closed prompt does nothing.
    let answered = panel.update(cx, |panel, cx| {
        panel.view().update(cx, |view, cx| {
            view.answer_permission(0, "call_1", PermissionAnswer::AllowOnce, cx)
        })
    });
    assert!(!answered);
    draw(cx);
    assert!(!root.path().join("ran").exists(), "the command never ran");
    // Its audit line: the prompt closed unanswered.
    let audit = audit_lines(root.path(), cx);
    assert!(audit.contains(r#""decision":"cancelled""#), "{audit}");

    assert_next_request(&panel, &provider, cx);
}

#[gpui::test]
fn trust_withdrawn_during_a_read(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::write(root.path().join("notes.txt"), "the secret plans\n").unwrap();
    let input = json!({"end_line": null, "path": "notes.txt", "start_line": null});
    let provider = Arc::new(ScriptedProvider::new([
        calling("Let me read it.", READ_FILE, input),
        saying("Understood."),
    ]));
    // The read's outcome is held in flight until the test lets it go.
    let (release, gate) = async_channel::bounded::<()>(1);
    cx.update(|cx| cx.set_global(Held(vec![(READ_FILE, gate)])));
    let (workbench, panel, cx) = ready(root.path(), state.path(), &provider, cx);
    ask(&panel, "What's in the notes?", cx);
    assert_eq!(status(&panel, cx), Some(ToolStatus::Running));
    assert!(panel.read_with(cx, |panel, cx| panel.view().read(cx).is_streaming()));
    assert!(shown(&panel, cx).turns[0].results.is_empty());

    untrust(&workbench, cx);
    assert_ended(&panel, &provider, cx);
    assert_eq!(status(&panel, cx), Some(ToolStatus::Failed));

    // Let go, the read never lands: it was cancelled with the turn.
    release.try_send(()).ok();
    draw(cx);
    assert_ended(&panel, &provider, cx);
    let sent = serde_json::to_string(&shown(&panel, cx)).unwrap();
    assert!(
        !sent.contains("secret"),
        "nothing the read gathered is kept"
    );

    assert_next_request(&panel, &provider, cx);
    let requests = serde_json::to_string(&provider.requests()[1].messages).unwrap();
    assert!(
        !requests.contains("secret"),
        "nothing the read gathered is sent"
    );
}

#[gpui::test]
fn trust_withdrawn_during_a_running_command(cx: &mut TestAppContext) {
    // The command's runner is a real thread, which wakes the loop.
    cx.executor().allow_parking();
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::write(root.path().join("notes.txt"), "plans\n").unwrap();
    // It prints `X42Y`, which its input doesn't hold.
    let script = "echo $$ > pid; echo X$((40 + 2))Y; exec sleep 30";
    let input = json!({"args": ["-c", script], "cwd": null, "program": "sh"});
    let provider = Arc::new(ScriptedProvider::new([
        calling("I'll run it.", "run_command", input),
        saying("Understood."),
    ]));
    let (workbench, panel, cx) = ready(root.path(), state.path(), &provider, cx);
    ask(&panel, "Run the slow thing", cx);
    let allowed = panel.update(cx, |panel, cx| {
        panel.view().update(cx, |view, cx| {
            view.answer_permission(0, "call_1", PermissionAnswer::AllowOnce, cx)
        })
    });
    assert!(allowed);
    let pid_file = root.path().join("pid");
    wait_until("the command to start", cx, |_| {
        fs::read_to_string(&pid_file).is_ok_and(|pid| pid.ends_with('\n'))
    });
    let pid: i32 = fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(status(&panel, cx), Some(ToolStatus::Running));
    // SAFETY: signal 0 only asks whether the process exists.
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0, "it runs");

    untrust(&workbench, cx);
    assert_ended(&panel, &provider, cx);
    assert_eq!(status(&panel, cx), Some(ToolStatus::Failed));

    // Its process group is killed, and the audit trail says why.
    wait_until("the command to be killed", cx, |cx| {
        audit_lines(root.path(), cx).contains(r#""killed":"trust_withdrawn""#)
    });
    // SAFETY: as above.
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "it was killed");
    draw(cx);
    // Its output never lands.
    assert_ended(&panel, &provider, cx);
    let kept = serde_json::to_string(&shown(&panel, cx)).unwrap();
    assert!(!kept.contains("X42Y"), "the command's output is discarded");

    assert_next_request(&panel, &provider, cx);
}

/// Plays `inner`'s replies, but for request `hang`, whose stream never
/// ends: a turn still streaming when the test stops it.
struct Hanging {
    inner: ScriptedProvider,
    hang: usize,
    asked: std::sync::atomic::AtomicUsize,
}

impl ChatProvider for Hanging {
    fn name(&self) -> &str {
        "hanging"
    }

    fn stream(&self, request: ChatRequest) -> fe_chat::ChatStream {
        let asked = (self.asked).fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if asked == self.hang {
            return Box::pin(futures::stream::pending());
        }
        self.inner.stream(request)
    }
}

/// F1: the last turn's proposals are handed in on their own. A turn that
/// begins while the hand-in waits begins its proposals after it, so the
/// hand-in never ends the new turn's, and the new turn's edit lands.
#[gpui::test]
fn a_hand_in_under_way_ends_only_its_own_turn(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::write(root.path().join("notes.txt"), "a\n").unwrap();
    let provider = Arc::new(Hanging {
        inner: ScriptedProvider::new([
            calling(
                "Changing it.",
                PROPOSE_EDIT,
                edit("notes.txt", "a\n", "b\n"),
            ),
            saying("Proposed."),
            calling("Again.", PROPOSE_EDIT, edit("notes.txt", "b\n", "c\n")),
            saying("Done."),
        ]),
        hang: 2,
        asked: Default::default(),
    });
    let (workbench, cx) = opened(cx, root.path());
    let shared = provider.clone();
    cx.update(|_, cx| {
        cx.set_global(ChatStateDir(Some(state.path().to_owned())));
        cx.set_global(KeySource {
            env: Some("key".to_owned()),
            store: Arc::new(MemoryStore::new()),
        });
        ChatPanel::set_provider_factory(
            cx,
            Rc::new(move |_: &ApiKey, _| shared.clone() as Arc<dyn ChatProvider>),
        );
    });
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    dispatch("chat.open", cx);
    let panel = workbench
        .read_with(cx, |workbench, cx| {
            workbench.dock().read(cx).chat().cloned()
        })
        .expect("made with the dock");
    ask(&panel, "Make it b", cx);
    let chat = shown(&panel, cx).id;
    assert_eq!(shown_review(&workbench, cx), Some(format!("{chat}:0")));

    // Turn 1 streams, and is stopped as turn 2 is sent: turn 1's hand-in
    // is still to come when turn 2 begins.
    ask(&panel, "Wait", cx);
    assert!(shown(&panel, cx).streaming_turn().is_some());
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            kiln::with_cx(window, cx, |cx| {
                view.stop_reply(cx);
                view.set_composer_text("Make it c", cx.cx);
                view.send_message(cx);
            })
        })
    });
    draw(cx);

    let conversation = shown(&panel, cx);
    assert_eq!(conversation.turns.len(), 3);
    assert_eq!(conversation.turns[1].state, TurnState::Stopped);
    let results = &conversation.turns[2].rounds[0].results;
    assert_eq!(results.len(), 1);
    assert!(!results[0].is_error, "{results:?}");
    assert!(results[0].content.starts_with("Proposed"), "{results:?}");
    assert_eq!(conversation.turns[2].state, TurnState::Done);
    assert_eq!(shown_review(&workbench, cx), Some(format!("{chat}:2")));
}

/// F2: New Chat in the middle of a turn ends it as Stop would: its
/// running command is killed (and its audit line written), and its
/// finished proposal still reaches the AI Changes panel.
#[gpui::test]
fn a_chat_replaced_mid_turn_hands_in_and_kills_its_work(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::write(root.path().join("notes.txt"), "a\n").unwrap();
    let provider = Arc::new(ScriptedProvider::new([calling_all(&[
        ("call_1", PROPOSE_EDIT, edit("notes.txt", "a\n", "b\n")),
        ("call_2", RUN_COMMAND, shell("echo $$ > pid; exec sleep 30")),
    ])]));
    let (workbench, panel, cx) = ready(root.path(), state.path(), &provider, cx);
    ask(&panel, "Change it and run", cx);
    let chat = shown(&panel, cx).id;
    answer(&panel, 0, "call_2", PermissionAnswer::AllowOnce, cx);
    let pid = started(root.path(), cx);
    assert_eq!(
        row(&panel, 0, "call_1", cx).map(|row| row.status),
        Some(ToolStatus::Done)
    );
    assert_eq!(
        shown_review(&workbench, cx),
        None,
        "not until the turn ends"
    );

    dispatch_in_chat("chat.new", cx);
    assert_ne!(shown(&panel, cx).id, chat, "a new chat");
    wait_until("the command to be killed", cx, |cx| {
        audit_lines(root.path(), cx).contains(r#""killed":"stop""#)
    });
    // SAFETY: signal 0 only asks whether the process exists.
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "it was killed");
    draw(cx);
    assert_eq!(shown_review(&workbench, cx), Some(format!("{chat}:0")));
}

/// Runs command `id` in the chat's context.
fn dispatch_in_chat(id: &str, cx: &mut VisualTestContext) {
    let stack = (ContextStack::new())
        .with(ContextFrame::new(crate::KEY_CONTEXT))
        .with(ContextFrame::new("chat"));
    let run = RunCommand(Invocation::of(id).unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// F3: a turn's edits run in the calls' order. With `propose_edit` set to
/// ask, allowing the second (b→c) before the first (a→b) still runs a→b
/// first, so both land.
#[gpui::test]
fn edits_run_in_the_calls_order_whatever_is_answered_first(cx: &mut TestAppContext) {
    let config = tempfile::tempdir().unwrap();
    let _sender = settings(
        config.path(),
        "[agents.permissions]\npropose_edit = \"ask\"\n",
        cx,
    );
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::write(root.path().join("notes.txt"), "a\n").unwrap();
    let provider = Arc::new(ScriptedProvider::new([
        calling_all(&[
            ("call_1", PROPOSE_EDIT, edit("notes.txt", "a\n", "b\n")),
            ("call_2", PROPOSE_EDIT, edit("notes.txt", "b\n", "c\n")),
        ]),
        saying("Done."),
    ]));
    let (_workbench, panel, cx) = ready(root.path(), state.path(), &provider, cx);
    ask(&panel, "Make it c", cx);
    let waiting = panel.read_with(cx, |panel, cx| panel.view().read(cx).waiting_permissions());
    assert_eq!(waiting.len(), 2, "{waiting:?}");

    answer(&panel, 0, "call_2", PermissionAnswer::AllowOnce, cx);
    assert_eq!(
        row(&panel, 0, "call_2", cx).map(|row| row.status),
        Some(ToolStatus::Pending),
        "it waits for call_1"
    );
    answer(&panel, 0, "call_1", PermissionAnswer::AllowOnce, cx);

    let conversation = shown(&panel, cx);
    let results = &conversation.turns[0].rounds[0].results;
    assert_eq!(results.len(), 2);
    for result in results {
        assert!(!result.is_error, "{results:?}");
        assert!(result.content.starts_with("Proposed"), "{results:?}");
    }
    assert_eq!(conversation.turns[0].state, TurnState::Done);
}

/// F4: a root change while a command runs kills it and still writes its
/// audit line, though the chat is replaced and its loop gone before the
/// command has ended.
#[gpui::test]
fn a_root_change_mid_command_still_records_the_kill(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::write(root.path().join("notes.txt"), "a\n").unwrap();
    let provider = Arc::new(ScriptedProvider::new([calling(
        "I'll run it.",
        RUN_COMMAND,
        shell("echo $$ > pid; exec sleep 30"),
    )]));
    let (workbench, panel, cx) = ready(root.path(), state.path(), &provider, cx);
    ask(&panel, "Run the slow thing", cx);
    let chat = shown(&panel, cx).id;
    answer(&panel, 0, "call_1", PermissionAnswer::AllowOnce, cx);
    let pid = started(root.path(), cx);

    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.set_root(other.path().to_owned(), window, cx)
        })
    });
    draw(cx);
    assert_ne!(shown(&panel, cx).id, chat, "the other folder's chat shows");
    wait_until("the kill's audit line", cx, |cx| {
        audit_lines(root.path(), cx).contains(r#""killed":"trust_withdrawn""#)
    });
    // SAFETY: signal 0 only asks whether the process exists.
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "it was killed");
    assert_eq!(provider.requests().len(), 1, "nothing was sent");
}

/// F5: a command stopped with its turn takes a while to end (it ignores
/// SIGTERM until the SIGKILL). Its end never lands on the next turn's
/// call of the same id.
#[gpui::test]
fn a_stopped_commands_end_never_lands_on_a_later_call(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::write(root.path().join("notes.txt"), "a\n").unwrap();
    let stubborn = "trap '' TERM; echo $$ > pid; while :; do sleep 0.05; done";
    let provider = Arc::new(ScriptedProvider::new([
        calling("I'll run it.", RUN_COMMAND, shell(stubborn)),
        calling("Once more.", RUN_COMMAND, shell("true")),
    ]));
    let (_workbench, panel, cx) = ready(root.path(), state.path(), &provider, cx);
    ask(&panel, "Run the stubborn thing", cx);
    answer(&panel, 0, "call_1", PermissionAnswer::AllowOnce, cx);
    started(root.path(), cx);
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            kiln::with_cx(window, cx, |cx| view.stop_reply(cx))
        })
    });
    draw(cx);
    let stopped = shown(&panel, cx);
    assert_eq!(stopped.turns[0].state, TurnState::Stopped);
    assert_eq!(
        stopped.turns[0].rounds[0].results,
        [ToolResult::not_run("call_1", NotRun::Stopped)]
    );

    ask(&panel, "Try again", cx);
    let waiting = panel.read_with(cx, |panel, cx| panel.view().read(cx).waiting_permissions());
    assert_eq!(waiting, [(1, "call_1".to_owned())]);
    wait_until("the stopped command's end", cx, |cx| {
        audit_lines(root.path(), cx).contains(r#""killed":"stop""#)
    });
    draw(cx);
    let conversation = shown(&panel, cx);
    assert!(conversation.turns[1].results.is_empty(), "nothing landed");
    let waiting = panel.read_with(cx, |panel, cx| panel.view().read(cx).waiting_permissions());
    assert_eq!(waiting, [(1, "call_1".to_owned())], "it still asks");
    assert_eq!(
        row(&panel, 1, "call_1", cx).map(|row| row.status),
        Some(ToolStatus::Pending)
    );
}

/// F6: a copy of a chat taken mid-turn (as the panel keeps in memory)
/// opens stopped, not busy: its waiting calls answered, Send usable.
#[gpui::test]
fn a_copy_taken_mid_turn_opens_stopped(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let (_workbench, cx) = opened(cx, root.path());
    let mut conversation = Conversation::new(&ChatSettings::default());
    conversation.set_tools(tool_set(), 0);
    conversation.begin_turn(vec![json!({"text": "Read it", "type": "text"})], Vec::new());
    let input = json!({"end_line": null, "path": "notes.txt", "start_line": null});
    for event in calling("Reading.", READ_FILE, input) {
        conversation.apply(event);
    }
    assert_eq!(conversation.pending_calls().len(), 1, "a call waits");
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let provider = Arc::new(ScriptedProvider::new([]));
            ChatView::new(provider, conversation, ChatOptions::default(), window, cx)
        })
    });
    let (busy, copy) = view.read_with(cx, |view, _| {
        (view.is_streaming(), view.conversation().clone())
    });
    assert!(!busy);
    assert_eq!(copy.turns[0].state, TurnState::Stopped);
    assert_eq!(
        copy.turns[0].rounds[0].results,
        [ToolResult::not_run("call_1", NotRun::Stopped)]
    );
}

/// F7: on a withdrawal, a finished call's row loses what it gathered,
/// whatever its status: here a command that printed and exited 3 while
/// another call waited for permission.
#[gpui::test]
fn withdrawal_discards_every_finished_calls_output(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::write(root.path().join("notes.txt"), "a\n").unwrap();
    let provider = Arc::new(ScriptedProvider::new([calling_all(&[
        ("call_1", RUN_COMMAND, shell("echo Z$((1 + 1))Q; exit 3")),
        ("call_2", RUN_COMMAND, shell("true")),
    ])]));
    let (workbench, panel, cx) = ready(root.path(), state.path(), &provider, cx);
    ask(&panel, "Run both", cx);
    answer(&panel, 0, "call_1", PermissionAnswer::AllowOnce, cx);
    wait_until("call_1 to end", cx, |cx| {
        row(&panel, 0, "call_1", cx).is_some_and(|row| row.status != ToolStatus::Running)
    });
    let ran = row(&panel, 0, "call_1", cx).unwrap();
    assert!(
        ran.output.as_deref().unwrap_or("").contains("Z2Q"),
        "{ran:?}"
    );

    untrust(&workbench, cx);
    let withdrawn = row(&panel, 0, "call_1", cx).unwrap();
    assert!(
        !withdrawn.output.as_deref().unwrap_or("").contains("Z2Q"),
        "{withdrawn:?}"
    );
    assert_eq!(withdrawn.status, ToolStatus::Failed);
    let results = &shown(&panel, cx).turns[0].rounds[0].results;
    assert_eq!(
        results,
        &[
            ToolResult::error("call_1", WITHDRAWN),
            ToolResult::error("call_2", WITHDRAWN)
        ]
    );
}
