//! Review Changes ([`super`]) on temp folders: each reply a
//! [`ScriptedProvider`]'s, the button pressed on the turn's footer, and the
//! changeset it loads looked at in the AI Changes panel.

#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::{cell::RefCell, fs, path::Path, rc::Rc, sync::Arc};

use fe_chat::{BlockKind, ChatEvent, ChatProvider, ScriptedProvider, StopReason};
use fe_chat_view::SendMessage;
use fe_command::{Actor, Invocation, Source};
use fe_gpui::EditorView;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::core::secrets::MemoryStore;
use serde_json::json;

use crate::{
    Launch, Workbench,
    chat_panel::{ChatPanel, KeySource},
    tests::open,
};

const MAIN: &str = "func main() {\n    x := 1\n}\n";
const LIB: &str = "package main\n\nfunc a() int {\n    return 1\n}\n";

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

/// The chat, open, over a provider that streams `replies` in turn, and
/// `question` sent.
fn chat_with(
    workbench: &Entity<Workbench>,
    replies: &[&str],
    cx: &mut VisualTestContext,
) -> Entity<ChatPanel> {
    let provider = Arc::new(ScriptedProvider::new(
        replies.iter().map(|text| reply(text)).collect::<Vec<_>>(),
    ));
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
    let panel = workbench
        .read_with(cx, |workbench, cx| {
            workbench.dock().read(cx).chat().cloned()
        })
        .expect("made");
    send(&panel, cx);
    panel
}

fn send(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) {
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |view, cx| view.set_composer_text("Change it", cx));
    let focus = panel.read_with(cx, gpui::Focusable::focus_handle);
    cx.update(|window, cx| window.focus(&focus, cx));
    draw(cx);
    cx.dispatch_action(SendMessage);
    draw(cx);
}

fn press(selector: String, cx: &mut VisualTestContext) {
    let button = cx.debug_bounds(selector.leak()).expect("drawn");
    cx.simulate_click(button.center(), Default::default());
    draw(cx);
}

fn has(selector: String, cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(selector.leak()).is_some()
}

fn review(turn: usize, cx: &mut VisualTestContext) {
    press(format!("chat-review-changes-{turn}"), cx);
}

fn chat_id(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> String {
    panel.read_with(cx, |panel, cx| {
        panel.view().read(cx).conversation().id.to_string()
    })
}

fn take_toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    let shown = layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    });
    layer.update(cx, |layer, cx| layer.dismiss_all(cx));
    shown
}

/// The AI Changes panel's rows: path from the root and added, removed.
fn rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, usize, usize)> {
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).ai_changes().cloned()
    });
    let Some(panel) = panel else {
        return Vec::new();
    };
    panel.read_with(cx, |panel, _| {
        (panel.shown_rows().into_iter())
            .map(|(path, added, removed, _)| (path, added, removed))
            .collect()
    })
}

fn turn_id(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    workbench.read_with(cx, |workbench, _| {
        (workbench.ai_changes.as_ref()).map(|state| state.changeset.turn_id().to_owned())
    })
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

fn pending(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    editor.read_with(cx, |editor, _| {
        let snapshot = editor.model().snapshot();
        (editor.proposals().iter())
            .map(|proposal| proposal.borrow().hunks(snapshot).len())
            .sum()
    })
}

const TWO: &str = "Both:\n\n```go path=src/main.go\nfunc main() {\n    x := 2\n}\n```\n\n\
```go path=src/lib.go\npackage main\n\nfunc a() int {\n    return 2\n}\n```\n";

#[gpui::test]
fn button_shows_for_two_path_blocks(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, dir.path());
    chat_with(&workbench, &[TWO], cx);
    assert!(has("chat-review-changes-0".into(), cx));
    assert!(has("chat-apply-0-0".into(), cx), "beside Apply");
    assert!(has("chat-apply-0-1".into(), cx));
}

#[gpui::test]
fn single_block_has_only_apply(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, dir.path());
    let one = "```go path=src/main.go\nfunc main() {}\n```\n\n```go\nfunc other() {}\n```\n";
    chat_with(&workbench, &[one], cx);
    assert!(has("chat-apply-0-0".into(), cx));
    assert!(!has("chat-review-changes-0".into(), cx));
}

#[gpui::test]
fn apply_still_proposes_one_block(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    chat_with(&workbench, &[TWO], cx);
    press("chat-apply-0-1".into(), cx);
    let lib = editor_of(&workbench, &root.join("src/lib.go"), cx);
    assert_eq!(pending(&lib, cx), 1);
    assert_eq!(turn_id(&workbench, cx), None, "no changeset");
}

#[gpui::test]
fn review_builds_changeset_for_all_blocks(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let panel = chat_with(&workbench, &[TWO], cx);
    let chat = chat_id(&panel, cx);
    review(0, cx);
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());
    assert_eq!(turn_id(&workbench, cx), Some(format!("{chat}:0")));
    assert_eq!(
        rows(&workbench, cx),
        [
            ("src/main.go".to_owned(), 1, 1),
            ("src/lib.go".to_owned(), 1, 1)
        ]
    );
    for file in ["src/main.go", "src/lib.go"] {
        let editor = editor_of(&workbench, &root.join(file), cx);
        assert_eq!(pending(&editor, cx), 1, "{file} shows its proposal");
    }
}

