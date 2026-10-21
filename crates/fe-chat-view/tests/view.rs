//! A chat view in a test window over a scripted provider: sending and
//! streaming, stopping, the record's § 14 states and their actions, and
//! thinking.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use fe_chat::{
    BlockKind, ChatEvent, ChatProvider, ChatSettings, ContextItem, ContextKind, Conversation,
    ErrorKind, ProviderError, Rows, ScriptedProvider, StopReason, TurnState, Usage,
};
use fe_chat_view::{
    BannerAction, ChatOptions, ChatView, ChatViewEvent, RetryTurn, SendMessage, StopReply,
};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use serde_json::json;

const MODEL: &str = "claude-opus-5-5";

/// A reply of `deltas` from `model`, its text block finished whole, then
/// `stop_reason`.
fn reply_from(model: &str, deltas: &[&str], stop_reason: StopReason) -> Vec<ChatEvent> {
    let mut events = vec![
        ChatEvent::Started {
            model: model.to_owned(),
            message_id: "msg_1".to_owned(),
        },
        ChatEvent::BlockStarted {
            index: 0,
            kind: BlockKind::Text,
        },
    ];
    events.extend(deltas.iter().map(|text| ChatEvent::TextDelta {
        index: 0,
        text: (*text).to_owned(),
    }));
    events.extend([
        ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "text", "text": deltas.concat()}),
        },
        ChatEvent::Usage(Usage {
            input_tokens: 12_000,
            output_tokens: 40,
            ..Usage::default()
        }),
        ChatEvent::Finished {
            stop_reason,
            stop_details: None,
        },
    ]);
    events
}

fn reply(deltas: &[&str]) -> Vec<ChatEvent> {
    reply_from(MODEL, deltas, StopReason::EndTurn)
}

fn failure(kind: ErrorKind, message: &str) -> Vec<ChatEvent> {
    vec![ChatEvent::Failed(ProviderError::new(kind, message))]
}

fn refusal(category: &str) -> Vec<ChatEvent> {
    vec![
        ChatEvent::Started {
            model: MODEL.to_owned(),
            message_id: "msg_r".to_owned(),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::Refusal,
            stop_details: Some(json!({"type": "refusal", "category": category})),
        },
    ]
}

struct Chat<'a> {
    view: Entity<ChatView>,
    cx: &'a mut VisualTestContext,
    events: Rc<RefCell<Vec<ChatViewEvent>>>,
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_chat_view::init(cx);
    });
}

