//! Applying a reply's code blocks ([`super::super::chat_apply`]) on temp
//! folders: each reply a [`ScriptedProvider`]'s, Apply pressed on its
//! block's header, and the proposal it makes in the editor looked at.

#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use fe_buffer::{HunkStatus, HunkView, Origin, ResolvedSelection};
use fe_chat::{BlockKind, ChatEvent, ChatProvider, Rows, ScriptedProvider, StopReason};
use fe_chat_view::{ChatViewEvent, RetryTurn, SendMessage};
use fe_command::{Actor, Invocation, Source};
use fe_gpui::EditorView;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::core::secrets::MemoryStore;
use serde_json::json;

use super::{ChatPanel, chat, dispatch, dispatch_in, draw, opened, set_source};
use crate::Workbench;

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
    return 1
}
";

/// A reply of `text`, streamed as one text block.
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

/// A folder holding `src/main.go` and `src/lib.go`: Go, whose
/// project file turns the built-in language server (gopls) off for. Under the real temp folder: on
/// macOS `/var` is a link to `/private/var`, and the workbench opens
/// files by their real paths.
fn project() -> tempfile::TempDir {
    let temp = dunce::canonicalize(std::env::temp_dir()).unwrap();
    let dir = tempfile::tempdir_in(temp).unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.go"), MAIN).unwrap();
    fs::write(dir.path().join("src/lib.go"), LIB).unwrap();
    let settings = fe_config::project_settings_path(dir.path());
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(settings, "[lsp.servers.gopls]\nenabled = false\n").unwrap();
    dir
}

/// The chat, open, over a provider that gives `replies` in turn.
fn open_chat(
    workbench: &Entity<Workbench>,
    replies: &[&str],
    cx: &mut VisualTestContext,
) -> Entity<ChatPanel> {
    open_chat_with(
        workbench,
        replies.iter().map(|text| reply(text)).collect(),
        cx,
    )
}

/// The chat, open, over a provider that streams `replies` in turn.
fn open_chat_with(
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
    });
    set_source(Some("key"), &Arc::new(MemoryStore::new()), cx);
    dispatch("chat.open", cx);
    chat(workbench, cx).expect("made")
}

/// Opens `path` in the active pane, keeping focus where it is, and gives
/// back its editor.
fn open_file(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| {
        let focused = window.focused(cx);
        pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx));
        if let Some(focused) = focused {
            window.focus(&focused, cx);
        }
    });
    draw(cx);
    pane.read_with(cx, |pane, _| pane.active_editor().cloned())
        .expect("an editor")
}

/// Edits `editor`'s buffer as the user, unsaved.
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

fn select(editor: &Entity<EditorView>, range: std::ops::Range<usize>, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        let selection = ResolvedSelection {
            range,
            reversed: false,
        };
        editor.model_mut().set_selections([selection]);
        cx.notify();
    });
    draw(cx);
}

/// Sends `question` from the composer, and lets the reply land. The
/// composer's focused first, and the active file vetted again as that
/// has it.
fn send(panel: &Entity<ChatPanel>, question: &str, cx: &mut VisualTestContext) {
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |view, cx| view.set_composer_text(question, cx));
    let focus = panel.read_with(cx, gpui::Focusable::focus_handle);
    cx.update(|window, cx| window.focus(&focus, cx));
    draw(cx);
    cx.dispatch_action(SendMessage);
    draw(cx);
}

/// Presses Apply on code block `block` of turn `turn`'s reply.
fn apply(turn: usize, block: usize, cx: &mut VisualTestContext) {
    press_apply(turn, block, cx);
    draw(cx);
}

/// The file the active editor is on.
fn active_path(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    workbench.read_with(cx, |workbench, cx| {
        let editor = workbench.active_editor(cx)?.read(cx);
        editor.model().path().map(Path::to_owned)
    })
}

/// Presses Apply, leaving what it starts to run.
fn press_apply(turn: usize, block: usize, cx: &mut VisualTestContext) {
    let button =
        (cx.debug_bounds(format!("chat-apply-{turn}-{block}").leak())).expect("Apply drawn");
    cx.simulate_click(button.center(), Default::default());
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().buffer().text().to_string())
}

