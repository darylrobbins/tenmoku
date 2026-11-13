//! [`Workbench::ask_agent`]: the chat opened with items attached and a
//! prompt, sent only when the chat can. The provider is the chat's
//! scripted one: no network.

use std::{rc::Rc, sync::Arc};

use fe_chat::{
    BlockKind, ChatEvent, ChatProvider, ContextItem, ContextKind, Rows, ScriptedProvider,
    StopReason,
};
use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln::core::secrets::{MemoryStore, SecretStore as _};
use kiln_l10n::tr;
use serde_json::json;

use crate::{
    Launch, Workbench,
    chat_panel::agent_actions::{AgentAsk, rows_item, text_item},
    chat_panel::{ChatPanel, KeySource},
    tests::open,
};

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
            text: "ok".to_owned(),
        },
        ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "text", "text": "ok"}),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        },
    ]
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A workbench on an empty folder, trusted or not, with a chat over a
/// provider that answers once, from a key or none.
fn setup(
    cx: &mut TestAppContext,
    key: bool,
    trusted: bool,
) -> (
    Entity<Workbench>,
    &mut VisualTestContext,
    Arc<ScriptedProvider>,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let launch = Launch {
        root: Some(root),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    let provider = Arc::new(ScriptedProvider::new([reply()]));
    let shared = provider.clone();
    cx.update(|window, cx| {
        window.activate_window();
        crate::chat_panel::ChatPanel::set_provider_factory(
            cx,
            Rc::new(move |_, _| shared.clone() as Arc<dyn ChatProvider>),
        );
        cx.set_global(KeySource {
            env: key.then(|| "key".to_owned()),
            store: Arc::new(MemoryStore::new()),
        });
    });
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(trusted, cx));
    draw(cx);
    (workbench, cx, provider, dir)
}

fn ask(
    workbench: &Entity<Workbench>,
    ask: AgentAsk,
    cx: &mut VisualTestContext,
) -> Entity<ChatPanel> {
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.ask_agent(ask, window, cx)
    });
    draw(cx);
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).chat().cloned().expect("made")
    })
}

fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

fn composer(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> String {
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.read_with(cx, |view, cx| view.composer_text(cx))
}

fn attached(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> Vec<ContextItem> {
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.read_with(cx, |view, _| view.attachments())
}

/// An ask of files, and of `output`.
fn the_ask_of(send: bool, context: Vec<ContextItem>, output: Vec<ContextItem>) -> AgentAsk {
    AgentAsk {
        prompt: "Why did this fail?".to_owned(),
        context,
        output,
        send,
    }
}

fn the_ask(send: bool, context: Vec<ContextItem>) -> AgentAsk {
    the_ask_of(send, context, vec![])
}

/// An ask of output only.
fn output_ask(send: bool, output: Vec<ContextItem>) -> AgentAsk {
    the_ask_of(send, vec![], output)
}

#[gpui::test]
fn ask_agent_attaches_items_and_fills_the_composer(cx: &mut TestAppContext) {
    let (workbench, cx, provider, _dir) = setup(cx, true, true);
    let out = text_item("Test output", "assertion failed");
    let rows = rows_item("src/a.rs", Rows { first: 3, last: 4 }, "x\ny\n");
    let panel = ask(
        &workbench,
        the_ask_of(false, vec![rows.clone()], vec![out.clone()]),
        cx,
    );
    assert_eq!(attached(&panel, cx), vec![rows, out]);
    assert_eq!(composer(&panel, cx), "Why did this fail?");
    assert!(provider.requests().is_empty());
    let (shown, panel_now) = workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        (dock.is_visible(), dock.active())
    });
    assert!(shown);
    assert_eq!(panel_now, crate::DockPanel::Chat);
}

#[gpui::test]
fn send_true_sends_when_a_key_is_set(cx: &mut TestAppContext) {
    let (workbench, cx, provider, _dir) = setup(cx, true, true);
    let item = text_item("Test output", "assertion failed");
    let panel = ask(&workbench, output_ask(true, vec![item]), cx);
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    let sent = format!("{:?}", requests[0].messages.last().unwrap().content);
    assert!(sent.contains("Why did this fail?"), "{sent}");
    assert!(sent.contains("assertion failed"), "{sent}");
    assert_eq!(composer(&panel, cx), "");
    assert!(attached(&panel, cx).is_empty());
}

#[gpui::test]
fn with_no_key_the_draft_stays_and_a_toast_says_why(cx: &mut TestAppContext) {
    let (workbench, cx, provider, _dir) = setup(cx, false, true);
    let panel = ask(
        &workbench,
        output_ask(true, vec![text_item("Out", "x")]),
        cx,
    );
    assert!(provider.requests().is_empty());
    assert_eq!(composer(&panel, cx), "Why did this fail?");
    assert_eq!(attached(&panel, cx).len(), 1);
    assert_eq!(
        toasts(&workbench, cx),
        vec![tr!("workbench-chat-ask-no-key").to_string()]
    );
}

/// Rows of the file at `path`, with `text`.
fn file(path: &str) -> ContextItem {
    rows_item(path, Rows { first: 1, last: 1 }, "hunter2")
}