/// A focused chat over `provider`, the conversation showing thinking or
/// not.
fn open(cx: &mut TestAppContext, provider: Arc<ScriptedProvider>, show_thinking: bool) -> Chat<'_> {
    let conversation = Conversation::new(&ChatSettings {
        show_thinking,
        ..ChatSettings::default()
    });
    let options = ChatOptions {
        max_tokens: 1000,
        fallbacks: true,
    };
    let provider: Arc<dyn ChatProvider> = provider;
    let (view, cx) = cx.add_window_view(move |window, cx| {
        ChatView::new(provider, conversation, options, window, cx)
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let log = events.clone();
    cx.update(|window, cx| {
        cx.subscribe(&view, move |_, event: &ChatViewEvent, _| {
            log.borrow_mut().push(event.clone())
        })
        .detach();
        view.focus_handle(cx).focus(window, cx);
    });
    let mut chat = Chat { view, cx, events };
    chat.settle();
    chat
}

impl Chat<'_> {
    fn settle(&mut self) {
        for _ in 0..3 {
            self.cx.run_until_parked();
            self.cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    }

    fn draw(&mut self) {
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn type_text(&mut self, text: &str) {
        self.view
            .update(self.cx, |view, cx| view.set_composer_text(text, cx));
    }

    fn send(&mut self, text: &str) {
        self.type_text(text);
        self.cx.dispatch_action(SendMessage);
        self.settle();
    }

    /// Dispatches `action` to the focused element without running what it
    /// spawns, which `VisualTestContext::dispatch_action` runs to the end.
    fn dispatch_now(&mut self, action: impl gpui::Action) {
        let action = action.boxed_clone();
        self.cx
            .update(|window, cx| window.dispatch_action(action, cx));
    }

    fn shown(&mut self, selector: String) -> bool {
        self.cx.debug_bounds(selector.leak()).is_some()
    }

    fn click(&mut self, selector: String) {
        let bounds = (self.cx.debug_bounds(selector.clone().leak()))
            .unwrap_or_else(|| panic!("no {selector} drawn"));
        self.cx.simulate_click(bounds.center(), Modifiers::none());
        self.settle();
    }

    fn take_events(&mut self) -> Vec<ChatViewEvent> {
        std::mem::take(&mut self.events.borrow_mut())
    }

    fn read<R>(&mut self, f: impl FnOnce(&ChatView, &gpui::App) -> R) -> R {
        self.view.read_with(self.cx, f)
    }

    fn state(&mut self, turn: usize) -> TurnState {
        self.read(|view, _| view.conversation().turns[turn].state.clone())
    }
}

fn last_text(request: &fe_chat::ChatRequest) -> String {
    let message = request.messages.last().expect("a message");
    let block = message.content.last().expect("a block");
    block["text"].as_str().expect("a text block").to_owned()
}

#[gpui::test]
fn a_sent_message_streams_into_the_transcript(cx: &mut TestAppContext) {
    init(cx);
    let provider = Arc::new(ScriptedProvider::new([
        reply(&["Hello", ", ", "world!"]),
        reply(&["Second"]),
    ]));
    let mut chat = open(cx, provider.clone(), false);
    chat.view.update(chat.cx, |view, cx| {
        view.attach(
            ContextItem {
                kind: ContextKind::Selection {
                    path: "src/lib.rs".to_owned(),
                    rows: Rows { first: 3, last: 4 },
                },
                label: "lib.rs:3-4".to_owned(),
                text: "fn main() {}\n".to_owned(),
            },
            cx,
        )
    });
    chat.settle();
    assert!(chat.shown("chat-chip-0".into()));

    chat.send("What does this do?");

    let shown = chat.read(|view, _| view.turn_display(0)).expect("a turn");
    assert_eq!(shown.reply, "Hello, world!");
    assert_eq!(shown.question, "What does this do?");
    assert_eq!(shown.attachments, ["lib.rs:3-4"]);
    assert!(chat.shown("chat-turn-0".into()));
    assert!(chat.shown("chat-reply-0".into()));
    assert!(!chat.shown("chat-banner-0".into()));
    assert!(!chat.shown("chat-chip-0".into()));
    assert_eq!(chat.read(|view, cx| view.composer_text(cx)), "");
    assert!(chat.read(|view, _| view.attachments().is_empty()));
    assert_eq!(chat.state(0), TurnState::Done);
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(last_text(&requests[0]), "What does this do?");
    assert_eq!(
        requests[0].messages[0].content.len(),
        2,
        "the selection, then the question"
    );
    assert_eq!(requests[0].max_tokens, 1000);
    assert_eq!(chat.take_events(), [ChatViewEvent::TurnFinished]);
    assert_eq!(
        chat.read(|view, _| view.meter()).as_deref(),
        Some("12k of 1M tokens")
    );

    // Nothing to send.
    chat.send("   \n");
    assert_eq!(provider.requests().len(), 1);
    assert_eq!(chat.read(|view, _| view.conversation().turns.len()), 1);

    // Nothing while a reply streams: the text stays for later.
    chat.type_text("Next");
    chat.dispatch_now(SendMessage);
    assert!(chat.read(|view, _| view.is_streaming()));
    chat.type_text("And another");
    chat.dispatch_now(SendMessage);
    chat.settle();
    assert_eq!(provider.requests().len(), 2);
    assert_eq!(chat.read(|view, _| view.conversation().turns.len()), 2);
    assert_eq!(chat.read(|view, cx| view.composer_text(cx)), "And another");
    assert_eq!(chat.take_events(), [ChatViewEvent::TurnFinished]);
}

#[gpui::test]
fn stop_cancels_the_stream(cx: &mut TestAppContext) {
    init(cx);
    let deltas: Vec<String> = (0..20).map(|n| format!("word{n} ")).collect();
    let deltas: Vec<&str> = deltas.iter().map(String::as_str).collect();
    let provider = Arc::new(ScriptedProvider::new([reply(&deltas)]));
    let mut chat = open(cx, provider.clone(), false);

    chat.type_text("Tell me a long story");
    chat.dispatch_now(SendMessage);
    // Step the stream until some of the reply has come.
    let mut ticks = 0;
    while chat.read(|view, _| view.conversation().turns[0].text().is_empty()) {
        assert!(chat.cx.executor().tick(), "the stream stalled");
        ticks += 1;
        assert!(ticks < 10_000, "no text came");
    }
    chat.draw();
    assert!(chat.shown("chat-stop".into()));
    assert!(!chat.shown("chat-send".into()));

    chat.cx.dispatch_action(StopReply);
    chat.settle();

    assert_eq!(provider.dropped(), 1);
    assert_eq!(chat.state(0), TurnState::Stopped);
    let text = chat.read(|view, _| view.conversation().turns[0].text());
    assert!(!text.is_empty());
    assert_ne!(text, deltas.concat());
    assert_eq!(
        chat.read(|view, _| view.turn_display(0)).unwrap().reply,
        text
    );
    assert!(!chat.read(|view, _| view.is_streaming()));
    assert!(chat.shown("chat-send".into()));
    assert!(!chat.shown("chat-stop".into()));
    assert_eq!(chat.take_events(), [ChatViewEvent::TurnFinished]);

    // A second stop has nothing to stop.
    chat.cx.dispatch_action(StopReply);
    chat.settle();
    assert!(chat.take_events().is_empty());
}

#[gpui::test]
fn states_show_their_banner_and_action(cx: &mut TestAppContext) {
    init(cx);
    let mut cut_off = reply(&["Partial"]);
    cut_off.pop();
    cut_off.push(ChatEvent::Finished {
        stop_reason: StopReason::MaxTokens,
        stop_details: None,
    });
    let provider = Arc::new(ScriptedProvider::new([
        failure(ErrorKind::Auth, "invalid x-api-key"),
        failure(
            ErrorKind::RateLimited,
            "Number of requests has exceeded your rate limit",
        ),
        refusal("cyber"),
        reply_from(
            "claude-sonnet-5-5",
            &["From the fallback"],
            StopReason::EndTurn,
        ),
        cut_off,
        fallen_back(&["claude-sonnet-5-5"]),
        fallen_back(&["claude-sonnet-5-5", "claude-haiku-5-5"]),
    ]));
    let mut chat = open(cx, provider.clone(), false);

    // No key: the composer is disabled and the panel asks for one.
    chat.view
        .update(chat.cx, |view, cx| view.set_provider(None, cx));
    chat.settle();
    assert!(chat.shown("chat-no-key".into()));
    assert!(chat.read(|view, cx| view.composer().read(cx).is_read_only()));
    chat.click("chat-set-key".into());
    assert_eq!(chat.take_events(), [ChatViewEvent::SetKeyRequested]);
    chat.send("Hello");
    assert!(provider.requests().is_empty());
    let provider_dyn: Arc<dyn ChatProvider> = provider.clone();
    chat.view.update(chat.cx, |view, cx| {
        view.set_provider(Some(provider_dyn), cx)
    });
    chat.settle();
    assert!(!chat.shown("chat-no-key".into()));
    assert!(!chat.read(|view, cx| view.composer().read(cx).is_read_only()));

    // 401: the key was rejected; its button asks for another.
    chat.send("First");
    let banner = chat
        .read(|view, _| view.turn_display(0))
        .unwrap()
        .banner
        .unwrap();
    assert_eq!(banner.text, "Your API key was rejected");
    assert_eq!(banner.action, Some(BannerAction::SetKey));
    assert!(chat.shown("chat-banner-0".into()));
    assert_eq!(chat.take_events(), [ChatViewEvent::TurnFinished]);
    chat.click("chat-banner-action-0".into());
    assert_eq!(chat.take_events(), [ChatViewEvent::SetKeyRequested]);

    // 429: Retry sends the failed request again, as a turn of its own.
    chat.send("Second");
    let banner = chat
        .read(|view, _| view.turn_display(1))
        .unwrap()
        .banner
        .unwrap();
    assert_eq!(banner.action, Some(BannerAction::Retry));
    assert!(chat.shown("chat-banner-action-1".into()));
    assert_eq!(chat.take_events(), [ChatViewEvent::TurnFinished]);
    chat.cx.dispatch_action(RetryTurn);
    chat.settle();
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[2], requests[1]);
    assert_eq!(last_text(&requests[2]), "Second");
    assert_eq!(chat.take_events(), [ChatViewEvent::TurnFinished]);

    // The retry was refused: the category shows.
    assert_eq!(chat.state(2), TurnState::Refused);
    let banner = chat
        .read(|view, _| view.turn_display(2))
        .unwrap()
        .banner
        .unwrap();
    assert_eq!(banner.text, "Claude declined this request (cyber)");
    assert!(chat.shown("chat-banner-2".into()));
    // An earlier turn's Retry is gone: only the last turn sends again.
    assert!(!chat.shown("chat-banner-action-1".into()));

    // Another model answered.
    chat.send("Third");
    let shown = chat.read(|view, _| view.turn_display(3)).unwrap();
    assert_eq!(shown.answered_by.as_deref(), Some("claude-sonnet-5-5"));
    assert_eq!(shown.banner, None);
    assert!(chat.shown("chat-answered-by-3".into()));

    // Cut off at the token limit.
    chat.send("Fourth");
    let banner = chat
        .read(|view, _| view.turn_display(4))
        .unwrap()
        .banner
        .unwrap();
    assert_eq!(banner.text, "Reply cut off at the token limit");
    assert_eq!(banner.action, None);
    assert!(chat.shown("chat-banner-4".into()));
    assert!(
        chat.read(|view, _| view.turn_display(4))
            .unwrap()
            .answered_by
            .is_none()
    );
    assert_eq!(
        chat.take_events(),
        [ChatViewEvent::TurnFinished, ChatViewEvent::TurnFinished]
    );

    // A fallback mid-output names the model it fell back to, with no
    // Started of its own; after two, the last.
    chat.send("Fifth");
    let shown = chat.read(|view, _| view.turn_display(5)).unwrap();
    assert_eq!(shown.answered_by.as_deref(), Some("claude-sonnet-5-5"));
    assert_eq!(shown.reply, "Begun, then finished.");
    assert!(chat.shown("chat-answered-by-5".into()));
    chat.send("Sixth");
    let shown = chat.read(|view, _| view.turn_display(6)).unwrap();
    assert_eq!(shown.answered_by.as_deref(), Some("claude-haiku-5-5"));
    assert!(chat.shown("chat-answered-by-6".into()));
}

/// A reply that `MODEL` starts, then falls back mid-output to each of
/// `models` in turn, each `fallback` block between two text blocks.
fn fallen_back(models: &[&str]) -> Vec<ChatEvent> {
    let mut events = vec![ChatEvent::Started {
        model: MODEL.to_owned(),
        message_id: "msg_f".to_owned(),
    }];
    let text = |events: &mut Vec<ChatEvent>, index: usize, text: &str| {
        events.extend([
            ChatEvent::BlockStarted {
                index,
                kind: BlockKind::Text,
            },
            ChatEvent::TextDelta {
                index,
                text: text.to_owned(),
            },
            ChatEvent::BlockFinished {
                index,
                block: json!({"type": "text", "text": text}),
            },
        ]);
    };
    text(&mut events, 0, "Begun, ");
    let mut from = MODEL;
    for (n, to) in models.iter().enumerate() {
        let index = 2 * n + 1;
        events.extend([
            ChatEvent::BlockStarted {
                index,
                kind: BlockKind::Fallback {
                    from: from.to_owned(),
                    to: (*to).to_owned(),
                },
            },
            ChatEvent::BlockFinished {
                index,
                block: json!({"type": "fallback", "from": from, "to": to}),
            },
        ]);
        from = to;
    }
    text(&mut events, 2 * models.len() + 1, "then finished.");
    events.push(ChatEvent::Finished {
        stop_reason: StopReason::EndTurn,
        stop_details: None,
    });
    events
}

/// A reply that thinks first.
fn thoughtful_reply() -> Vec<ChatEvent> {
    let mut events = vec![
        ChatEvent::Started {
            model: MODEL.to_owned(),
            message_id: "msg_t".to_owned(),
        },
        ChatEvent::BlockStarted {
            index: 0,
            kind: BlockKind::Thinking,
        },
        ChatEvent::ThinkingDelta {
            index: 0,
            text: "Weighing the options.".to_owned(),
        },
        ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "thinking", "thinking": "Weighing the options.", "signature": "sig"}),
        },
    ];
    let text = reply(&["The answer."]);
    events.extend(text.into_iter().skip(1).map(|event| match event {
        ChatEvent::BlockStarted { kind, .. } => ChatEvent::BlockStarted { index: 1, kind },
        ChatEvent::TextDelta { text, .. } => ChatEvent::TextDelta { index: 1, text },
        ChatEvent::BlockFinished { block, .. } => ChatEvent::BlockFinished { index: 1, block },
        event => event,
    }));
    events
}

