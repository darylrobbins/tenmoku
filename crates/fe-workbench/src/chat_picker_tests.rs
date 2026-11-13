//! The chat header's model picker in a headless window: its label, its
//! list in place of the chat, and picks that apply from the next send. The
//! provider is a [`ScriptedProvider`] from the panel's test factory: no
//! network.

use std::{fs, path::Path, rc::Rc, sync::Arc};

use fe_chat::{
    ApiMessage, BlockKind, ChatEvent, ChatProvider, Effort, ScriptedProvider, StopReason,
};
use fe_chat_view::SendMessage;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::SettingsStore;
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    core::secrets::MemoryStore,
    platform::config::{BatchSender, batch_channel},
};
use serde_json::json;

use super::{ChatPanel, KeySource, picker_choices};
use crate::{Launch, Workbench, tests::open};

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

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    run_store(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    run_store(cx);
}

/// Lets background work land, running the chat store's requests as they
/// come.
fn run_store(cx: &mut VisualTestContext) {
    loop {
        cx.run_until_parked();
        let ran = cx.update(|_, cx| {
            let worker = cx.try_global::<super::chat_store_worker::WorkerHandle>();
            worker.map_or(0, |handle| handle.0.run_pending())
        });
        if ran == 0 {
            break;
        }
    }
}

/// kiln's settings service over a user settings file holding `text`, and
/// the sender that would reload it, kept for the test's length.
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

/// A finished reply of `text`, with a thinking block.
fn reply(text: &str) -> Vec<ChatEvent> {
    vec![
        ChatEvent::Started {
            model: "claude-opus-5-5".to_owned(),
            message_id: "msg".to_owned(),
        },
        ChatEvent::BlockStarted {
            index: 0,
            kind: BlockKind::Thinking,
        },
        ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "thinking", "thinking": "", "signature": "sig"}),
        },
        ChatEvent::BlockStarted {
            index: 1,
            kind: BlockKind::Text,
        },
        ChatEvent::BlockFinished {
            index: 1,
            block: json!({"type": "text", "text": text}),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        },
    ]
}

/// Opens the Chat panel with a key and `provider` behind it.
fn chat_with(
    workbench: &Entity<Workbench>,
    provider: Arc<ScriptedProvider>,
    cx: &mut VisualTestContext,
) -> Entity<ChatPanel> {
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
    let stack = ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT));
    let run = RunCommand(Invocation::of("chat.open").unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).chat().cloned().expect("made")
    })
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).expect("drawn");
    cx.simulate_click(bounds.center(), Default::default());
    draw(cx);
}