#[gpui::test]
fn review_skips_bad_block_and_toasts_the_reason(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, dir.path());
    let text = "```go path=src/main.go\n<<<<<<< SEARCH\nnowhere\n=======\nx\n>>>>>>> REPLACE\n```\n\n\
```go path=src/lib.go\npackage main\n```\n";
    chat_with(&workbench, &[text], cx);
    review(0, cx);
    assert_eq!(
        take_toasts(&workbench, cx),
        ["Can't apply the block to src/main.go: SEARCH 1 (\"nowhere\") isn't in it"]
    );
    assert_eq!(rows(&workbench, cx).len(), 1);
}

#[gpui::test]
fn new_file_block_becomes_new_file_entry(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let text = "```go path=src/new.go\npackage main\n```\n\n\
```go path=nodir/new.go\npackage main\n```\n\n\
```go path=src/main.go\nfunc main() {}\n```\n";
    chat_with(&workbench, &[text], cx);
    review(0, cx);
    assert_eq!(
        take_toasts(&workbench, cx),
        ["Can't apply the block to nodir/new.go: its folder isn't there"]
    );
    let rows = rows(&workbench, cx);
    assert_eq!(
        rows.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
        ["src/new.go", "src/main.go"]
    );
    let new = root.join("src/new.go");
    workbench.read_with(cx, |workbench, _| {
        let state = workbench.ai_changes.as_ref().unwrap();
        assert_eq!(state.changeset.new_file_text(&new), Some("package main\n"));
    });
    assert!(!new.exists(), "nothing written yet");
}

#[gpui::test]
#[cfg(unix)]
fn path_outside_root_refused(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let outside = tempfile::tempdir_in(dunce::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
    fs::write(outside.path().join("x.go"), "x\n").unwrap();
    symlink(outside.path(), root.join("link")).unwrap();
    let (workbench, cx) = opened(cx, root);
    let text = "```go path=../x.go\nx\n```\n\n```go path=link/x.go\ny\n```\n";
    chat_with(&workbench, &[text], cx);
    review(0, cx);
    let toasts = take_toasts(&workbench, cx);
    assert_eq!(toasts.len(), 2, "{toasts:?}");
    assert!(
        toasts
            .iter()
            .all(|toast| toast.ends_with("it's outside the folder"))
    );
    assert_eq!(turn_id(&workbench, cx), None, "no block loaded: no panel");
    assert_eq!(
        fs::read_to_string(outside.path().join("x.go")).unwrap(),
        "x\n"
    );
}

#[gpui::test]
fn review_is_dispatched_as_ai_actor(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, dir.path());
    let seen: Rc<RefCell<Vec<Invocation>>> = Rc::default();
    let observed = seen.clone();
    cx.update(|_, cx| {
        kiln::platform::commands::observe_invocations(cx, move |invocation| {
            observed.borrow_mut().push(invocation.clone());
        });
    });
    let panel = chat_with(&workbench, &[TWO], cx);
    let chat = chat_id(&panel, cx);
    review(0, cx);
    let reviews: Vec<Invocation> = (seen.borrow().iter())
        .filter(|invocation| invocation.command.as_str() == "chat.review_changes")
        .cloned()
        .collect();
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].args, json!({"chat": chat, "turn": 0}));
    assert_eq!(
        reviews[0].actor,
        Actor::Ai {
            agent: "fastedit.chat".to_owned(),
            session: chat.clone()
        }
    );
    assert_eq!(reviews[0].source, Source::Script);

    // Replays.
    let replay = reviews[0].clone();
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.run_review_changes(&replay, window, cx)
        })
    });
    draw(cx);
    assert_eq!(rows(&workbench, cx).len(), 2);
}

#[gpui::test]
#[cfg(unix)]
fn partial_failures_do_not_block_others(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    fs::write(root.join("src/ro.go"), "a\n").unwrap();
    fs::set_permissions(root.join("src/ro.go"), fs::Permissions::from_mode(0o444)).unwrap();
    let (workbench, cx) = opened(cx, root);
    let text = "```go path=src/ro.go\nx\n```\n\n```go path=src/lib.go\nsame\n```\n\n\
```go path=src/main.go\n<<<<<<< SEARCH\nfunc main() {\n=======\nfn main() {\n>>>>>>> REPLACE\n```\n\n\
```go path=src/lib.go\nagain\n```\n";
    chat_with(&workbench, &[text], cx);
    review(0, cx);
    let toasts = take_toasts(&workbench, cx);
    assert!(
        toasts
            .iter()
            .any(|toast| toast == "Can't apply the block to src/ro.go: it's read-only"),
        "{toasts:?}"
    );
    assert!(
        toasts
            .iter()
            .any(|toast| toast.contains("src/lib.go: the reply has"))
    );
    // lib.go (the first block) and main.go load.
    let paths: Vec<String> = rows(&workbench, cx).into_iter().map(|row| row.0).collect();
    assert_eq!(paths, ["src/lib.go", "src/main.go"]);
}

