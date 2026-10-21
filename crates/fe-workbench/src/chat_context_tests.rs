//! The chat's context from the workspace, on temp folders, trusted and
//! not: what the request a [`ScriptedProvider`] records carries, gathered
//! from the open editors and picked as @files.

use std::{fs, os::unix::fs::PermissionsExt as _, path::Path, rc::Rc, sync::Arc};

use fe_buffer::{Origin, ResolvedSelection};
use fe_chat::{BlockKind, ChatEvent, ChatProvider, ChatRequest, ScriptedProvider, StopReason};
use fe_chat_view::SendMessage;
use fe_editor::diagnostics::{Diagnostic, Severity};
use fe_gpui::{DiffView, EditorView};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::{
    command::{Capability, Grants},
    core::secrets::MemoryStore,
};
use serde_json::json;

use super::{ChatPanel, chat, dispatch, dispatch_in, draw, opened, set_source};
use crate::Workbench;

const MAIN: &str = "func main() {\n    x := 1\n}\n";
const LIB: &str = "package main\n";

fn reply() -> Vec<ChatEvent> {
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
            text: "Yes.".to_owned(),
        },
        ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "text", "text": "Yes."}),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        },
    ]
}

/// A folder holding `src/main.go` and `src/lib.go`: Go, which no
/// language server starts for by default.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.go"), MAIN).unwrap();
    fs::write(dir.path().join("src/lib.go"), LIB).unwrap();
    dir
}

/// The chat, open, over a provider that replies to `replies` requests and
/// records them.
fn open_chat(
    workbench: &Entity<Workbench>,
    replies: usize,
    cx: &mut VisualTestContext,
) -> (Entity<ChatPanel>, Arc<ScriptedProvider>) {
    let provider = Arc::new(ScriptedProvider::new((0..replies).map(|_| reply())));
    let shared = provider.clone();
    cx.update(|_, cx| {
        ChatPanel::set_provider_factory(
            cx,
            Rc::new(move |_, _| shared.clone() as Arc<dyn ChatProvider>),
        );
    });
    set_source(Some("key"), &Arc::new(MemoryStore::new()), cx);
    dispatch("chat.open", cx);
    let panel = chat(workbench, cx).expect("made");
    (panel, provider)
}

fn set_trust(workbench: &Entity<Workbench>, trusted: bool, cx: &mut VisualTestContext) {
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(trusted, cx));
    draw(cx);
    assert_eq!(
        workbench.read_with(cx, |workbench, _| workbench.root_trusted()),
        trusted
    );
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

/// Selects `range` of `editor`'s text.
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

/// Inserts `text` at `at` in `editor`'s buffer, unsaved.
fn insert(editor: &Entity<EditorView>, at: usize, text: &str, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        let mut buffer = editor.model_mut().buffer_mut();
        buffer.edit([(at..at, text)], Origin::User);
        drop(buffer);
        cx.notify();
    });
    draw(cx);
}

/// Sends `question` from the composer.
fn send(panel: &Entity<ChatPanel>, question: &str, cx: &mut VisualTestContext) {
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |view, cx| view.set_composer_text(question, cx));
    let focus = panel.read_with(cx, gpui::Focusable::focus_handle);
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.dispatch_action(SendMessage);
    draw(cx);
}

/// The new turn's blocks' texts in `request`, the question last.
fn new_turn(request: &ChatRequest) -> Vec<String> {
    let message = request.messages.last().expect("a message");
    (message.content.iter())
        .map(|block| block["text"].as_str().expect("text").to_owned())
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

/// What the chips say.
fn chips(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    panel.read_with(cx, |panel, cx| {
        let view = panel.view().read(cx);
        (0..view.chips().len())
            .map(|index| view.chip_text(index).unwrap())
            .collect()
    })
}

fn attach_file(panel: &Entity<ChatPanel>, path: &Path, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.attach_file(path.to_owned(), window, cx)
        })
    });
    draw(cx);
}

