//! What came of applying a reply's code blocks ([`super`]), on their
//! headers, on temp folders: each reply a [`ScriptedProvider`]'s, Apply
//! pressed on its block's header, and the proposal's hunks decided in the
//! editor.

use std::{fs, path::Path, rc::Rc, sync::Arc};

use fe_buffer::{HunkId, HunkStatus};
use fe_chat::{BlockKind, ChatEvent, ChatProvider, ScriptedProvider, StopReason};
use fe_chat_view::{BlockState, ChatViewEvent, SendMessage};
use fe_command::Invocation;
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
const LIB: &str = "\
package main

func a() int {
    return 1
}

func b() int {
    return 2
}

func c() int {
    return 3
}
";

/// Two sections for `src/lib.go`, a hunk each.
const TWO_SECTIONS: &str = "```go path=src/lib.go
<<<<<<< SEARCH
    return 1
=======
    return 10
>>>>>>> REPLACE

<<<<<<< SEARCH
    return 3
=======
    return 30
>>>>>>> REPLACE
```
";

/// A whole `src/main.go`, one line changed.
const WHOLE_MAIN: &str = "```go path=src/main.go\nfunc main() {\n    x := 2\n}\n```\n";

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
    let temp = fs::canonicalize(std::env::temp_dir()).unwrap();
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

/// The chat, open, over a provider that streams `replies` in turn, the
/// first one asked for.
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

/// Apply on code block `block` of turn `turn`'s reply, as its header
/// asks (a header scrolled out of the transcript takes no click), and
/// what it starts landed.
fn apply(panel: &Entity<ChatPanel>, turn: usize, block: usize, cx: &mut VisualTestContext) {
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |_, cx| {
        cx.emit(ChatViewEvent::ApplyBlock { turn, block })
    });
    draw(cx);
}

fn has(selector: String, cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(selector.leak()).is_some()
}

/// What block `block` of turn `turn`'s header shows came of it.
fn state(
    panel: &Entity<ChatPanel>,
    turn: usize,
    block: usize,
    cx: &mut VisualTestContext,
) -> Option<BlockState> {
    panel.read_with(cx, |panel, cx| {
        panel.view().read(cx).block_state(turn, block).cloned()
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

/// The ids of the hunks still pending in the proposals `editor` shows,
/// in order.
fn pending(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<HunkId> {
    editor.read_with(cx, |editor, _| {
        let snapshot = editor.model().buffer().snapshot();
        (editor.proposals().iter())
            .flat_map(|proposal| proposal.borrow().hunks(&snapshot))
            .filter(|hunk| hunk.status == HunkStatus::Pending)
            .map(|hunk| hunk.id)
            .collect()
    })
}

fn accept(editor: &Entity<EditorView>, hunk: HunkId, cx: &mut VisualTestContext) {
    editor
        .update(cx, |editor, cx| editor.accept_hunk(hunk, cx))
        .unwrap();
    draw(cx);
}

fn reject(editor: &Entity<EditorView>, hunk: HunkId, cx: &mut VisualTestContext) {
    editor
        .update(cx, |editor, cx| editor.reject_hunk(hunk, cx))
        .unwrap();
    draw(cx);
}

#[gpui::test]
fn an_applied_block_follows_its_hunks(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let text = format!("Both:\n\n{TWO_SECTIONS}\n{WHOLE_MAIN}");
    let panel = chat_with(&workbench, &[&text], cx);
    assert_eq!(state(&panel, 0, 0, cx), None, "not applied yet");
    assert!(!has("chat-block-state-0-0".into(), cx));

    apply(&panel, 0, 0, cx);
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());
    assert_eq!(
        state(&panel, 0, 0, cx),
        Some(BlockState::Proposed {
            pending: 2,
            hunks: 2
        })
    );
    assert!(has("chat-block-state-0-0".into(), cx));
    assert!(!has("chat-block-error-0-0".into(), cx));

    let lib = editor_of(&workbench, &root.join("src/lib.go"), cx);
    let hunks = pending(&lib, cx);
    assert_eq!(hunks.len(), 2);
    accept(&lib, hunks[0], cx);
    assert_eq!(
        state(&panel, 0, 0, cx),
        Some(BlockState::Proposed {
            pending: 1,
            hunks: 2
        })
    );
    reject(&lib, hunks[1], cx);
    assert_eq!(
        state(&panel, 0, 0, cx),
        Some(BlockState::Decided {
            accepted: 1,
            hunks: 2
        })
    );
    assert!(has("chat-block-state-0-0".into(), cx));

    // Another block, all its hunks rejected; the first keeps its own.
    apply(&panel, 0, 1, cx);
    assert_eq!(
        state(&panel, 0, 1, cx),
        Some(BlockState::Proposed {
            pending: 1,
            hunks: 1
        })
    );
    let main = editor_of(&workbench, &root.join("src/main.go"), cx);
    let hunks = pending(&main, cx);
    assert_eq!(hunks.len(), 1);
    reject(&main, hunks[0], cx);
    assert_eq!(
        state(&panel, 0, 1, cx),
        Some(BlockState::Decided {
            accepted: 0,
            hunks: 1
        })
    );
    assert!(has("chat-block-state-0-1".into(), cx));
    assert_eq!(
        state(&panel, 0, 0, cx),
        Some(BlockState::Decided {
            accepted: 1,
            hunks: 2
        })
    );
}

#[gpui::test]
fn a_block_that_cannot_apply_says_why(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let text = "\
```go path=src/lib.go
<<<<<<< SEARCH
nowhere
=======
x
>>>>>>> REPLACE
```

```go path=../outside.go
package outside
```

```go
func other() {}
```

```go path=src/main.go
func main() {
    x := 1
}
```
";
    let panel = chat_with(&workbench, &[text], cx);

    apply(&panel, 0, 0, cx);
    let why = "SEARCH 1 (\"nowhere\") isn't in it";
    assert_eq!(
        take_toasts(&workbench, cx),
        [format!("Can't apply the block to src/lib.go: {why}")],
        "the toast stays"
    );
    assert_eq!(
        state(&panel, 0, 0, cx),
        Some(BlockState::Failed(why.to_owned()))
    );
    assert!(has("chat-block-state-0-0".into(), cx));
    assert!(has("chat-block-error-0-0".into(), cx));

    apply(&panel, 0, 1, cx);
    assert_eq!(
        take_toasts(&workbench, cx),
        ["Can't apply the block to ../outside.go: it's outside the folder"]
    );
    assert_eq!(
        state(&panel, 0, 1, cx),
        Some(BlockState::Failed("it's outside the folder".to_owned()))
    );
    assert!(has("chat-block-error-0-1".into(), cx));

    // No path, and the turn was sent with no selection.
    apply(&panel, 0, 2, cx);
    let message = "This block doesn't say which file it's for";
    assert_eq!(take_toasts(&workbench, cx), [message]);
    assert_eq!(
        state(&panel, 0, 2, cx),
        Some(BlockState::Failed(message.to_owned()))
    );
    assert!(has("chat-block-error-0-2".into(), cx));

    apply(&panel, 0, 3, cx);
    assert_eq!(
        take_toasts(&workbench, cx),
        ["The block changes nothing in src/main.go"]
    );
    assert_eq!(state(&panel, 0, 3, cx), Some(BlockState::NoChange));
    assert!(has("chat-block-state-0-3".into(), cx));
    assert!(!has("chat-block-error-0-3".into(), cx));
}

#[gpui::test]
fn applying_again_or_closing_the_file_resets_the_state(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let panel = chat_with(&workbench, &[WHOLE_MAIN, WHOLE_MAIN], cx);

    // Decided, then applied again: proposed again.
    apply(&panel, 0, 0, cx);
    let main = editor_of(&workbench, &root.join("src/main.go"), cx);
    let hunks = pending(&main, cx);
    reject(&main, hunks[0], cx);
    assert_eq!(
        state(&panel, 0, 0, cx),
        Some(BlockState::Decided {
            accepted: 0,
            hunks: 1
        })
    );
    apply(&panel, 0, 0, cx);
    let proposed = Some(BlockState::Proposed {
        pending: 1,
        hunks: 1,
    });
    assert_eq!(state(&panel, 0, 0, cx), proposed);

    // Its file's editor closed with the hunk still pending: cleared.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    drop(main);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.close_active(window, cx)));
    draw(cx);
    assert_eq!(
        workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx).len()),
        0,
        "closed"
    );
    assert_eq!(state(&panel, 0, 0, cx), None);
    assert!(!has("chat-block-state-0-0".into(), cx));

    // Applied again, then a new chat: its view shows none.
    apply(&panel, 0, 0, cx);
    assert_eq!(state(&panel, 0, 0, cx), proposed);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.start_chat(window, cx)));
    draw(cx);
    send(&panel, cx);
    assert_eq!(state(&panel, 0, 0, cx), None);
    assert!(has("chat-apply-0-0".into(), cx));
    assert!(!has("chat-block-state-0-0".into(), cx));
}

#[gpui::test]
fn an_earlier_apply_landing_later_leaves_the_state(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let panel = chat_with(&workbench, &[WHOLE_MAIN], cx);

    // The first Apply's file is found, but held from landing.
    let release = panel.update(cx, |panel, _| panel.applies.hold_next_locate());
    apply(&panel, 0, 0, cx);
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());
    // A second lands, and its hunk is accepted.
    apply(&panel, 0, 0, cx);
    let main = editor_of(&workbench, &root.join("src/main.go"), cx);
    let hunks = pending(&main, cx);
    assert_eq!(hunks.len(), 1);
    accept(&main, hunks[0], cx);
    let accepted = Some(BlockState::Decided {
        accepted: 1,
        hunks: 1,
    });
    assert_eq!(state(&panel, 0, 0, cx), accepted);

    // The first lands on the accepted text: its toast, but the header
    // keeps the second's outcome, and follows it still.
    release.send_blocking(()).unwrap();
    draw(cx);
    assert_eq!(
        take_toasts(&workbench, cx),
        ["The block changes nothing in src/main.go"]
    );
    assert_eq!(state(&panel, 0, 0, cx), accepted);
    assert!(has("chat-block-state-0-0".into(), cx));
    assert!(!has("chat-block-error-0-0".into(), cx));
}
