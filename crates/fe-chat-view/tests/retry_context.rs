//! Retry sends the context that is authorized now, not the failed turn's.

use std::sync::Arc;

use fe_chat::{
    ChatEvent, ChatProvider, ChatRequest, ChatSettings, ContextItem, ContextKind, Conversation,
    ScriptedProvider, StopReason,
};
use fe_chat_view::{ChatOptions, ChatView, RetryTurn, SendMessage};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use serde_json::json;

fn refusal() -> Vec<ChatEvent> {
    vec![
        ChatEvent::Started {
            model: "claude-opus-5-5".to_owned(),
            message_id: "msg_r".to_owned(),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::Refusal,
            stop_details: Some(json!({"type": "refusal", "category": "cyber"})),
        },
    ]
}

fn file(path: &str, text: &str) -> ContextItem {
    ContextItem {
        kind: ContextKind::File {
            path: path.to_owned(),
            rows: None,
        },
        label: path.to_owned(),
        text: text.to_owned(),
    }
}

/// The tags sent with the last request, before its question.
fn tags(request: &ChatRequest) -> Vec<String> {
    let content = &request.messages.last().unwrap().content;
    (content[..content.len() - 1].iter())
        .map(|block| block["text"].as_str().unwrap().to_owned())
        .collect()
}

struct Chat<'a> {
    view: Entity<ChatView>,
    cx: &'a mut VisualTestContext,
}

impl Chat<'_> {
    fn settle(&mut self) {
        for _ in 0..3 {
            self.cx.run_until_parked();
            self.cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    }

    fn send(&mut self, text: &str) {
        self.view
            .update(self.cx, |view, cx| view.set_composer_text(text, cx));
        self.cx.dispatch_action(SendMessage);
        self.settle();
    }

    fn retry(&mut self) {
        self.cx.dispatch_action(RetryTurn);
        self.settle();
    }
}

#[gpui::test]
fn retry_drops_unauthorized_context(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_chat_view::init(cx);
    });
    let provider = Arc::new(ScriptedProvider::new((0..9).map(|_| refusal())));
    let conversation = Conversation::new(&ChatSettings::default());
    let dyn_provider: Arc<dyn ChatProvider> = provider.clone();
    let (view, cx) = cx.add_window_view(move |window, cx| {
        ChatView::new(
            dyn_provider,
            conversation,
            ChatOptions::default(),
            window,
            cx,
        )
    });
    cx.update(|window, cx| view.focus_handle(cx).focus(window, cx));
    let mut chat = Chat { view, cx };
    chat.settle();

    // What changes: the root or the trust (the host clears what the user
    // attached and stops gathering), `buffer.read` going (the same), and
    // the host alone no longer gathering.
    let changes: [(&str, bool, bool); 3] = [
        ("root", true, true),
        ("buffer.read", true, true),
        ("trust", true, false),
    ];
    for (n, (name, clear, ungather)) in changes.into_iter().enumerate() {
        let mine_secret = format!("mine {name}");
        let host_secret = format!("host {name}");
        let mine = file("mine.txt", &mine_secret);
        let host = file("host.txt", &host_secret);
        chat.view.update(chat.cx, |view, cx| {
            view.set_context(vec![host.clone()], cx);
            view.attach(mine.clone(), cx);
        });
        chat.send(&format!("question {name}"));
        assert_eq!(tags(&provider.requests()[n * 3]).len(), 2, "{name}");

        // Unchanged: the same context goes again.
        chat.retry();
        let requests = provider.requests();
        assert_eq!(requests[n * 3 + 1], requests[n * 3], "{name}");

        chat.view.update(chat.cx, |view, cx| {
            if clear {
                view.clear_attachments(cx);
            }
            if ungather {
                view.set_context(Vec::new(), cx);
            }
        });
        chat.retry();
        let retried = provider.requests()[n * 3 + 2].clone();
        let sent = tags(&retried).join("\n");
        assert!(!sent.contains(&mine_secret), "{name}: {sent}");
        assert_eq!(sent.contains(&host_secret), !ungather, "{name}: {sent}");
        let blocks = &retried.messages.last().unwrap().content;
        assert_eq!(blocks.last().unwrap()["text"], format!("question {name}"));
        chat.view
            .update(chat.cx, |view, cx| view.set_context(Vec::new(), cx));
    }
}

#[gpui::test]
fn retry_drops_context_gathered_under_another_authorization(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_chat_view::init(cx);
    });
    let provider = Arc::new(ScriptedProvider::new((0..3).map(|_| refusal())));
    let conversation = Conversation::new(&ChatSettings::default());
    let dyn_provider: Arc<dyn ChatProvider> = provider.clone();
    let (view, cx) = cx.add_window_view(move |window, cx| {
        ChatView::new(
            dyn_provider,
            conversation,
            ChatOptions::default(),
            window,
            cx,
        )
    });
    cx.update(|window, cx| view.focus_handle(cx).focus(window, cx));
    let mut chat = Chat { view, cx };
    chat.settle();

    let diagnostics = ContextItem {
        kind: ContextKind::Diagnostics,
        label: "Problems".to_owned(),
        text: "old root problem".to_owned(),
    };
    let editors = ContextItem {
        kind: ContextKind::OpenEditors,
        label: "Open Editors".to_owned(),
        text: "old.rs\n".to_owned(),
    };
    let main = file("src/main.rs", "old root main");
    chat.view.update(chat.cx, |view, cx| {
        view.set_context(vec![main, diagnostics.clone(), editors.clone()], cx);
    });
    chat.send("question");
    assert_eq!(tags(&provider.requests()[0]).len(), 3);

    // Another root: the same relative path, the same kinds, new text.
    chat.view.update(chat.cx, |view, cx| {
        view.clear_attachments(cx);
        view.set_authorization(1);
        view.set_context(
            vec![
                file("src/main.rs", "new root main"),
                ContextItem {
                    text: "new root problem".to_owned(),
                    ..diagnostics
                },
                ContextItem {
                    text: "new.rs\n".to_owned(),
                    ..editors
                },
            ],
            cx,
        );
    });
    chat.retry();
    let retried = provider.requests()[1].clone();
    let sent = tags(&retried).join("\n");
    for old in ["old root main", "old root problem", "old.rs"] {
        assert!(!sent.contains(old), "{old} resent: {sent}");
    }
    assert!(sent.is_empty(), "{sent}");
}
