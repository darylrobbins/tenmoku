//! A reply's code block header pinned over the transcript's top while its
//! block is scrolled under it: what is pinned where, and the pinned copy's
//! Copy and Apply.

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
};

use fe_chat::{
    BlockKind, ChatEvent, ChatProvider, ChatRequest, ChatSettings, ChatStream, Conversation,
    ScriptedProvider, StopReason,
};
use fe_chat_view::{ChatOptions, ChatView, ChatViewEvent, SendMessage};
use futures::channel::mpsc;
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent,
    TestAppContext, TouchPhase, VisualTestContext, point, px, size,
};
use serde_json::json;

/// A paragraph, an 80-line block for `deny.toml`, a paragraph, an 80-line
/// block for `src/lib.rs`, and a paragraph.
fn markdown() -> String {
    let lines = |name: &str| {
        (0..80)
            .map(|ix| format!("{name}_{ix} = {ix}\n"))
            .collect::<String>()
    };
    format!(
        "Two files.\n\n```toml path=deny.toml\n{}```\n\nAnd then:\n\n\
         ```rust path=src/lib.rs\n{}```\n\nDone.\n",
        lines("deny"),
        lines("lib"),
    )
}

fn delta(text: &str) -> ChatEvent {
    ChatEvent::TextDelta {
        index: 0,
        text: text.to_owned(),
    }
}

fn started() -> [ChatEvent; 2] {
    [
        ChatEvent::Started {
            model: "claude-opus-5-5".to_owned(),
            message_id: "msg_1".to_owned(),
        },
        ChatEvent::BlockStarted {
            index: 0,
            kind: BlockKind::Text,
        },
    ]
}

/// A provider whose one reply is what the test sends down a channel.
struct Channel(Mutex<Option<mpsc::UnboundedReceiver<ChatEvent>>>);

impl ChatProvider for Channel {
    fn name(&self) -> &str {
        "channel"
    }

    fn stream(&self, _: ChatRequest) -> ChatStream {
        Box::pin(self.0.lock().unwrap().take().expect("one reply"))
    }
}

struct Chat<'a> {
    view: Entity<ChatView>,
    cx: &'a mut VisualTestContext,
    events: Rc<RefCell<Vec<ChatViewEvent>>>,
}

/// A chat over `provider` in a 400 × 300 window, a question sent.
fn open(cx: &mut TestAppContext, provider: Arc<dyn ChatProvider>) -> Chat<'_> {
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_chat_view::init(cx);
    });
    let conversation = Conversation::new(&ChatSettings::default());
    let (view, cx) = cx.add_window_view(move |window, cx| {
        ChatView::new(provider, conversation, ChatOptions::default(), window, cx)
    });
    let events: Rc<RefCell<Vec<ChatViewEvent>>> = Rc::default();
    let log = events.clone();
    cx.update(|window, cx| {
        cx.subscribe(&view, move |_, event: &ChatViewEvent, _| {
            log.borrow_mut().push(event.clone())
        })
        .detach();
        view.focus_handle(cx).focus(window, cx);
    });
    cx.simulate_resize(size(px(400.), px(300.)));
    let mut chat = Chat { view, cx, events };
    chat.view
        .update(chat.cx, |view, cx| view.set_composer_text("Show me", cx));
    chat.cx
        .update(|window, cx| window.dispatch_action(Box::new(SendMessage), cx));
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

    fn bounds(&mut self, selector: &str) -> Option<Bounds<Pixels>> {
        self.cx.debug_bounds(selector.to_owned().leak())
    }

    fn wheel(&mut self, over: &str, dy: f32) {
        let over = self
            .bounds(over)
            .unwrap_or_else(|| panic!("no {over} drawn"));
        self.cx.simulate_event(ScrollWheelEvent {
            position: over.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
            modifiers: Modifiers::none(),
            touch_phase: TouchPhase::Moved,
        });
        self.settle();
    }

    /// Scrolled to the top of the transcript, then down by `y`.
    fn scroll_to(&mut self, y: Pixels) {
        self.wheel("chat-transcript", 1e6);
        self.wheel("chat-transcript", -f32::from(y));
    }

    /// Where `selector` is below the transcript's top, scrolled to its top.
    fn content_top(&mut self, selector: &str) -> Pixels {
        self.scroll_to(px(0.));
        let transcript = self.bounds("chat-transcript").expect("a transcript");
        let top = self
            .bounds(selector)
            .unwrap_or_else(|| panic!("no {selector} drawn"));
        top.top() - transcript.top()
    }

    fn pinned(&mut self) -> Option<(usize, usize)> {
        self.view
            .read_with(self.cx, |view, _| view.pinned_code_block())
    }

    fn click(&mut self, selector: &str) {
        let bounds = self
            .bounds(selector)
            .unwrap_or_else(|| panic!("no {selector} drawn"));
        self.cx.simulate_click(bounds.center(), Modifiers::none());
        self.settle();
    }

    fn take_events(&mut self) -> Vec<ChatViewEvent> {
        std::mem::take(&mut self.events.borrow_mut())
    }
}