fn attached(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> Vec<(String, String)> {
    panel.read_with(cx, |panel, cx| {
        (panel.view().read(cx).attachments().iter())
            .map(|item| (item.label.clone(), item.text.clone()))
            .collect()
    })
}

#[gpui::test]
fn trusted_roots_attach_the_workspace_context(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    set_trust(&workbench, true, cx);
    open_file(&workbench, &root.join("src/lib.go"), cx);
    let main = open_file(&workbench, &root.join("src/main.go"), cx);
    let (panel, provider) = open_chat(&workbench, 1, cx);

    // Unsaved, selected and with a problem.
    insert(&main, 0, "// edited\n", cx);
    let text = format!("// edited\n{MAIN}");
    let at = text.find("x :=").unwrap();
    select(&main, at..text.len() - 1, cx);
    main.update(cx, |main, cx| {
        let x = text.find("x :=").unwrap();
        let version = main.model().snapshot().version();
        let mut problem = Diagnostic::new(Severity::Error, "declared and not used: x");
        problem.source = Some("compiler".to_owned());
        let items = vec![(x..x + 1, problem)];
        assert!(
            main.model_mut()
                .apply_diagnostics(1u32, 1u32, version, items, Some(1u32))
        );
        cx.notify();
    });
    draw(cx);

    // Shown as chips before Send.
    assert_eq!(
        chips(&panel, cx),
        [
            "src/main.go:3-4 · ~3",
            "Problems (1) · ~19",
            "src/main.go · ~13",
            "Open editors (2) · ~8",
        ]
    );

    send(&panel, "Is x used?", cx);
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        new_turn(&requests[0]),
        [
            "<selection path=\"src/main.go\" rows=\"3-4\">\nx := 1\n}\n</selection>".to_owned(),
            "<diagnostics>\nsrc/main.go:3: error: declared and not used: x (compiler)\n</diagnostics>"
                .to_owned(),
            format!("<file path=\"src/main.go\">\n{text}</file>"),
            "<open-editors>\nsrc/lib.go\nsrc/main.go\n</open-editors>".to_owned(),
            "Is x used?".to_owned(),
        ]
    );
    assert_eq!(fs::read_to_string(root.join("src/main.go")).unwrap(), MAIN);

    // Another tab active: its file, with no selection nor problems.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    draw(cx);
    assert_eq!(
        chips(&panel, cx),
        ["src/lib.go · ~5", "Open editors (2) · ~8"]
    );
}

#[gpui::test]
fn untrusted_roots_attach_only_what_the_user_chose(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    set_trust(&workbench, false, cx);
    open_file(&workbench, &root.join("src/lib.go"), cx);
    let main = open_file(&workbench, &root.join("src/main.go"), cx);
    let (panel, provider) = open_chat(&workbench, 2, cx);
    select(&main, 0..2, cx);

    // A file picked from quick open, which `@` in the composer opens.
    cx.simulate_input("@");
    draw(cx);
    let composed = panel.read_with(cx, |panel, cx| panel.view().read(cx).composer_text(cx));
    assert_eq!(composed, "");
    cx.simulate_input("lib.go");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        attached(&panel, cx),
        [("src/lib.go".to_owned(), LIB.to_owned())]
    );

    send(&panel, "What's this?", cx);
    let requests = provider.requests();
    assert_eq!(
        new_turn(&requests[0]),
        [
            "<selection path=\"src/main.go\" rows=\"1-1\">\nfu\n</selection>".to_owned(),
            format!("<file path=\"src/lib.go\">\n{LIB}</file>"),
            "What's this?".to_owned(),
        ]
    );

    // Without buffer.read, nothing: neither gathered nor picked.
    let net = Capability::parse("net:api.anthropic.com").unwrap();
    panel.update(cx, |panel, cx| {
        panel.set_grants([net].into_iter().collect::<Grants>(), cx);
        panel.gather(cx);
    });
    attach_file(&panel, &root.join("src/lib.go"), cx);
    assert_eq!(attached(&panel, cx), []);
    send(&panel, "And now?", cx);
    let requests = provider.requests();
    assert_eq!(new_turn(&requests[1]), ["And now?"]);
}