#[gpui::test]
fn thinking_shows_only_when_asked(cx: &mut TestAppContext) {
    init(cx);
    {
        let provider = Arc::new(ScriptedProvider::new([thoughtful_reply()]));
        let mut chat = open(cx, provider, false);
        chat.send("Why?");
        let shown = chat.read(|view, _| view.turn_display(0)).unwrap();
        assert_eq!(shown.reply, "The answer.");
        assert_eq!(shown.thinking, None);
        assert!(chat.shown("chat-reply-0".into()));
        assert!(!chat.shown("chat-thinking-0".into()));
    }

    let provider = Arc::new(ScriptedProvider::new([thoughtful_reply()]));
    let mut chat = open(cx, provider, true);
    chat.send("Why?");
    let shown = chat.read(|view, _| view.turn_display(0)).unwrap();
    assert_eq!(shown.reply, "The answer.");
    assert_eq!(shown.thinking.as_deref(), Some("Weighing the options."));
    assert!(chat.shown("chat-thinking-0".into()));
    assert!(
        !chat.shown("chat-thinking-text-0".into()),
        "collapsed at first"
    );
    assert!(!chat.read(|view, _| view.thinking_open(0)));

    chat.click("chat-thinking-0".into());
    assert!(chat.read(|view, _| view.thinking_open(0)));
    assert!(chat.shown("chat-thinking-text-0".into()));

    chat.click("chat-thinking-0".into());
    assert!(!chat.shown("chat-thinking-text-0".into()));
}

