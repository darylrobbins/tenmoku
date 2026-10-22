//! How each turn was made, as the view records it as it makes the turn:
//! sent from the composer, by any path, or a Retry of a turn, whatever
//! the composer held meanwhile.

use std::sync::Arc;

use fe_chat::{
    ChatEvent, ChatProvider, ChatSettings, Conversation, ErrorKind, ProviderError,
    ScriptedProvider, StopReason,
};
use fe_chat_view::{ChatOptions, ChatView, RetryTurn, SendMessage};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::command::{Invocation, Source};
use serde_json::json;

fn refusal() -> Vec<ChatEvent> {
    vec![
        ChatEvent::Started {
            model: "claude-opus-5-5".to_owned(),
            message_id: "msg_r".to_owned(),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::Refusal,
            stop_details: Some(json!({"type": "refusal"})),
        },
    ]
}

/// A reply that fails as an overloaded server's does, which the banner
/// offers to retry.
fn failure() -> Vec<ChatEvent> {
    vec![ChatEvent::Failed(ProviderError::new(
        ErrorKind::Server,
        "overloaded",
    ))]
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

/// Each turn's origin: whether it was sent, and the turn it retried.
fn origins(view: &Entity<ChatView>, cx: &mut VisualTestContext) -> Vec<(bool, Option<usize>)> {
    view.read_with(cx, |view, _| {
        (0..view.conversation().turns.len())
            .map(|index| (view.was_sent(index), view.retry_of(index)))
            .collect()
    })
}

#[gpui::test]
fn turns_know_how_they_were_made(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_chat_view::init(cx);
    });
    let replies = [refusal(), refusal(), refusal(), failure(), refusal()];
    let provider = Arc::new(ScriptedProvider::new(replies));
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
    settle(cx);
    const SENT: (bool, Option<usize>) = (true, None);

    // Typed, and sent as a key bound to `chat.send` sends it, through
    // kiln's dispatch.
    cx.simulate_input("Rename");
    let typed = view.read_with(cx, |view, cx| view.composer_text(cx));
    assert_eq!(typed, "Rename");
    let key = Invocation::of("chat.send")
        .unwrap()
        .source(Source::Keyboard);
    cx.update(|window, cx| {
        kiln::platform::command_actions::dispatch_invocation(key, window, cx).unwrap();
    });
    settle(cx);
    assert_eq!(origins(&view, cx), [SENT]);

    // A draft set and cleared, then Retry, in one update: a retry.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_composer_text("Rename", cx);
            view.set_composer_text("", cx);
        });
        window.dispatch_action(Box::new(RetryTurn), cx);
    });
    settle(cx);
    assert_eq!(origins(&view, cx), [SENT, (false, Some(0))]);

    // The same question set and sent in one update: a send, not a retry.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.set_composer_text("Rename", cx));
        window.dispatch_action(Box::new(SendMessage), cx);
    });
    settle(cx);
    assert_eq!(origins(&view, cx), [SENT, (false, Some(0)), SENT]);

    // The Send button, then the Retry banner's button.
    view.update(cx, |view, cx| view.set_composer_text("Again", cx));
    let send = cx.debug_bounds("chat-send").expect("Send drawn");
    cx.simulate_click(send.center(), Default::default());
    settle(cx);
    let retry = cx
        .debug_bounds("chat-banner-action-3")
        .expect("Retry drawn");
    cx.simulate_click(retry.center(), Default::default());
    settle(cx);
    assert_eq!(
        origins(&view, cx),
        [SENT, (false, Some(0)), SENT, SENT, (false, Some(3))]
    );
    assert_eq!(provider.requests().len(), 5);
}