/// The pending hunks of the proposals `editor` shows, and each one's
/// origin.
fn hunks(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(HunkView, Origin)> {
    editor.read_with(cx, |editor, _| {
        let snapshot = editor.model().buffer().snapshot();
        (editor.proposals().iter())
            .flat_map(|proposal| {
                let proposal = proposal.borrow();
                let origin = proposal.origin().clone();
                (proposal.hunks(&snapshot).into_iter())
                    .filter(|hunk| hunk.status == HunkStatus::Pending)
                    .map(move |hunk| (hunk, origin.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    })
}

/// Each pending hunk's old and new text.
fn changes(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(String, String)> {
    (hunks(editor, cx).into_iter())
        .map(|(hunk, _)| (hunk.old_text.to_string(), hunk.new_text.to_string()))
        .collect()
}

/// What the window's shown toasts say, dismissing them.
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

fn chat_id(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> String {
    panel.read_with(cx, |panel, cx| {
        panel.view().read(cx).conversation().id.to_string()
    })
}

fn chat_origin(chat: &str) -> Origin {
    Origin::Ai {
        agent: "fastedit.chat".into(),
        session: chat.into(),
    }
}

#[gpui::test]
fn a_whole_file_block_proposes_its_diff(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let main = open_file(&workbench, &root.join("src/main.go"), cx);
    // Unsaved: what's matched is the buffer's text.
    edit(&main, 0..0, "// unsaved\n", cx);
    let edited = format!("// unsaved\n{MAIN}");
    let whole = "Here:\n\n```go path=src/main.go\n// unsaved\nfunc main() {\n    x := 2\n}\n```\n";
    let panel = open_chat(&workbench, &[whole], cx);
    send(&panel, "Make x 2", cx);
    let chat = chat_id(&panel, cx);

    // Shown away from the file: Apply shows it again.
    open_file(&workbench, &root.join("src/lib.go"), cx);
    apply(0, 0, cx);
    let active = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    assert_eq!(active.as_ref(), Some(&main), "the open buffer's tab");
    assert_eq!(text(&main, cx), edited, "nothing written yet");
    let shown = hunks(&main, cx);
    assert_eq!(shown.len(), 1, "only the changed line");
    let (hunk, origin) = &shown[0];
    assert_eq!(&*hunk.old_text, "    x := 1\n");
    assert_eq!(&*hunk.new_text, "    x := 2\n");
    let at = edited.find("    x := 1").unwrap();
    assert_eq!(hunk.range, at..at + "    x := 1\n".len());
    assert_eq!(*origin, chat_origin(&chat));
    let caret = main.read_with(cx, |main, _| main.model().primary_selection().head());
    assert_eq!(caret, at, "at the first hunk");
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());

    // Accepting it is the user's one transaction, which undo takes back.
    let id = hunk.id;
    main.update(cx, |main, cx| main.accept_hunk(id, cx))
        .unwrap();
    draw(cx);
    let accepted = edited.replace("x := 1", "x := 2");
    assert_eq!(text(&main, cx), accepted);
    let ai_history = main.read_with(cx, |main, _| {
        main.model().buffer().history_bytes(&chat_origin(&chat))
    });
    assert_eq!(ai_history, 0, "no AI edit in the history");
    let focus = main.read_with(cx, |main, cx| main.focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus, cx));
    dispatch_in("editor.undo", &[crate::KEY_CONTEXT, "editor"], cx);
    assert_eq!(text(&main, cx), edited, "one undo takes the hunk back");
    dispatch_in("editor.undo", &[crate::KEY_CONTEXT, "editor"], cx);
    assert_eq!(text(&main, cx), MAIN, "then the user's own edit");
}

#[gpui::test]
fn search_replace_and_selection_blocks_target_their_ranges(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let sections = "\
```go path=src/lib.go
<<<<<<< SEARCH
func b() int {
    return 2
=======
func b() int {
    return 20
>>>>>>> REPLACE
<<<<<<< SEARCH
func a() int {
=======
func a() int64 {
>>>>>>> REPLACE
```
";
    let twice = "\
```go path=src/lib.go
<<<<<<< SEARCH
    return 1
=======
    return 3
>>>>>>> REPLACE
```
";
    let nowhere = "\
```go path=src/lib.go
<<<<<<< SEARCH
func a() int {
=======
func z() int {
>>>>>>> REPLACE
<<<<<<< SEARCH
func d() int {
=======
func e() int {
>>>>>>> REPLACE
```
";
    let overlapping = "\
```go path=src/lib.go
<<<<<<< SEARCH
func b() int {
    return 2
=======
x
>>>>>>> REPLACE
<<<<<<< SEARCH
    return 2
}
=======
y
>>>>>>> REPLACE
```
";
    let selection = "```go\n    y := 2\n    z := 3\n```\n";
    let no_file = "```go\nfunc main() {}\n```\n";
    let replies = [sections, twice, nowhere, overlapping, selection, no_file];
    let panel = open_chat(&workbench, &replies, cx);
    let chat = chat_id(&panel, cx);

    // Not open: read into the active pane, then each section at its match.
    send(&panel, "Change b and a", cx);
    apply(0, 0, cx);
    let lib = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("opened");
    let path = lib.read_with(cx, |lib, _| lib.model().path().map(Path::to_owned));
    assert_eq!(path, Some(root.join("src/lib.go")));
    assert_eq!(
        changes(&lib, cx),
        [
            (
                "func a() int {\n".to_owned(),
                "func a() int64 {\n".to_owned()
            ),
            (
                "func b() int {\n    return 2\n".to_owned(),
                "func b() int {\n    return 20\n".to_owned()
            ),
        ]
    );
    let a = LIB.find("func a").unwrap();
    let ranges: Vec<_> = (hunks(&lib, cx).into_iter())
        .map(|(hunk, origin)| {
            assert_eq!(origin, chat_origin(&chat));
            hunk.range
        })
        .collect();
    let b = LIB.find("func b").unwrap();
    assert_eq!(ranges, [a..a + 15, b..b + 28]);
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());

    // Found twice, or one of two found nowhere: nothing proposed, and a
    // toast says which.
    send(&panel, "Change the 1s", cx);
    apply(1, 0, cx);
    assert_eq!(hunks(&lib, cx).len(), 2, "as it was");
    assert_eq!(
        take_toasts(&workbench, cx),
        ["Can't apply the block to src/lib.go: SEARCH 1 (\"return 1\") is in it more than once"]
    );
    send(&panel, "Rename a and d", cx);
    apply(2, 0, cx);
    assert_eq!(hunks(&lib, cx).len(), 2, "as it was");
    assert_eq!(
        take_toasts(&workbench, cx),
        ["Can't apply the block to src/lib.go: SEARCH 2 (\"func d() int {\") isn't in it"]
    );
    send(&panel, "Overlap", cx);
    apply(3, 0, cx);
    assert_eq!(hunks(&lib, cx).len(), 2, "as it was");
    assert_eq!(
        take_toasts(&workbench, cx),
        ["Can't apply the block to src/lib.go: two of its sections change the same text"]
    );

    // No path: the selection the turn was sent with, where it is by then.
    let main = open_file(&workbench, &root.join("src/main.go"), cx);
    let selected = MAIN.find("    x").unwrap()..MAIN.find('}').unwrap();
    select(&main, selected.clone(), cx);
    send(&panel, "Split x", cx);
    let sent = panel.read_with(cx, |panel, cx| {
        panel.view().read(cx).turn_display(4).unwrap().attachments
    });
    assert_eq!(sent, ["src/main.go:2-2"]);
    // Edits above it, and the caret elsewhere, move nothing it replaces.
    edit(&main, 0..0, "// one\n// two\n", cx);
    select(&main, 0..0, cx);
    apply(4, 0, cx);
    let moved = selected.start + 14..selected.end + 14;
    let shown = hunks(&main, cx);
    assert_eq!(shown.len(), 1);
    let (hunk, origin) = &shown[0];
    assert_eq!(hunk.range, moved);
    assert_eq!(&*hunk.old_text, "    x := 1\n");
    assert_eq!(&*hunk.new_text, "    y := 2\n    z := 3\n");
    assert_eq!(*origin, chat_origin(&chat));

    // No path and no selection.
    select(&main, 0..0, cx);
    send(&panel, "Again", cx);
    apply(5, 0, cx);
    assert_eq!(hunks(&main, cx).len(), 1, "as it was");
    assert_eq!(
        take_toasts(&workbench, cx),
        ["This block doesn't say which file it's for"]
    );
}

#[gpui::test]
fn apply_is_an_ai_invocation(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let seen: Rc<RefCell<Vec<Invocation>>> = Rc::default();
    let observed = seen.clone();
    cx.update(|_, cx| {
        kiln::platform::commands::observe_invocations(cx, move |invocation| {
            observed.borrow_mut().push(invocation.clone());
        });
    });
    let block =
        "```go path=src/main.go\nfunc main() {}\n```\n\n```go path=src/lib.go\npackage lib\n```\n";
    let panel = open_chat(&workbench, &[block], cx);
    // Sent as a key would, through kiln's dispatch, which observers hear.
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |view, cx| view.set_composer_text("Two files", cx));
    let focus = panel.read_with(cx, gpui::Focusable::focus_handle);
    cx.update(|window, cx| {
        window.focus(&focus, cx);
        let send = Invocation::of("chat.send").unwrap();
        kiln::platform::command_actions::dispatch_invocation(send, window, cx).unwrap();
    });
    draw(cx);
    let chat = chat_id(&panel, cx);
    apply(0, 1, cx);
    apply(0, 0, cx);

    let proposed: Vec<Invocation> = (seen.borrow().iter())
        .filter(|invocation| invocation.command.as_str() == "chat.propose_edit")
        .cloned()
        .collect();
    let ai = Actor::Ai {
        agent: "fastedit.chat".to_owned(),
        session: chat.clone(),
    };
    assert_eq!(proposed.len(), 2, "one for each Apply");
    for (invocation, block) in proposed.iter().zip([1, 0]) {
        assert_eq!(
            invocation.args,
            json!({"chat": chat, "turn": 0, "block": block})
        );
        assert_eq!(invocation.actor, ai);
        assert_eq!(invocation.source, Source::Script);
    }
    let sends: Vec<Actor> = (seen.borrow().iter())
        .filter(|invocation| invocation.command.as_str() == "chat.send")
        .map(|invocation| invocation.actor.clone())
        .collect();
    assert_eq!(sends, [Actor::User], "the user's");

    // Each made its proposal.
    let main = open_file(&workbench, &root.join("src/main.go"), cx);
    assert_eq!(hunks(&main, cx).len(), 1);
    let lib = open_file(&workbench, &root.join("src/lib.go"), cx);
    assert_eq!(hunks(&lib, cx).len(), 1);

    // A recorded invocation replays.
    let replay = proposed[1].clone();
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.run_propose_edit(&replay, window, cx))
    });
    draw(cx);
    assert_eq!(hunks(&main, cx).len(), 2, "beside the one shown");
}

#[gpui::test]
fn links_open_files_under_the_root(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let panel = open_chat(&workbench, &[], cx);
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    let click = |path: &str, row, cx: &mut VisualTestContext| {
        let path = path.to_owned();
        view.update(cx, |_, cx| cx.emit(ChatViewEvent::OpenPath { path, row }));
        draw(cx);
    };
    let active = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| {
            let editor = workbench.active_editor(cx)?.read(cx);
            let model = editor.model();
            let row = (model.text())
                .offset_to_point(model.primary_selection().head())
                .row;
            Some((model.path()?.to_owned(), row))
        })
    };

    click("src/lib.go", Some(7), cx);
    assert_eq!(active(cx), Some((root.join("src/lib.go"), 6)));
    click("src/main.go", None, cx);
    assert_eq!(active(cx), Some((root.join("src/main.go"), 0)));

    // Leaving the root does nothing.
    for outside in ["../outside.go", "/etc/hosts"] {
        click(outside, Some(1), cx);
        assert_eq!(active(cx), Some((root.join("src/main.go"), 0)), "{outside}");
    }
}

#[gpui::test]
#[cfg(unix)]
fn blocks_and_links_stay_inside_the_real_root(cx: &mut TestAppContext) {
    // `root` and `outside` side by side, so `../outside` leaves the root.
    let temp = dunce::canonicalize(std::env::temp_dir()).unwrap();
    let base = tempfile::tempdir_in(temp).unwrap();
    let (root, outside) = (base.path().join("root"), base.path().join("outside"));
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.go"), MAIN).unwrap();
    fs::write(root.join("src/lib.go"), LIB).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.go"), "package secret\n").unwrap();
    let link = |target: &str, name: &str| symlink(target, root.join(name)).unwrap();
    // Out of the root: a file link, a folder link, dangling links absolute
    // and relative, one through `..`, one through a linked folder, and a
    // chain ending in one.
    link(outside.join("secret.go").to_str().unwrap(), "link.go");
    link(outside.to_str().unwrap(), "out");
    link(outside.join("missing.go").to_str().unwrap(), "dangling.go");
    link("../outside/missing.go", "up.go");
    link("src/../../outside/missing.go", "down_up.go");
    link("out/missing.go", "via.go");
    link("up.go", "chain.go");
    // In it: a link whose `..` stays inside.
    link("src/../src/lib.go", "inside.go");
    let (workbench, cx) = opened(cx, &root);
    let outward = [
        "link.go",
        "out/secret.go",
        "out/new/file.go",
        "dangling.go",
        "up.go",
        "down_up.go",
        "via.go",
        "chain.go",
    ];
    let blocks: String = (outward.iter().chain(&["inside.go"]))
        .map(|path| format!("```go path={path}\npackage x\n```\n\n"))
        .collect();
    let panel = open_chat(&workbench, &[&blocks], cx);
    send(&panel, "Change them", cx);
    // As Apply does: the transcript scrolls the first blocks away.
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    let apply_block = |block, cx: &mut VisualTestContext| {
        view.update(cx, |_, cx| {
            cx.emit(ChatViewEvent::ApplyBlock { turn: 0, block })
        });
        draw(cx);
    };
    for (block, path) in outward.iter().enumerate() {
        apply_block(block, cx);
        assert_eq!(active_path(&workbench, cx), None, "{path}: nothing opened");
        assert_eq!(
            take_toasts(&workbench, cx),
            [format!(
                "Can't apply the block to {path}: it's outside the folder"
            )]
        );
    }
    assert_eq!(
        fs::read_to_string(outside.join("secret.go")).unwrap(),
        "package secret\n"
    );
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1, "nothing made");
    apply_block(outward.len(), cx);
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());
    let editor = (workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned()))
        .expect("inside.go opened");
    assert_eq!(hunks(&editor, cx).len(), 1);
    // Opened at the target vetted, not through the link: retargeting it
    // now takes nothing elsewhere.
    let target = dunce::canonicalize(root.join("src/lib.go")).unwrap();
    assert_eq!(active_path(&workbench, cx), Some(target.clone()));
    fs::remove_file(root.join("inside.go")).unwrap();
    symlink(outside.join("secret.go"), root.join("inside.go")).unwrap();
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned)),
        Some(target)
    );

    // Nor do links to them open anything; one inside does.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let tabs = pane.read_with(cx, |pane, _| pane.tabs().len());
    for path in outward.iter().chain(&["src/main.go"]) {
        let path = (*path).to_owned();
        view.update(cx, |_, cx| {
            cx.emit(ChatViewEvent::OpenPath { path, row: None })
        });
        draw(cx);
    }
    assert_eq!(active_path(&workbench, cx), Some(root.join("src/main.go")));
    assert_eq!(pane.read_with(cx, |pane, _| pane.tabs().len()), tabs + 1);
}

#[gpui::test]
#[cfg(unix)]
fn an_alias_applies_to_the_open_buffer(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    symlink(root.join("src/main.go"), root.join("alias.go")).unwrap();
    let (workbench, cx) = opened(cx, root);
    let main = open_file(&workbench, &root.join("src/main.go"), cx);
    edit(&main, 0..0, "// unsaved\n", cx);
    let block =
        "```go path=alias.go\n<<<<<<< SEARCH\n// unsaved\n=======\n// kept\n>>>>>>> REPLACE\n```\n";
    let panel = open_chat(&workbench, &[block], cx);
    send(&panel, "Keep it", cx);
    apply(0, 0, cx);
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());
    assert_eq!(
        changes(&main, cx),
        [("// unsaved\n".to_owned(), "// kept\n".to_owned())],
        "the open buffer's unsaved text matched"
    );
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_eq!(
        pane.read_with(cx, |pane, _| pane.tabs().len()),
        1,
        "no second tab"
    );
}

#[gpui::test]
#[cfg(unix)]
fn read_only_files_take_no_proposal(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let lib = root.join("src/lib.go");
    fs::set_permissions(&lib, fs::Permissions::from_mode(0o444)).unwrap();
    let (workbench, cx) = opened(cx, root);
    let block = "```go path=src/lib.go\npackage lib\n```\n";
    let panel = open_chat(&workbench, &[block], cx);
    send(&panel, "Rename it", cx);

    // Not open, then open: refused either way.
    apply(0, 0, cx);
    assert_eq!(active_path(&workbench, cx), None);
    assert_eq!(
        take_toasts(&workbench, cx),
        ["Can't apply the block to src/lib.go: it's read-only"]
    );
    let editor = open_file(&workbench, &lib, cx);
    apply(0, 0, cx);
    assert_eq!(hunks(&editor, cx).len(), 0);
    assert_eq!(
        take_toasts(&workbench, cx),
        ["Can't apply the block to src/lib.go: it's read-only"]
    );
}

#[gpui::test]
fn applies_wait_on_their_own_files(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let blocks =
        "```go path=src/main.go\nfunc main() {}\n```\n\n```go path=src/lib.go\npackage lib\n```\n";
    let panel = open_chat(&workbench, &[blocks], cx);
    send(&panel, "Both", cx);
    // Both pressed before either file's read lands.
    press_apply(0, 0, cx);
    press_apply(0, 1, cx);
    draw(cx);
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());
    let main = open_file(&workbench, &root.join("src/main.go"), cx);
    assert_eq!(hunks(&main, cx).len(), 1);
    let lib = open_file(&workbench, &root.join("src/lib.go"), cx);
    assert_eq!(hunks(&lib, cx).len(), 1);
}

#[gpui::test]
fn a_markdown_preview_gives_way_to_the_proposal(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    fs::write(root.join("notes.md"), "# Notes\n\nOne.\n").unwrap();
    let (workbench, cx) = opened(cx, root);
    let notes = open_file(&workbench, &root.join("notes.md"), cx);
    let focus = notes.read_with(cx, |notes, cx| notes.focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus, cx));
    dispatch_in(
        "markdown.toggle_preview",
        &[crate::KEY_CONTEXT, "editor"],
        cx,
    );
    let in_place = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            crate::workbench_commands::markdown_preview::in_place(&notes, cx).is_some()
        })
    };
    assert!(in_place(cx), "the preview shows in the tab");
    let block = "```markdown path=notes.md\n# Notes\n\nTwo.\n```\n";
    let panel = open_chat(&workbench, &[block], cx);
    send(&panel, "Two", cx);
    apply(0, 0, cx);
    assert!(!in_place(cx), "the text shows");
    assert_eq!(
        changes(&notes, cx),
        [("One.\n".to_owned(), "Two.\n".to_owned())]
    );
}

/// The ranges of the pending hunks `editor` shows.
fn ranges(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<std::ops::Range<usize>> {
    (hunks(editor, cx).into_iter())
        .map(|(hunk, _)| hunk.range)
        .collect()
}

#[gpui::test]
fn a_selection_block_replaces_what_its_turn_sent(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let block = "```go\n    y := 2\n```\n";
    let replies = vec![super::refused(), reply(block), reply(block)];
    let panel = open_chat_with(&workbench, replies, cx);
    let main = open_file(&workbench, &root.join("src/main.go"), cx);
    let row = MAIN.find("    x").unwrap()..MAIN.find('}').unwrap();

    // Sent with the whole row, and refused.
    select(&main, row.clone(), cx);
    send(&panel, "Change x", cx);
    // Another selection on the same row: a retry sends the first again.
    let part = row.start + 4..row.end - 1;
    select(&main, part.clone(), cx);
    let focus = panel.read_with(cx, gpui::Focusable::focus_handle);
    cx.update(|window, cx| window.focus(&focus, cx));
    draw(cx);
    cx.update(|window, cx| window.dispatch_action(Box::new(RetryTurn), cx));
    draw(cx);
    apply(1, 0, cx);
    assert_eq!(
        ranges(&main, cx),
        std::slice::from_ref(&row),
        "what the retry sent"
    );

    // A chip gathered since stands for no earlier turn.
    main.update(cx, |main, cx| main.set_proposals(Vec::new(), cx));
    select(&main, part.clone(), cx);
    send(&panel, "Now this", cx);
    select(&main, 0..4, cx);
    apply(1, 0, cx);
    assert_eq!(
        ranges(&main, cx),
        std::slice::from_ref(&row),
        "still turn 1's"
    );
    main.update(cx, |main, cx| main.set_proposals(Vec::new(), cx));
    apply(2, 0, cx);
    assert_eq!(ranges(&main, cx), [part], "turn 2's own");
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());
}

#[gpui::test]
fn a_selection_cut_to_fit_is_not_replaced(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let long: String = (0..400).map(|row| format!("row {row:04}\n")).collect();
    fs::write(root.join("long.go"), &long).unwrap();
    let (workbench, cx) = opened(cx, root);
    let panel = open_chat(&workbench, &["```go\nshort\n```\n"], cx);
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |view, cx| view.set_context_budget(200, cx));
    let editor = open_file(&workbench, &root.join("long.go"), cx);
    select(&editor, 0..long.len(), cx);
    send(&panel, "Shorten", cx);
    let sent = panel.read_with(cx, |panel, cx| {
        panel.view().read(cx).turn_display(0).unwrap().attachments
    });
    assert_eq!(sent.len(), 1, "{sent:?}");
    let rows = panel.read_with(cx, |panel, cx| {
        panel.view().read(cx).conversation().turns[0].attachments[0].rows
    });
    assert_ne!(
        rows,
        Some(Rows {
            first: 1,
            last: 400
        }),
        "cut to fit"
    );
    apply(0, 0, cx);
    assert_eq!(hunks(&editor, cx).len(), 0);
    let why = "the selection it was sent with was cut to fit, or isn't known";
    assert_eq!(
        take_toasts(&workbench, cx),
        [format!("Can't apply the block to {}: {why}", sent[0])]
    );
}

#[gpui::test]
fn a_retry_replaces_what_it_resent_not_the_same_text_elsewhere(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let text = "foo foo\n";
    fs::write(root.join("foo.go"), text).unwrap();
    let (workbench, cx) = opened(cx, root);
    let replies = vec![super::refused(), reply("```go\nbar\n```\n")];
    let panel = open_chat_with(&workbench, replies, cx);
    let editor = open_file(&workbench, &root.join("foo.go"), cx);

    // The first `foo` sent, and refused; then the second selected, the
    // same text on the same row.
    select(&editor, 0..3, cx);
    send(&panel, "Rename", cx);
    select(&editor, 4..7, cx);
    let focus = panel.read_with(cx, gpui::Focusable::focus_handle);
    cx.update(|window, cx| window.focus(&focus, cx));
    draw(cx);
    let chip = panel.read_with(cx, |panel, cx| {
        let view = panel.view().read(cx);
        (view.chips().iter().map(|chip| chip.item.text.clone())).collect::<Vec<_>>()
    });
    assert_eq!(chip, ["foo"], "the second, rendered as the first was");
    cx.update(|window, cx| window.dispatch_action(Box::new(RetryTurn), cx));
    draw(cx);
    apply(1, 0, cx);
    let resent = 0..3;
    assert_eq!(
        ranges(&editor, cx),
        std::slice::from_ref(&resent),
        "the one the retry resent"
    );
}

#[gpui::test]
fn what_was_asked_under_an_earlier_context_is_dropped(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let block = "```go path=src/main.go\nfunc main() {}\n```\n";
    let panel = open_chat(&workbench, &[block], cx);
    send(&panel, "Shorter", cx);

    // The root's trust changes while the file is being found: an update
    // runs no background work, so the lookup is still pending.
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |_, cx| {
        cx.emit(ChatViewEvent::ApplyBlock { turn: 0, block: 0 })
    });
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    assert_eq!(active_path(&workbench, cx), None, "nothing opened");
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());
    // Asked again, it applies.
    apply(0, 0, cx);
    let main = (workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned()))
        .expect("opened");
    assert_eq!(hunks(&main, cx).len(), 1);

    // A link whose chat is gone by the time it's found opens nothing.
    view.update(cx, |_, cx| {
        let path = "src/lib.go".to_owned();
        cx.emit(ChatViewEvent::OpenPath { path, row: None })
    });
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.start_chat(window, cx)));
    draw(cx);
    assert_eq!(active_path(&workbench, cx), Some(root.join("src/main.go")));
    // As does one whose root's trust changed.
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |_, cx| {
        let path = "src/lib.go".to_owned();
        cx.emit(ChatViewEvent::OpenPath { path, row: None })
    });
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(false, cx));
    draw(cx);
    assert_eq!(active_path(&workbench, cx), Some(root.join("src/main.go")));
}