#[gpui::test]
fn the_notice_dismisses_and_the_meter_warns_late(cx: &mut TestAppContext) {
    init(cx);
    let mut long = reply(&["Long"]);
    for event in &mut long {
        if let ChatEvent::Usage(usage) = event {
            usage.cache_read_input_tokens = 800_000;
        }
    }
    let provider = Arc::new(ScriptedProvider::new([long]));
    let mut chat = open(cx, provider, false);
    assert!(!chat.shown("chat-notice".into()));
    assert_eq!(chat.read(|view, _| view.meter()), None);
    assert!(!chat.shown("chat-meter".into()));

    chat.view
        .update(chat.cx, |view, cx| view.set_notice(true, cx));
    chat.settle();
    assert!(chat.shown("chat-notice".into()));
    chat.click("chat-notice-dismiss".into());
    assert_eq!(chat.take_events(), [ChatViewEvent::NoticeDismissed]);
    assert!(!chat.shown("chat-notice".into()));

    chat.send("A long chat");
    assert_eq!(
        chat.read(|view, _| view.meter()).as_deref(),
        Some("812k of 1M tokens · Long chat: start a new one for better answers")
    );
    assert!(chat.shown("chat-meter".into()));
}

#[gpui::test]
fn the_composer_text_is_sent_as_typed(cx: &mut TestAppContext) {
    init(cx);
    let provider = Arc::new(ScriptedProvider::new([reply(&["Ok"])]));
    let mut chat = open(cx, provider.clone(), false);
    chat.send("    let x = 1;\n  ");
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(last_text(&requests[0]), "    let x = 1;\n  ");
}