#[gpui::test]
fn denied_files_are_refused_as_attachments(cx: &mut TestAppContext) {
    let (workbench, cx, _provider, dir) = setup(cx, true, true);
    let root = dunce::canonicalize(dir.path()).unwrap();
    std::fs::write(root.join("ok.txt"), "fine").unwrap();
    #[cfg(unix)]
    {
        std::fs::write(root.join(".env"), "x").unwrap();
        std::os::unix::fs::symlink(root.join(".env"), root.join("link.txt")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("o.txt"), "x").unwrap();
        std::os::unix::fs::symlink(outside.path().join("o.txt"), root.join("out.txt")).unwrap();
    }
    let ok = file("ok.txt");
    let whole = |path: &str| ContextItem {
        kind: ContextKind::File {
            path: path.to_owned(),
            rows: None,
        },
        label: path.to_owned(),
        text: "hunter2".to_owned(),
    };
    let mut items = vec![
        file(".env"),
        whole(".env"),
        whole(".git/config"),
        ok.clone(),
    ];
    #[cfg(unix)]
    items.extend([file("link.txt"), whole("link.txt"), whole("out.txt")]);
    let panel = ask(&workbench, the_ask(false, items), cx);
    assert_eq!(attached(&panel, cx), vec![ok]);
    let said = toasts(&workbench, cx);
    // The toast layer shows at most three.
    assert_eq!(said.len(), 3, "{said:?}");
    let reason = tr!("workbench-chat-why-secrets").to_string();
    assert_eq!(
        said[0],
        tr!("workbench-chat-cant-attach", path = ".env", reason = reason).to_string()
    );
}

#[gpui::test]
fn text_items_are_not_files(cx: &mut TestAppContext) {
    let (workbench, cx, _provider, _dir) = setup(cx, true, true);
    let item = text_item("id_check", "output");
    let panel = ask(&workbench, output_ask(false, vec![item.clone()]), cx);
    assert_eq!(attached(&panel, cx), vec![item]);
}

#[gpui::test]
fn text_items_attach_in_a_window_with_no_folder(cx: &mut TestAppContext) {
    let (workbench, cx, _) = open(
        cx,
        Launch {
            root: None,
            file: None,
        },
    );
    let item = text_item("Output", "x");
    let panel = ask(&workbench, output_ask(false, vec![item.clone()]), cx);
    assert_eq!(attached(&panel, cx), vec![item]);
}

#[gpui::test]
fn a_key_still_being_read_is_waited_for(cx: &mut TestAppContext) {
    let (workbench, cx, provider, _dir) = setup(cx, false, true);
    let store = Arc::new(MemoryStore::new());
    store
        .set(
            &super::super::super::key_service(),
            crate::chat_panel::KEY_ACCOUNT,
            "saved",
        )
        .ok();
    cx.update(|_, cx| {
        cx.set_global(KeySource {
            env: None,
            store: store.clone(),
        })
    });
    ask(
        &workbench,
        output_ask(true, vec![text_item("Out", "x")]),
        cx,
    );
    assert_eq!(provider.requests().len(), 1);
    assert!(toasts(&workbench, cx).is_empty());
}

#[gpui::test]
fn history_and_the_picker_give_way(cx: &mut TestAppContext) {
    let (workbench, cx, _provider, _dir) = setup(cx, true, true);
    let panel = ask(&workbench, the_ask(false, vec![]), cx);
    panel.update(cx, |panel, cx| panel.toggle_picker(cx));
    assert!(panel.read_with(cx, |panel, _| panel.picker));
    ask(&workbench, the_ask(false, vec![]), cx);
    assert!(!panel.read_with(cx, |panel, _| panel.picker));

    // History, listing still under way.
    panel.update(cx, |panel, cx| panel.toggle_history(cx));
    ask(&workbench, the_ask(false, vec![]), cx);
    draw(cx);
    panel.read_with(cx, |panel, _| {
        assert!(panel.history.is_none() && panel.listing.is_none());
    });
}

#[gpui::test]
fn an_untrusted_folder_keeps_it_from_sending(cx: &mut TestAppContext) {
    let (workbench, cx, provider, _dir) = setup(cx, true, false);
    let panel = ask(
        &workbench,
        output_ask(true, vec![text_item("Out", "x")]),
        cx,
    );
    assert!(provider.requests().is_empty());
    assert_eq!(composer(&panel, cx), "Why did this fail?");
    assert_eq!(
        toasts(&workbench, cx),
        vec![tr!("workbench-chat-ask-untrusted").to_string()]
    );
}

#[gpui::test]
fn a_pending_send_is_for_the_chat_asked(cx: &mut TestAppContext) {
    let (workbench, cx, provider, _dir) = setup(cx, false, true);
    let store = Arc::new(MemoryStore::new());
    store
        .set(
            &super::super::super::key_service(),
            crate::chat_panel::KEY_ACCOUNT,
            "saved",
        )
        .ok();
    cx.update(|_, cx| {
        cx.set_global(KeySource {
            env: None,
            store: store.clone(),
        })
    });
    // The key read is still out when the chat is replaced and the same
    // prompt typed into the new one.
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.ask_agent(output_ask(true, vec![text_item("Out", "x")]), window, cx)
    });
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).chat().cloned().expect("made")
    });
    panel.update_in(cx, |panel, window, cx| panel.start_chat(window, cx));
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |view, cx| {
        view.set_composer_text("Why did this fail?", cx)
    });
    draw(cx);
    assert!(provider.requests().is_empty());
    assert_eq!(composer(&panel, cx), "Why did this fail?");
}