#[gpui::test]
fn a_code_header_is_pinned_while_its_block_is_scrolled_under(cx: &mut TestAppContext) {
    let markdown = markdown();
    let mut reply = started().to_vec();
    reply.extend([
        delta(&markdown),
        ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "text", "text": markdown}),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        },
    ]);
    let mut chat = open(cx, Arc::new(ScriptedProvider::new([reply])));
    assert_eq!(chat.take_events(), [ChatViewEvent::TurnFinished]);
    let first = chat.content_top("md-code-0");
    let between = chat.content_top("md-block-2");
    let second = chat.content_top("md-code-1");

    // At the top, the first block's header is its own.
    assert_eq!(chat.pinned(), None);
    assert!(chat.bounds("chat-code-pinned").is_none());

    // The first block's code under the top, its header above it.
    chat.scroll_to(first + px(100.));
    let transcript = chat.bounds("chat-transcript").unwrap();
    let header = chat.bounds("chat-code-0-0").unwrap();
    assert!(
        header.bottom() < transcript.top(),
        "{header:?} {transcript:?}"
    );
    assert_eq!(chat.pinned(), Some((0, 0)));
    let pinned = chat.bounds("chat-code-pinned").expect("a pinned header");
    assert!(
        (pinned.top() - transcript.top()).abs() <= px(1.),
        "{pinned:?} {transcript:?}"
    );
    assert!(chat.bounds("chat-copy-pinned").is_some());

    chat.click("chat-apply-pinned");
    assert_eq!(
        chat.take_events(),
        [ChatViewEvent::ApplyBlock { turn: 0, block: 0 }]
    );
    chat.click("chat-copy-pinned");
    let copied = chat.cx.read_from_clipboard().and_then(|item| item.text());
    let code = chat
        .view
        .read_with(chat.cx, |view, _| view.code_block(0, 0));
    assert_eq!(copied, code.map(|block| block.code));
    assert!(copied.is_some_and(|code| code.starts_with("deny_0 = 0\n")));

    // A wheel over the pinned header scrolls the transcript under it.
    let code = chat.bounds("md-code-0").unwrap();
    chat.wheel("chat-code-pinned", -40.);
    let scrolled = chat.bounds("md-code-0").unwrap();
    assert_eq!(scrolled.top(), code.top() - px(40.));
    assert_eq!(chat.pinned(), Some((0, 0)));

    // Between the blocks, neither is pinned.
    chat.scroll_to(between);
    assert_eq!(chat.pinned(), None);
    assert!(chat.bounds("chat-code-pinned").is_none());

    // Into the second block.
    chat.scroll_to(second + px(100.));
    assert_eq!(chat.pinned(), Some((0, 1)));
    let transcript = chat.bounds("chat-transcript").unwrap();
    let pinned = chat.bounds("chat-code-pinned").expect("a pinned header");
    assert!((pinned.top() - transcript.top()).abs() <= px(1.));
    chat.click("chat-apply-pinned");
    assert_eq!(
        chat.take_events(),
        [ChatViewEvent::ApplyBlock { turn: 0, block: 1 }]
    );
}