#[gpui::test]
fn vim_stays_off_in_the_composer(cx: &mut TestAppContext) {
    init(cx);
    cx.update(|cx| fe_gpui::set_vim_enabled(cx, true));
    let provider = Arc::new(ScriptedProvider::new([]));
    let mut chat = open(cx, provider, false);
    let vim_mode = |chat: &mut Chat| chat.read(|view, cx| view.composer().read(cx).vim_mode());
    assert!(chat.read(|view, cx| view.composer().read(cx).is_vim_off()));
    assert_eq!(vim_mode(&mut chat), None);

    chat.cx.simulate_input("dd");
    chat.settle();
    assert_eq!(chat.read(|view, cx| view.composer_text(cx)), "dd");

    // Turning the switch off and on again leaves it off here.
    chat.cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, false));
    chat.settle();
    chat.cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    chat.settle();
    assert_eq!(vim_mode(&mut chat), None);
    chat.cx.simulate_input("x");
    chat.settle();
    assert_eq!(chat.read(|view, cx| view.composer_text(cx)), "ddx");
}

fn selection(label: &str, text: &str) -> ContextItem {
    ContextItem {
        kind: ContextKind::Selection {
            path: "src/a <b>.rs".to_owned(),
            rows: Rows { first: 1, last: 2 },
        },
        label: label.to_owned(),
        text: text.to_owned(),
    }
}