fn label(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> String {
    panel.read_with(cx, |panel, cx| panel.picker_label(cx))
}

fn model_and_effort(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> (String, Effort) {
    panel.read_with(cx, |panel, cx| {
        let conversation = panel.view().read(cx).conversation();
        (conversation.model.clone(), conversation.effort)
    })
}

fn composer_focused(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| {
        let view = panel.read(cx).view().read(cx);
        view.composer().focus_handle(cx).is_focused(window)
    })
}

/// Sends `question` and lets the turn finish.
fn ask(panel: &Entity<ChatPanel>, question: &str, cx: &mut VisualTestContext) {
    let view = panel.read_with(cx, |panel, _| panel.view().clone());
    view.update(cx, |view, cx| view.set_composer_text(question, cx));
    cx.update(|window, cx| window.dispatch_action(Box::new(SendMessage), cx));
    draw(cx);
    draw(cx);
}

#[gpui::test]
fn header_shows_model_and_effort(cx: &mut TestAppContext) {
    let config = tempfile::tempdir().unwrap();
    let _sender = settings(
        config.path(),
        "[chat]\nmodel = \"claude-sonnet-5-5\"\neffort = \"high\"\n",
        cx,
    );
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let panel = chat_with(&workbench, Arc::new(ScriptedProvider::new([])), cx);
    assert!(cx.debug_bounds("chat-model-picker").is_some());
    assert_eq!(label(&panel, cx), "Sonnet 5.5 · High");

    // It opens the list in place of the chat: the models, then the
    // efforts.
    click("chat-model-picker", cx);
    assert!(panel.read_with(cx, |panel, _| panel.picker_open()));
    assert!(cx.debug_bounds("chat-picker-list").is_some());
    for row in ["chat-model-row-3", "chat-effort-row-0", "chat-effort-row-4"] {
        assert!(cx.debug_bounds(row).is_some(), "{row}");
    }
    assert!(
        cx.debug_bounds("chat-model-row-4").is_none(),
        "no typed-in model"
    );

    // On an empty chat a pick is the chat's own, and the list closes.
    click("chat-effort-row-0", cx);
    assert!(!panel.read_with(cx, |panel, _| panel.picker_open()));
    assert!(cx.debug_bounds("chat-picker-list").is_none());
    assert!(composer_focused(&panel, cx));
    assert_eq!(
        model_and_effort(&panel, cx),
        ("claude-sonnet-5-5".to_owned(), Effort::Low)
    );
    assert_eq!(label(&panel, cx), "Sonnet 5.5 · Low");
    click("chat-model-picker", cx);
    click("chat-model-row-0", cx);
    assert_eq!(label(&panel, cx), "Opus 5.5 · Low");

    // History and the picker take each other's place.
    click("chat-model-picker", cx);
    click("chat-history", cx);
    assert!(!panel.read_with(cx, |panel, _| panel.picker_open()));
    assert!(cx.debug_bounds("chat-picker-list").is_none());
    click("chat-model-picker", cx);
    assert!(cx.debug_bounds("chat-history-list").is_none());
    assert!(cx.debug_bounds("chat-picker-list").is_some());
    // And a new chat closes it, from the settings again: the picks weren't
    // written there.
    click("chat-new", cx);
    assert!(cx.debug_bounds("chat-picker-list").is_none());
    assert_eq!(label(&panel, cx), "Sonnet 5.5 · High");
}

#[gpui::test]
fn pick_on_turned_chat_applies_on_next_send(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let provider = Arc::new(ScriptedProvider::new([reply("One."), reply("Two.")]));
    let panel = chat_with(&workbench, provider.clone(), cx);
    assert_eq!(label(&panel, cx), "Opus 5.5 · Medium");
    ask(&panel, "First?", cx);
    let history = panel.read_with(cx, |panel, cx| {
        panel.view().read(cx).conversation().history()
    });
    assert_eq!(history.len(), 2);

    // Picked between turns: nothing earlier changes.
    click("chat-model-picker", cx);
    click("chat-model-row-1", cx);
    click("chat-model-picker", cx);
    click("chat-effort-row-2", cx);
    assert_eq!(label(&panel, cx), "Sonnet 5.5 · High");
    let (turns, now) = panel.read_with(cx, |panel, cx| {
        let conversation = panel.view().read(cx).conversation();
        (conversation.turns.clone(), conversation.history())
    });
    assert_eq!(now, history);
    assert_eq!(turns[0].asked_model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(turns[0].asked_effort, Some(Effort::Medium));
    assert_eq!(provider.requests().len(), 1, "nothing sent on a pick");

    ask(&panel, "Second?", cx);
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let (first, second) = (&requests[0], &requests[1]);
    assert_eq!(first.model, "claude-opus-5-5");
    assert_eq!(second.model, "claude-sonnet-5-5");
    // Claude Sonnet 5.5 takes the effort message: the top-level effort
    // stays the chat's first, and the message goes before the new turn.
    assert_eq!(second.effort, Effort::Medium);
    assert_eq!(second.messages[..2], history[..]);
    assert_eq!(second.messages[2], ApiMessage::effort(Effort::High));
    assert_eq!(second.messages.len(), 4);
    let turn = panel.read_with(cx, |panel, cx| {
        panel.view().read(cx).conversation().turns[1].clone()
    });
    assert_eq!(turn.asked_model.as_deref(), Some("claude-sonnet-5-5"));
    assert_eq!(turn.asked_effort, Some(Effort::High));
}

#[gpui::test]
fn typed_in_model_listed(cx: &mut TestAppContext) {
    let config = tempfile::tempdir().unwrap();
    let _sender = settings(
        config.path(),
        "[chat]\nmodel = \"claude-opus-6-preview\"\n",
        cx,
    );
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    let panel = chat_with(&workbench, Arc::new(ScriptedProvider::new([])), cx);
    // Shown as itself.
    assert_eq!(label(&panel, cx), "claude-opus-6-preview · Medium");

    let choices = panel.read_with(cx, |panel, cx| {
        picker_choices(panel.view().read(cx).conversation())
    });
    let mut expected: Vec<String> = (fe_config::chat_models::KNOWN.iter())
        .map(|model| (*model).to_owned())
        .collect();
    expected.push("claude-opus-6-preview".to_owned());
    assert_eq!(choices.models, expected, "the known ones, then the chat's");
    assert_eq!(
        choices.efforts,
        [
            Effort::Low,
            Effort::Medium,
            Effort::High,
            Effort::XHigh,
            Effort::Max
        ]
    );

    click("chat-model-picker", cx);
    assert!(cx.debug_bounds("chat-model-row-4").is_some());
    // Once a known one is picked, it is listed no more.
    click("chat-model-row-0", cx);
    assert_eq!(label(&panel, cx), "Opus 5.5 · Medium");
    click("chat-model-picker", cx);
    assert!(cx.debug_bounds("chat-model-row-4").is_none());
}