#[gpui::test]
fn at_file_refuses_secrets_and_large_files(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("notes.txt"), "secret\n").unwrap();
    for name in [
        ".env",
        ".env.local",
        "server.pem",
        "server.key",
        "id_ed25519",
    ] {
        fs::write(root.join(name), "KEY=1\n").unwrap();
    }
    fs::create_dir(root.join(".git")).unwrap();
    fs::write(root.join(".git/config"), "[core]\n").unwrap();
    fs::write(root.join("big.txt"), "x".repeat(2 << 20)).unwrap();
    fs::write(root.join("image.png"), b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
    std::os::unix::fs::symlink(outside.path().join("notes.txt"), root.join("notes.txt")).unwrap();
    let (workbench, cx) = opened(cx, root);
    let (panel, _) = open_chat(&workbench, 0, cx);
    take_toasts(&workbench, cx);

    let refusals = [
        (".env", "it may hold secrets"),
        (".env.local", "it may hold secrets"),
        ("server.pem", "it may hold secrets"),
        ("server.key", "it may hold secrets"),
        ("id_ed25519", "it may hold secrets"),
        (".git/config", "it's in .git"),
        ("big.txt", "it's over 1 MiB"),
        ("image.png", "it's binary"),
        ("notes.txt", "it's outside the folder"),
        ("src/../../elsewhere.rs", "it's outside the folder"),
    ];
    for (name, reason) in refusals {
        attach_file(&panel, &root.join(name), cx);
        assert_eq!(attached(&panel, cx), [], "{name}");
        assert_eq!(
            take_toasts(&workbench, cx),
            [format!("Can't attach {name}: {reason}")],
        );
    }
    let notes = outside.path().join("notes.txt");
    attach_file(&panel, &notes, cx);
    assert_eq!(attached(&panel, cx), []);
    assert_eq!(
        take_toasts(&workbench, cx),
        [format!(
            "Can't attach {}: it's outside the folder",
            notes.display()
        )],
    );

    // An open file's unsaved text is what's attached.
    let lib = open_file(&workbench, &root.join("src/lib.go"), cx);
    insert(&lib, 0, "// unsaved\n", cx);
    attach_file(&panel, &root.join("src/lib.go"), cx);
    assert_eq!(
        attached(&panel, cx),
        [("src/lib.go".to_owned(), format!("// unsaved\n{LIB}"))]
    );
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());
}

#[gpui::test]
fn automatic_context_skips_links_out_of_the_folder_and_to_secrets(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("notes.txt"), "private\n").unwrap();
    fs::create_dir(root.join(".git")).unwrap();
    fs::write(root.join(".git/config"), "[core]\n").unwrap();
    std::os::unix::fs::symlink(outside.path().join("notes.txt"), root.join("notes.txt")).unwrap();
    std::os::unix::fs::symlink(root.join(".git/config"), root.join("config.txt")).unwrap();
    let (workbench, cx) = opened(cx, root);
    set_trust(&workbench, true, cx);
    let (panel, provider) = open_chat(&workbench, 2, cx);

    // Each a link out: neither its text nor its selection goes.
    for (name, question) in [("notes.txt", "Notes?"), ("config.txt", "Config?")] {
        let editor = open_file(&workbench, &root.join(name), cx);
        select(&editor, 0..3, cx);
        send(&panel, question, cx);
        let requests = provider.requests();
        let sent = new_turn(requests.last().unwrap());
        assert_eq!(sent.len(), 2, "{name}: {sent:?}");
        assert!(sent[0].starts_with("<open-editors>"), "{name}: {sent:?}");
        assert_eq!(sent[1], question);
    }

    // A file in the folder still gives its text.
    open_file(&workbench, &root.join("src/main.go"), cx);
    let shown = chips(&panel, cx);
    assert!(shown.contains(&"src/main.go · ~9".to_owned()), "{shown:?}");
}