#[gpui::test]
fn edit_brings_back_the_question_and_its_attachments(cx: &mut TestAppContext) {
    init(cx);
    let provider = Arc::new(ScriptedProvider::new([
        refusal("cyber"),
        reply(&["Fine"]),
        failure(ErrorKind::TooLarge, "request too large"),
        reply(&["Smaller"]),
    ]));
    let mut chat = open(cx, provider.clone(), false);
    let items = [
        selection("a.rs:1-2", "fn a() {}\n"),
        ContextItem {
            kind: ContextKind::Diagnostics,
            label: "Problems".to_owned(),
            text: String::new(),
        },
    ];
    let attach = |chat: &mut Chat, items: &[ContextItem]| {
        let items = items.to_vec();
        chat.view.update(chat.cx, |view, cx| {
            for item in items {
                view.attach(item, cx);
            }
        });
    };

    // A refusal's Edit puts both back; sending again sends the same.
    attach(&mut chat, &items);
    chat.send("Is this safe?");
    let banner = chat
        .read(|view, _| view.turn_display(0))
        .unwrap()
        .banner
        .unwrap();
    assert_eq!(banner.action, Some(BannerAction::Edit));
    chat.click("chat-banner-action-0".into());
    assert_eq!(
        chat.read(|view, cx| view.composer_text(cx)),
        "Is this safe?"
    );
    assert_eq!(chat.read(|view, _| view.attachments().to_vec()), items);
    assert!(chat.shown("chat-chip-1".into()));
    chat.cx.dispatch_action(SendMessage);
    chat.settle();
    let requests = provider.requests();
    assert_eq!(requests[1], requests[0]);

    // A 413's Remove Attachments brings the chips back to take some off.
    attach(&mut chat, &items);
    chat.send("And this?");
    let banner = chat
        .read(|view, _| view.turn_display(2))
        .unwrap()
        .banner
        .unwrap();
    assert_eq!(banner.text, "Too much context");
    assert_eq!(banner.action, Some(BannerAction::RemoveAttachments));
    chat.click("chat-banner-action-2".into());
    assert_eq!(chat.read(|view, _| view.attachments().to_vec()), items);
    chat.click("chat-chip-remove-0".into());
    assert_eq!(chat.read(|view, _| view.attachments().to_vec()), items[1..]);
    chat.cx.dispatch_action(SendMessage);
    chat.settle();
    let requests = provider.requests();
    let sent = &requests[3].messages.last().unwrap().content;
    assert_eq!(sent.len(), 2, "the diagnostics, then the question");
    assert_eq!(sent[1]["text"], "And this?");
    assert_eq!(chat.state(3), TurnState::Done);
}