/// Sets the composer to `question` and dispatches `chat.send` as kiln
/// does, in one App update: no notification in between. The composer's
/// focused first, and the active file vetted again as that has it.
fn send_at_once(panel: &Entity<ChatPanel>, question: &str, cx: &mut VisualTestContext) {
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    let focus = panel.read_with(cx, gpui::Focusable::focus_handle);
    cx.update(|window, cx| window.focus(&focus, cx));
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.set_composer_text(question, cx));
        let send = Invocation::of("chat.send").unwrap();
        kiln::platform::command_actions::dispatch_invocation(send, window, cx).unwrap();
    });
    draw(cx);
}

/// A folder with `foo.go` holding `foo foo`, its editor, and a chat whose
/// first reply is refused and second holds a block with no path.
fn foo_foo(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<ChatPanel>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let dir = project();
    fs::write(dir.path().join("foo.go"), "foo foo\n").unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let replies = vec![super::refused(), reply("```go\nbar\n```\n")];
    let panel = open_chat_with(&workbench, replies, cx);
    let editor = open_file(&workbench, &dir.path().join("foo.go"), cx);
    (dir, workbench, panel, editor, cx)
}

#[gpui::test]
fn a_send_in_one_update_keeps_the_selection_it_sent(cx: &mut TestAppContext) {
    let (_dir, workbench, panel, editor, cx) = foo_foo(cx);
    // The first `foo` sent, set and sent in one update, and refused.
    select(&editor, 0..3, cx);
    send_at_once(&panel, "Rename", cx);
    let turns = panel.read_with(cx, |panel, cx| {
        panel.view().read(cx).conversation().turns.len()
    });
    assert_eq!(turns, 1);
    // The second selected, the same text on the same row, and the same
    // question sent afresh the same way: not a retry of the first.
    select(&editor, 4..7, cx);
    send_at_once(&panel, "Rename", cx);
    apply(1, 0, cx);
    let second = 4..7;
    assert_eq!(
        ranges(&editor, cx),
        std::slice::from_ref(&second),
        "the one this send attached"
    );
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());
}