#[gpui::test]
fn at_files_wait_for_their_reads_and_keep_their_places(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let (panel, provider) = open_chat(&workbench, 1, cx);

    // Picked in an order, attached in that order, whichever read ends
    // first; Send waits for both.
    let attach_now = |path: &Path, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.attach_file(path.to_owned(), window, cx)
            })
        });
    };
    attach_now(&root.join("src/main.go"), cx);
    attach_now(&root.join("src/lib.go"), cx);
    let reading = panel.read_with(cx, |panel, cx| panel.view().read(cx).reading());
    assert!(reading);
    let chat_view = panel.read_with(cx, |panel, _| panel.view().clone());
    chat_view.update(cx, |view, cx| view.set_composer_text("Both?", cx));
    cx.update(|window, cx| window.dispatch_action(Box::new(SendMessage), cx));
    assert_eq!(provider.requests().len(), 0, "not while reading");
    draw(cx);
    assert_eq!(
        attached(&panel, cx),
        [
            ("src/main.go".to_owned(), MAIN.to_owned()),
            ("src/lib.go".to_owned(), LIB.to_owned()),
        ]
    );
    send(&panel, "Both?", cx);
    let requests = provider.requests();
    assert_eq!(new_turn(&requests[0]).len(), 3);

    // A read that ends after New Chat lands nowhere.
    let before = chat_view;
    attach_now(&root.join("src/lib.go"), cx);
    dispatch_in("chat.new", &[crate::KEY_CONTEXT, "chat"], cx);
    draw(cx);
    assert_ne!(panel.read_with(cx, |panel, _| panel.view().clone()), before);
    assert_eq!(attached(&panel, cx), []);
    assert!(!panel.read_with(cx, |panel, cx| panel.view().read(cx).reading()));
}

#[gpui::test]
fn at_file_takes_an_open_buffer_however_its_open(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    std::os::unix::fs::symlink(root.join("src/lib.go"), root.join("alias.go")).unwrap();
    let (workbench, cx) = opened(cx, root);
    let (panel, _) = open_chat(&workbench, 0, cx);

    // Through another path to it, and with the disk's unreadable.
    let lib = open_file(&workbench, &root.join("src/lib.go"), cx);
    insert(&lib, 0, "// unsaved\n", cx);
    attach_file(&panel, &root.join("alias.go"), cx);
    let unsaved = format!("// unsaved\n{LIB}");
    assert_eq!(
        attached(&panel, cx),
        [("alias.go".to_owned(), unsaved.clone())]
    );
    let file = root.join("src/lib.go");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
    attach_file(&panel, &file, cx);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(attached(&panel, cx)[1], ("src/lib.go".to_owned(), unsaved));
    assert_eq!(take_toasts(&workbench, cx), Vec::<String>::new());

    // Open only as a diff's working side.
    let main = root.join("src/main.go");
    open_file(&workbench, &main, cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| window.focus(&pane.focus_handle(cx), cx));
    dispatch("diff.compare_file", cx);
    cx.simulate_input("lib.go");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    let diff = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.active_item()?.downcast_ref::<DiffView>().cloned()
    });
    let right = diff
        .expect("a diff")
        .read_with(cx, |diff, _| diff.right().clone());
    for index in (0..2).rev() {
        cx.update(|window, cx| {
            pane.update(cx, |pane, cx| {
                pane.activate(index, window, cx);
                pane.close_active(window, cx);
            })
        });
    }
    draw(cx);
    let tabs = workbench.read_with(cx, |workbench, cx| {
        (workbench.open_editors(cx).iter())
            .filter_map(|editor| editor.read(cx).model().path().map(Path::to_owned))
            .collect::<Vec<_>>()
    });
    assert!(!tabs.contains(&main), "only the diff has it open: {tabs:?}");
    insert(&right, 0, "// in the diff\n", cx);
    attach_file(&panel, &main, cx);
    assert_eq!(
        attached(&panel, cx)[2],
        ("src/main.go".to_owned(), format!("// in the diff\n{MAIN}"))
    );
}

/// Replaces `editor`'s text with `text`, as a reload from disk would.
fn reload(editor: &Entity<EditorView>, text: &str, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        let mut buffer = editor.model_mut().buffer_mut();
        let len = buffer.text().len();
        buffer.edit([(0..len, text)], Origin::User);
        drop(buffer);
        cx.notify();
    });
}