#[gpui::test]
fn the_pinned_header_of_a_streaming_reply_cannot_apply(cx: &mut TestAppContext) {
    let (sender, receiver) = mpsc::unbounded();
    let provider = Arc::new(Channel(Mutex::new(Some(receiver))));
    let mut chat = open(cx, provider);
    for event in started() {
        sender.unbounded_send(event).unwrap();
    }
    sender.unbounded_send(delta(&markdown())).unwrap();
    chat.settle();
    assert!(chat.view.read_with(chat.cx, |view, _| view.is_streaming()));

    let first = chat.content_top("md-code-0");
    chat.scroll_to(first + px(100.));
    assert_eq!(chat.pinned(), Some((0, 0)));
    chat.click("chat-apply-pinned");
    assert_eq!(chat.take_events(), []);
    assert!(chat.view.read_with(chat.cx, |view, _| view.is_streaming()));
    drop(sender);
}

/// A finished reply of `markdown`.
fn finished(markdown: &str) -> Vec<ChatEvent> {
    let mut reply = started().to_vec();
    reply.extend([
        delta(markdown),
        ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "text", "text": markdown}),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        },
    ]);
    reply
}

#[gpui::test]
fn a_wrapped_header_needs_its_own_height_in_view(cx: &mut TestAppContext) {
    let lines: String = (0..80).map(|ix| format!("line_{ix}\n")).collect();
    let long = "src/a/rather/deeply/nested/folder/of/the/project/with/a/long/file/name.rs";
    let markdown =
        format!("```rust path={long}\n{lines}```\n\nAnd:\n\n```rust path=b.rs\n{lines}```\n");
    let mut chat = open(cx, Arc::new(ScriptedProvider::new([finished(&markdown)])));
    chat.scroll_to(px(0.));
    let tall = chat.bounds("chat-code-0-0").unwrap().size.height;
    let short = chat.bounds("chat-code-0-1").unwrap().size.height;
    assert!(tall > short + px(8.), "{tall:?} {short:?}: the path wraps");
    let second = chat.content_top("md-code-1");
    let first_bottom = {
        let transcript = chat.bounds("chat-transcript").unwrap();
        chat.bounds("md-code-0").unwrap().bottom() - transcript.top()
    };
    assert!(first_bottom < second);

    // Less of the first block in view than its own header's height, though
    // more than the second's: nothing is pinned.
    chat.scroll_to(first_bottom - (tall - px(4.)));
    assert_eq!(chat.pinned(), None);
    assert!(chat.bounds("chat-code-pinned").is_none());
    // Its header's height and a little more: pinned, and drawn as tall.
    chat.scroll_to(first_bottom - (tall + px(2.)));
    assert_eq!(chat.pinned(), Some((0, 0)));
    let pinned = chat.bounds("chat-code-pinned").expect("a pinned header");
    assert_eq!(pinned.size.height, tall);
}

#[gpui::test]
fn no_header_is_pinned_in_a_transcript_shorter_than_one(cx: &mut TestAppContext) {
    let mut chat = open(cx, Arc::new(ScriptedProvider::new([finished(&markdown())])));
    let first = chat.content_top("md-code-0");
    chat.scroll_to(first + px(100.));
    assert_eq!(chat.pinned(), Some((0, 0)));
    let header = chat.bounds("chat-code-pinned").unwrap().size.height;
    let transcript = chat.bounds("chat-transcript").unwrap().size.height;

    // The window shrunk until the transcript is shorter than the header.
    let window = px(300.) - transcript + header / 2.;
    chat.cx.simulate_resize(size(px(400.), window));
    chat.settle();
    let transcript = chat.bounds("chat-transcript").unwrap();
    assert!(transcript.size.height < header, "{transcript:?}");
    assert_eq!(chat.pinned(), None);
    assert!(chat.bounds("chat-code-pinned").is_none());

    // And back.
    chat.cx.simulate_resize(size(px(400.), px(300.)));
    chat.settle();
    assert_eq!(chat.pinned(), Some((0, 0)));
    let transcript = chat.bounds("chat-transcript").unwrap();
    let pinned = chat.bounds("chat-code-pinned").expect("a pinned header");
    assert!(pinned.bottom() <= transcript.bottom());
}