#[gpui::test]
fn keyboard_and_button_sends_keep_the_selection_they_sent(cx: &mut TestAppContext) {
    let (_dir, workbench, panel, editor, cx) = foo_foo(cx);
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    let focus = panel.read_with(cx, gpui::Focusable::focus_handle);
    let typed = |question: &str, cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.focus(&focus, cx));
        draw(cx);
        cx.simulate_input(question);
    };
    // Typed, and sent as a key bound to `chat.send` sends it (no key is
    // bound yet): the first `foo`, refused.
    select(&editor, 0..3, cx);
    typed("Rename", cx);
    let key = Invocation::of("chat.send")
        .unwrap()
        .source(Source::Keyboard);
    let key = key.context(
        fe_command::ContextStack::new()
            .with(fe_command::ContextFrame::new(crate::KEY_CONTEXT))
            .with(fe_command::ContextFrame::new("chat"))
            .with(fe_command::ContextFrame::new(
                fe_chat_view::COMPOSER_CONTEXT,
            )),
    );
    cx.update(|window, cx| {
        let run = fe_gpui::commands::RunCommand(key);
        window.dispatch_action(Box::new(run), cx)
    });
    draw(cx);
    // Then the second, by the Send button.
    select(&editor, 4..7, cx);
    typed("Rename", cx);
    let button = cx.debug_bounds("chat-send").expect("Send drawn");
    cx.simulate_click(button.center(), Default::default());
    draw(cx);
    let turns = view.read_with(cx, |view, _| view.conversation().turns.len());
    assert_eq!(turns, 2, "both sent");
    apply(1, 0, cx);
    let second = 4..7;
    assert_eq!(ranges(&editor, cx), std::slice::from_ref(&second));
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());
}