#[gpui::test]
fn automatic_context_is_vetted_again_as_its_file_changes(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "private\n").unwrap();
    fs::write(root.join("notes.txt"), "fine\n").unwrap();
    let (workbench, cx) = opened(cx, root);
    set_trust(&workbench, true, cx);
    let (panel, _) = open_chat(&workbench, 0, cx);
    let notes = open_file(&workbench, &root.join("notes.txt"), cx);
    assert!(chips(&panel, cx).contains(&"notes.txt · ~2".to_owned()));

    // Replaced on disk by a link out of the folder, then read again: its
    // new text never shows.
    fs::remove_file(root.join("notes.txt")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret.txt"), root.join("notes.txt")).unwrap();
    reload(&notes, "private\n", cx);
    // Withheld while it's vetted again, not kept as it was.
    let context = panel.read_with(cx, |panel, cx| panel.view().read(cx).context().to_vec());
    assert!(
        context
            .iter()
            .all(|item| !item.label.starts_with("notes.txt")),
        "{context:?}"
    );
    draw(cx);
    let shown = chips(&panel, cx);
    assert!(
        !shown.iter().any(|chip| chip.starts_with("notes.txt")),
        "{shown:?}"
    );

    // The root changed while a file was being vetted: vetted again under
    // the new root, which it's outside of.
    let main = open_file(&workbench, &root.join("src/main.go"), cx);
    assert!(chips(&panel, cx).contains(&"src/main.go · ~9".to_owned()));
    // A file that's fine: withheld while its new text is vetted, then back.
    reload(&main, "// fine\n", cx);
    let held = chips(&panel, cx);
    assert!(
        !held.iter().any(|chip| chip.starts_with("src/main.go")),
        "{held:?}"
    );
    draw(cx);
    assert!(chips(&panel, cx).contains(&"src/main.go · ~3".to_owned()));
    let other = tempfile::tempdir().unwrap();
    insert(&main, 0, "// edited\n", cx);
    reload(&main, "// changed\n", cx);
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.set_root(other.path().to_owned(), window, cx)
        })
    });
    draw(cx);
    let shown = chips(&panel, cx);
    assert!(
        !shown.iter().any(|chip| chip.starts_with("src/main.go")),
        "{shown:?}"
    );
    let context = panel.read_with(cx, |panel, cx| panel.view().read(cx).context().to_vec());
    assert!(context.iter().all(|item| !item.text.contains("// changed")));
}

#[gpui::test]
fn dangling_links_are_vetted_by_their_targets(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.txt");
    fs::write(&secret, "private\n").unwrap();
    fs::write(root.join(".env"), "KEY=1\n").unwrap();
    std::os::unix::fs::symlink(&secret, root.join("out.txt")).unwrap();
    std::os::unix::fs::symlink(root.join(".env"), root.join("env.txt")).unwrap();
    let (workbench, cx) = opened(cx, root);
    set_trust(&workbench, true, cx);
    let (panel, _) = open_chat(&workbench, 0, cx);
    let out = open_file(&workbench, &root.join("out.txt"), cx);
    let env = open_file(&workbench, &root.join("env.txt"), cx);

    // Their targets gone, the buffers kept.
    fs::remove_file(&secret).unwrap();
    fs::remove_file(root.join(".env")).unwrap();
    for (editor, name, reason) in [
        (&out, "out.txt", "it's outside the folder"),
        (&env, "env.txt", "it may hold secrets"),
    ] {
        let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
        let index = pane.read_with(cx, |pane, _| {
            (pane.tabs().iter())
                .position(|tab| tab.editor() == Some(editor))
                .unwrap()
        });
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(index, window, cx)));
        insert(editor, 0, "x", cx);
        let shown = chips(&panel, cx);
        assert!(
            !shown.iter().any(|chip| chip.starts_with(name)),
            "{shown:?}"
        );
        attach_file(&panel, &root.join(name), cx);
        assert_eq!(attached(&panel, cx), []);
        assert_eq!(
            take_toasts(&workbench, cx),
            [format!("Can't attach {name}: {reason}")]
        );
    }
}