#[gpui::test]
fn rerun_replaces(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    chat_with(&workbench, &[TWO], cx);
    review(0, cx);
    // The panel took the chat's place in the dock.
    let run = Invocation::of("chat.open").unwrap();
    cx.update(|window, cx| {
        kiln::platform::command_actions::dispatch_invocation(run, window, cx).unwrap()
    });
    draw(cx);
    review(0, cx);
    assert_eq!(rows(&workbench, cx).len(), 2, "no file twice");
    let main = editor_of(&workbench, &root.join("src/main.go"), cx);
    assert_eq!(pending(&main, cx), 1, "one proposal on the editor");
}

#[gpui::test]
fn same_error_text_as_apply(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, dir.path());
    let text = "```go path=src/main.go\n<<<<<<< SEARCH\nnowhere\n=======\nx\n>>>>>>> REPLACE\n```\n\n\
```go path=src/lib.go\npackage main\n```\n";
    chat_with(&workbench, &[text], cx);
    press("chat-apply-0-0".into(), cx);
    let applied = take_toasts(&workbench, cx);
    review(0, cx);
    let reviewed = take_toasts(&workbench, cx);
    assert_eq!(applied.len(), 1);
    assert_eq!(reviewed, applied);
}

fn show_chat(cx: &mut VisualTestContext) {
    let run = Invocation::of("chat.open").unwrap();
    cx.update(|window, cx| {
        kiln::platform::command_actions::dispatch_invocation(run, window, cx).unwrap()
    });
    draw(cx);
}

#[gpui::test]
#[cfg(unix)]
fn aliases_of_one_file_are_reviewed_once(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    symlink(root.join("src"), root.join("alias")).unwrap();
    let (workbench, cx) = opened(cx, root);
    let text = "```go path=src/lib.go\npackage main\n```\n\n```go path=alias/lib.go\nother\n```\n";
    chat_with(&workbench, &[text], cx);
    review(0, cx);
    assert_eq!(take_toasts(&workbench, cx).len(), 1, "the alias is refused");
    assert_eq!(rows(&workbench, cx).len(), 1);
}

#[gpui::test]
fn a_failing_rerun_drops_the_old_changeset(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let text = "```go path=src/main.go\n<<<<<<< SEARCH\nx := 1\n=======\nx := 2\n>>>>>>> REPLACE\n```\n\n\
```go path=src/lib.go\n<<<<<<< SEARCH\nreturn 1\n=======\nreturn 2\n>>>>>>> REPLACE\n```\n";
    chat_with(&workbench, &[text], cx);
    review(0, cx);
    let main = editor_of(&workbench, &root.join("src/main.go"), cx);
    let lib = editor_of(&workbench, &root.join("src/lib.go"), cx);
    assert_eq!(pending(&main, cx), 1);
    // The text the blocks search for is gone.
    main.update(cx, |main, cx| {
        let len = main.model().buffer().len();
        main.model_mut()
            .buffer_mut()
            .edit([(0..len, "nothing\n")], fe_buffer::Origin::User);
        cx.notify();
    });
    lib.update(cx, |lib, cx| {
        let len = lib.model().buffer().len();
        lib.model_mut()
            .buffer_mut()
            .edit([(0..len, "nothing\n")], fe_buffer::Origin::User);
        cx.notify();
    });
    draw(cx);
    show_chat(cx);
    review(0, cx);
    assert_eq!(pending(&main, cx), 0);
    assert_eq!(pending(&lib, cx), 0);
    assert_eq!(turn_id(&workbench, cx), None);
}

#[gpui::test]
fn reviewing_a_turn_again_after_another_leaves_one_proposal(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let panel = chat_with(&workbench, &[TWO, TWO], cx);
    send(&panel, cx);
    review(0, cx);
    show_chat(cx);
    review(1, cx);
    show_chat(cx);
    review(0, cx);
    let main = editor_of(&workbench, &root.join("src/main.go"), cx);
    // Turn 0's earlier proposal is gone, not beside its new one.
    assert_eq!(pending(&main, cx), 1);
}

#[gpui::test]
fn the_typed_command_reviews_the_latest_turn(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, dir.path());
    let seen: Rc<RefCell<Vec<Invocation>>> = Rc::default();
    let observed = seen.clone();
    cx.update(|_, cx| {
        kiln::platform::commands::observe_invocations(cx, move |invocation| {
            observed.borrow_mut().push(invocation.clone());
        });
    });
    chat_with(&workbench, &[TWO], cx);
    let stack =
        fe_command::ContextStack::new().with(fe_command::ContextFrame::new(crate::KEY_CONTEXT));
    let run = fe_gpui::commands::RunCommand(
        Invocation::of("chat.review_latest_changes")
            .unwrap()
            .context(stack),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    assert_eq!(rows(&workbench, cx).len(), 2);
    let ran = (seen.borrow().iter())
        .filter(|invocation| invocation.command.as_str() == "chat.review_changes")
        .count();
    assert_eq!(ran, 1, "through the command Review runs by");
}