/// A reply of `lines` lines, cut off at the token limit, so it ends in a
/// banner.
fn long_reply(lines: usize) -> Vec<ChatEvent> {
    let lines: Vec<String> = (0..lines).map(|n| format!("line {n}\n")).collect();
    let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
    reply_from(MODEL, &lines, StopReason::MaxTokens)
}

impl Chat<'_> {
    /// Whether `selector` is drawn inside the transcript's visible area.
    fn in_view(&mut self, selector: String) -> bool {
        let transcript = self
            .cx
            .debug_bounds("chat-transcript")
            .expect("a transcript");
        let bounds = (self.cx.debug_bounds(selector.clone().leak()))
            .unwrap_or_else(|| panic!("no {selector} drawn"));
        bounds.top() >= transcript.top() && bounds.bottom() <= transcript.bottom()
    }
}

#[gpui::test]
fn the_transcript_follows_the_reply_to_its_end(cx: &mut TestAppContext) {
    init(cx);
    // The first reply's stream ends without saying how: its banner comes
    // in the update that ends the stream.
    let mut cut = long_reply(300);
    cut.pop();
    let provider = Arc::new(ScriptedProvider::new([cut, long_reply(300)]));
    let mut chat = open(cx, provider, false);

    chat.type_text("Go on");
    chat.dispatch_now(SendMessage);
    while chat.read(|view, _| view.conversation().turns[0].text().lines().count() < 150) {
        assert!(chat.cx.executor().tick(), "the stream stalled");
    }
    chat.draw();
    chat.draw();
    chat.settle();
    assert!(matches!(chat.state(0), TurnState::Failed(_)));
    assert!(chat.in_view("chat-banner-0".into()));

    // Scrolled up mid-reply, it stays where the user put it.
    chat.type_text("Again");
    chat.dispatch_now(SendMessage);
    while chat.read(|view, _| view.conversation().turns[1].text().lines().count() < 150) {
        assert!(chat.cx.executor().tick(), "the stream stalled");
    }
    chat.draw();
    chat.draw();
    let transcript = chat.cx.debug_bounds("chat-transcript").unwrap();
    chat.cx.simulate_event(gpui::ScrollWheelEvent {
        position: transcript.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(300.))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    chat.settle();
    assert_eq!(chat.state(1), TurnState::Done);
    assert!(!chat.in_view("chat-banner-1".into()));
}

#[gpui::test]
fn the_composer_rewraps_when_its_width_changes(cx: &mut TestAppContext) {
    init(cx);
    let provider = Arc::new(ScriptedProvider::new([]));
    let mut chat = open(cx, provider, false);
    chat.type_text(&"word ".repeat(80));
    chat.settle();
    // The height the composer's wrapped rows want, and the one it has.
    let heights = |chat: &mut Chat| {
        let wanted = chat.read(|view, cx| {
            let editor = view.composer().read(cx);
            let theme = editor.theme();
            let line_height = (theme.font_size * theme.line_height).round();
            let rows = editor.model().display().row_count().clamp(1, 10);
            (line_height * rows as f32 + gpui::px(2.), rows)
        });
        let drawn = chat.cx.debug_bounds("chat-composer").unwrap().size.height;
        (wanted, drawn)
    };
    let ((wide, wide_rows), drawn) = heights(&mut chat);
    assert_eq!(drawn, wide);

    // No input after the resizes: only the new width.
    chat.cx
        .simulate_resize(gpui::size(gpui::px(500.), gpui::px(900.)));
    chat.cx.run_until_parked();
    let ((narrow, narrow_rows), drawn) = heights(&mut chat);
    assert!(narrow_rows > wide_rows, "{narrow_rows} rows at 500px");
    assert_eq!(drawn, narrow);

    chat.cx
        .simulate_resize(gpui::size(gpui::px(1920.), gpui::px(900.)));
    chat.cx.run_until_parked();
    let ((wide_again, _), drawn) = heights(&mut chat);
    assert_eq!(wide_again, wide);
    assert_eq!(drawn, wide);
}