#[gpui::test]
fn at_file_keeps_a_buffer_whose_editor_closes_meanwhile(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let (panel, _) = open_chat(&workbench, 0, cx);
    let lib = open_file(&workbench, &root.join("src/lib.go"), cx);
    insert(&lib, 0, "// unsaved\n", cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| window.focus(&pane.focus_handle(cx), cx));
    dispatch("pane.split_right", cx);

    // The first editor closes while the file's being matched; the split
    // keeps the buffer.
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.attach_file(root.join("src/lib.go"), window, cx)
        });
        pane.update(cx, |pane, cx| pane.close_active(window, cx));
    });
    draw(cx);
    assert!(lib.read_with(cx, |_, _| true));
    let open = workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx).len());
    assert_eq!(open, 1, "the split's");
    assert_eq!(
        attached(&panel, cx),
        [("src/lib.go".to_owned(), format!("// unsaved\n{LIB}"))]
    );
}

#[gpui::test]
fn revoking_trust_cuts_the_context_before_a_pending_vetting_ends(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    set_trust(&workbench, true, cx);
    open_file(&workbench, &root.join("src/lib.go"), cx);
    let main = open_file(&workbench, &root.join("src/main.go"), cx);
    let (panel, provider) = open_chat(&workbench, 1, cx);
    assert_eq!(
        chips(&panel, cx),
        ["src/main.go · ~9", "Open editors (2) · ~8"]
    );

    // The file changes, and while it's vetted again trust is revoked and
    // the message sent: none of the trusted root's context goes.
    reload(&main, &format!("// changed\n{MAIN}"), cx);
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(false, cx));
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |view, cx| view.set_composer_text("Still?", cx));
    let focus = panel.read_with(cx, gpui::Focusable::focus_handle);
    cx.update(|window, cx| {
        window.focus(&focus, cx);
        window.dispatch_action(Box::new(SendMessage), cx);
    });
    draw(cx);
    let requests = provider.requests();
    assert_eq!(new_turn(&requests[0]), ["Still?"]);
    assert_eq!(chips(&panel, cx), Vec::<String>::new());
}

#[gpui::test]
fn links_through_a_missing_folder_are_refused(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "private\n").unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret.txt"), root.join("public.txt")).unwrap();
    fs::create_dir(root.join("gone")).unwrap();
    for alias in ["alias.txt", "unopened.txt"] {
        std::os::unix::fs::symlink("gone/../public.txt", root.join(alias)).unwrap();
    }
    let (workbench, cx) = opened(cx, root);
    set_trust(&workbench, true, cx);
    let (panel, _) = open_chat(&workbench, 0, cx);
    let alias = open_file(&workbench, &root.join("alias.txt"), cx);
    take_toasts(&workbench, cx);

    // `gone` gone: where the link leads can't be known.
    fs::remove_dir(root.join("gone")).unwrap();
    insert(&alias, 0, "x", cx);
    let shown = chips(&panel, cx);
    assert!(
        !shown.iter().any(|chip| chip.starts_with("alias.txt")),
        "{shown:?}"
    );
    // Neither its open buffer nor, through another such link, the disk.
    let reason = "a link leads through a folder that doesn't exist";
    for name in ["alias.txt", "unopened.txt"] {
        attach_file(&panel, &root.join(name), cx);
        assert_eq!(attached(&panel, cx), [], "{name}");
        assert_eq!(
            take_toasts(&workbench, cx),
            [format!("Can't attach {name}: {reason}")]
        );
    }
}