/// Types `question` into the focused composer, then sends it as a key
/// bound to `chat.send` would (no key is bound yet).
fn type_and_send_by_key(panel: &Entity<ChatPanel>, question: &str, cx: &mut VisualTestContext) {
    let focus = panel.read_with(cx, gpui::Focusable::focus_handle);
    cx.update(|window, cx| window.focus(&focus, cx));
    draw(cx);
    cx.simulate_input(question);
    let key = (Invocation::of("chat.send").unwrap())
        .source(Source::Keyboard)
        .context(
            fe_command::ContextStack::new()
                .with(fe_command::ContextFrame::new(crate::KEY_CONTEXT))
                .with(fe_command::ContextFrame::new("chat"))
                .with(fe_command::ContextFrame::new(
                    fe_chat_view::COMPOSER_CONTEXT,
                )),
        );
    cx.update(|window, cx| {
        let run = fe_gpui::commands::RunCommand(key);
        window.dispatch_action(Box::new(run), cx)
    });
    draw(cx);
}

#[gpui::test]
fn clearing_a_draft_then_retrying_keeps_the_resent_selection(cx: &mut TestAppContext) {
    let (_dir, workbench, panel, editor, cx) = foo_foo(cx);
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    // The first `foo` sent, and refused; the second selected.
    select(&editor, 0..3, cx);
    send_at_once(&panel, "Rename", cx);
    select(&editor, 4..7, cx);
    let focus = panel.read_with(cx, gpui::Focusable::focus_handle);
    cx.update(|window, cx| window.focus(&focus, cx));
    draw(cx);
    // A draft of the same question set and cleared, then Retry, in one
    // update: still a retry of the first turn.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_composer_text("Rename", cx);
            view.set_composer_text("", cx);
        });
        let retry = Invocation::of("chat.retry").unwrap();
        kiln::platform::command_actions::dispatch_invocation(retry, window, cx).unwrap();
    });
    draw(cx);
    let retried = view.read_with(cx, |view, _| view.retry_of(1));
    assert_eq!(retried, Some(0));
    apply(1, 0, cx);
    let first = 0..3;
    assert_eq!(
        ranges(&editor, cx),
        std::slice::from_ref(&first),
        "the one the retry resent"
    );
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());
}

#[gpui::test]
fn a_fresh_keyboard_send_after_a_refusal_keeps_its_selection(cx: &mut TestAppContext) {
    let (_dir, workbench, panel, editor, cx) = foo_foo(cx);
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    select(&editor, 0..3, cx);
    type_and_send_by_key(&panel, "Rename", cx);
    // The same question about the second `foo`, sent by key again.
    select(&editor, 4..7, cx);
    type_and_send_by_key(&panel, "Rename", cx);
    let sent = view.read_with(cx, |view, _| {
        (view.conversation().turns.len(), view.was_sent(1))
    });
    assert_eq!(sent, (2, true));
    apply(1, 0, cx);
    let second = 4..7;
    assert_eq!(ranges(&editor, cx), std::slice::from_ref(&second));
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());
}