#[gpui::test]
fn a_buffer_counts_only_for_the_file_its_text_came_from(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "private\n").unwrap();
    fs::write(root.join("ok.txt"), "ok\n").unwrap();
    fs::write(root.join("notes.txt"), "fine\n").unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret.txt"), root.join("alias.txt")).unwrap();
    let (workbench, cx) = opened(cx, root);
    set_trust(&workbench, true, cx);
    let (panel, _) = open_chat(&workbench, 0, cx);
    let alias = open_file(&workbench, &root.join("alias.txt"), cx);
    insert(&alias, 0, "x", cx);
    take_toasts(&workbench, cx);

    // Retargeted to a file that's fine, the buffer keeps the secret's text:
    // it never goes.
    fs::remove_file(root.join("alias.txt")).unwrap();
    std::os::unix::fs::symlink(root.join("ok.txt"), root.join("alias.txt")).unwrap();
    insert(&alias, 0, "y", cx);
    let shown = chips(&panel, cx);
    assert!(
        !shown.iter().any(|chip| chip.starts_with("alias.txt")),
        "{shown:?}"
    );
    attach_file(&panel, &root.join("alias.txt"), cx);
    assert_eq!(attached(&panel, cx), []);
    assert_eq!(
        take_toasts(&workbench, cx),
        ["Can't attach alias.txt: its text isn't from this file"]
    );

    // Nor a buffer of a file since replaced by another.
    let notes = open_file(&workbench, &root.join("notes.txt"), cx);
    assert!(chips(&panel, cx).contains(&"notes.txt · ~2".to_owned()));
    fs::remove_file(root.join("notes.txt")).unwrap();
    fs::write(root.join("notes.txt"), "other\n").unwrap();
    insert(&notes, 0, "z", cx);
    let shown = chips(&panel, cx);
    assert!(
        !shown.iter().any(|chip| chip.starts_with("notes.txt")),
        "{shown:?}"
    );
}

#[gpui::test]
fn a_file_swapped_after_it_is_vetted_is_never_what_is_read(cx: &mut TestAppContext) {
    use std::os::unix::fs::MetadataExt as _;

    use super::super::chat_context::{Source, find_source_with, vet_text};
    let dir = project();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.txt");
    fs::write(&secret, "private\n").unwrap();
    let lib = root.join("src/lib.go");
    let restore = || {
        fs::remove_file(&lib).unwrap();
        fs::write(&lib, LIB).unwrap();
    };
    let to_secret = || {
        fs::remove_file(&lib).unwrap();
        std::os::unix::fs::symlink(&secret, &lib).unwrap();
    };

    // Read from disk: what was vetted, whatever's at its path since.
    let found = find_source_with(root, &lib, &[], to_secret);
    assert!(matches!(found, Ok(Source::Disk(Some(text))) if text == LIB));
    restore();

    // A buffer holding the secret's text, stamped with its file: the
    // path, a file that's fine as it's vetted, then made a link to the
    // secret, still isn't the file the buffer read.
    let meta = fs::metadata(&secret).unwrap();
    let stamp = fe_buffer::DiskStamp::from_metadata(&meta).unwrap();
    assert_eq!((stamp.dev, stamp.ino), (meta.dev(), meta.ino()));
    let vetted = vet_text(root, &lib, Some(&stamp), to_secret);
    assert_eq!(
        vetted.err().as_deref(),
        Some("its text isn't from this file")
    );
    restore();
    let open = [(lib.clone(), Some(stamp))];
    let found = find_source_with(root, &lib, &open, to_secret);
    assert_eq!(
        found.err().as_deref(),
        Some("its text isn't from this file")
    );
    restore();
    let _ = cx;
}

#[gpui::test]
fn at_file_refuses_a_buffer_replaced_before_it_lands(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = opened(cx, root);
    let (panel, _) = open_chat(&workbench, 0, cx);
    let lib = open_file(&workbench, &root.join("src/lib.go"), cx);
    take_toasts(&workbench, cx);

    // Read again, or saved, while it's matched: refused.
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.attach_file(root.join("src/lib.go"), window, cx)
        });
        lib.update(cx, |lib, _| {
            lib.model_mut().buffer_mut().set_disk_stamp(None);
        });
    });
    draw(cx);
    assert_eq!(attached(&panel, cx), []);
    assert_eq!(
        take_toasts(&workbench, cx),
        ["Can't attach src/lib.go: it changed while it was read"]
    );
}

#[gpui::test]
fn at_files_go_when_the_root_or_buffer_read_does(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let other = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, root);
    let (panel, _) = open_chat(&workbench, 0, cx);
    let attach_now = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.attach_file(root.join("src/lib.go"), window, cx)
            })
        });
    };
    let set_root = |path: &Path, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.set_root(path.to_owned(), window, cx)
            })
        });
    };
    let reading = |cx: &mut VisualTestContext| {
        panel.read_with(cx, |panel, cx| panel.view().read(cx).reading())
    };

    // The root changes while it's read, or once it's attached.
    attach_now(cx);
    assert!(reading(cx));
    set_root(other.path(), cx);
    draw(cx);
    assert_eq!(attached(&panel, cx), []);
    assert!(!reading(cx));
    set_root(root, cx);
    draw(cx);
    attach_file(&panel, &root.join("src/lib.go"), cx);
    assert_eq!(attached(&panel, cx).len(), 1);
    set_root(other.path(), cx);
    assert_eq!(attached(&panel, cx), []);
    set_root(root, cx);
    draw(cx);

    // `buffer.read` goes while it's read, or once it's attached.
    let net = Capability::parse("net:api.anthropic.com").unwrap();
    let without = || [net.clone()].into_iter().collect::<Grants>();
    attach_now(cx);
    panel.update(cx, |panel, cx| panel.set_grants(without(), cx));
    draw(cx);
    assert_eq!(attached(&panel, cx), []);
    assert!(!reading(cx));
    panel.update(cx, |panel, cx| {
        panel.set_grants(super::super::chat_grants(), cx)
    });
    attach_file(&panel, &root.join("src/lib.go"), cx);
    assert_eq!(attached(&panel, cx).len(), 1);
    panel.update(cx, |panel, cx| panel.set_grants(without(), cx));
    assert_eq!(attached(&panel, cx), []);
}

fn refusal() -> Vec<ChatEvent> {
    vec![
        ChatEvent::Started {
            model: "claude-opus-5-5".to_owned(),
            message_id: "msg_r".to_owned(),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::Refusal,
            stop_details: None,
        },
    ]
}

fn too_large() -> Vec<ChatEvent> {
    let error = fe_chat::ProviderError::new(fe_chat::ErrorKind::TooLarge, "request too large");
    vec![ChatEvent::Failed(error)]
}

#[gpui::test]
fn earlier_at_files_dont_come_back_once_their_authority_goes(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let other = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, root);
    let provider = Arc::new(ScriptedProvider::new([
        refusal(),
        reply(),
        too_large(),
        reply(),
    ]));
    let shared = provider.clone();
    cx.update(|_, cx| {
        ChatPanel::set_provider_factory(
            cx,
            Rc::new(move |_, _| shared.clone() as Arc<dyn ChatProvider>),
        );
    });
    set_source(Some("key"), &Arc::new(MemoryStore::new()), cx);
    dispatch("chat.open", cx);
    let panel = chat(&workbench, cx).expect("made");
    let click = |selector: &'static str, cx: &mut VisualTestContext| {
        let bounds = cx.debug_bounds(selector).expect("drawn");
        cx.simulate_click(bounds.center(), Default::default());
        draw(cx);
    };

    // Refused, then the root changes: Edit brings the @file back no more.
    attach_file(&panel, &root.join("src/lib.go"), cx);
    send(&panel, "One?", cx);
    assert_eq!(new_turn(&provider.requests()[0]).len(), 2);
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.set_root(other.path().to_owned(), window, cx)
        })
    });
    draw(cx);
    click("chat-banner-action-0", cx);
    assert_eq!(attached(&panel, cx), []);
    send(&panel, "One again?", cx);
    assert_eq!(new_turn(&provider.requests()[1]), ["One again?"]);

    // Too large, then `buffer.read` goes: nor does Remove Attachments.
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.set_root(root.to_owned(), window, cx)
        })
    });
    draw(cx);
    attach_file(&panel, &root.join("src/lib.go"), cx);
    send(&panel, "Two?", cx);
    assert_eq!(new_turn(&provider.requests()[2]).len(), 2);
    let net = Capability::parse("net:api.anthropic.com").unwrap();
    panel.update(cx, |panel, cx| {
        panel.set_grants([net].into_iter().collect::<Grants>(), cx)
    });
    draw(cx);
    click("chat-banner-action-2", cx);
    assert_eq!(attached(&panel, cx), []);
    send(&panel, "Two again?", cx);
    assert_eq!(new_turn(&provider.requests()[3]), ["Two again?"]);
}
