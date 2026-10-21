//! The chips before Send: the host's context and the user's attachments,
//! each with its estimate and what the context budget leaves of it; and
//! the `@` and Attach that ask the host for a file.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use fe_buffer::ResolvedSelection;
use fe_chat::{
    BlockKind, ChatEvent, ChatProvider, ChatRequest, ChatSettings, ContextItem, ContextKind,
    Conversation, Rows, ScriptedProvider, StopReason,
};
use fe_chat_view::{ChatOptions, ChatView, ChatViewEvent, SendMessage};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use serde_json::json;

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

struct Chat<'a> {
    view: Entity<ChatView>,
    cx: &'a mut VisualTestContext,
    events: Rc<RefCell<Vec<ChatViewEvent>>>,
}

/// A focused chat over `provider`.
fn open(cx: &mut TestAppContext, provider: Arc<ScriptedProvider>) -> Chat<'_> {
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_chat_view::init(cx);
    });
    let conversation = Conversation::new(&ChatSettings::default());
    let provider: Arc<dyn ChatProvider> = provider;
    let (view, cx) = cx.add_window_view(move |window, cx| {
        ChatView::new(provider, conversation, ChatOptions::default(), window, cx)
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

    fn shown(&mut self, selector: &str) -> bool {
        self.cx.debug_bounds(selector.to_owned().leak()).is_some()
    }

    fn click(&mut self, selector: &str) {
        let bounds = (self.cx.debug_bounds(selector.to_owned().leak()))
            .unwrap_or_else(|| panic!("no {selector} drawn"));
        self.cx.simulate_click(bounds.center(), Modifiers::none());
        self.settle();
    }

    fn chips(&mut self) -> Vec<String> {
        self.view.read_with(self.cx, |view, _| {
            (0..view.chips().len())
                .map(|index| view.chip_text(index).unwrap())
                .collect()
        })
    }

    fn composer(&mut self) -> String {
        self.view
            .read_with(self.cx, |view, cx| view.composer_text(cx))
    }

    fn send(&mut self, text: &str) {
        self.view
            .update(self.cx, |view, cx| view.set_composer_text(text, cx));
        self.cx.dispatch_action(SendMessage);
        self.settle();
    }

    fn take_events(&mut self) -> Vec<ChatViewEvent> {
        std::mem::take(&mut self.events.borrow_mut())
    }
}

/// The tags sent with the last request, before its question.
fn tags(request: &ChatRequest) -> Vec<String> {
    let content = &request.messages.last().unwrap().content;
    (content[..content.len() - 1].iter())
        .map(|block| block["text"].as_str().unwrap().to_owned())
        .collect()
}

fn file_kind(path: &str) -> ContextKind {
    ContextKind::File {
        path: path.to_owned(),
        rows: None,
    }
}

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

fn item(kind: ContextKind, label: &str, text: &str) -> ContextItem {
    ContextItem {
        kind,
        label: label.to_owned(),
        text: text.to_owned(),
    }
}

/// `count` rows of 9 bytes each: 3 tokens a row.
fn rows(count: usize) -> String {
    (0..count).map(|row| format!("row {row:04}\n")).collect()
}

#[gpui::test]
fn chips_show_estimate_and_budget(cx: &mut TestAppContext) {
    let provider = Arc::new(ScriptedProvider::new([
        reply("One."),
        reply("Two."),
        reply("Three."),
        reply("Four."),
        reply("Five."),
        refusal(),
        reply("Six."),
        reply("Seven."),
        refusal(),
        reply("Nine."),
    ]));
    let mut chat = open(cx, provider.clone());
    let selection = item(
        ContextKind::Selection {
            path: "src/lib.rs".to_owned(),
            rows: Rows { first: 1, last: 2 },
        },
        "lib.rs:1-2",
        "fn a() {}\n}\n",
    );
    let active = item(
        ContextKind::ActiveFile {
            path: "src/main.rs".to_owned(),
            rows: None,
        },
        "main.rs",
        &rows(100),
    );
    let open_editors = item(ContextKind::OpenEditors, "Open Editors", "src/main.rs\n");
    let big = item(
        ContextKind::File {
            path: "big.txt".to_owned(),
            rows: None,
        },
        "big.txt",
        &"x".repeat(3600),
    );

    // 4 for the selection, 1200 for the file, room for 50 of the active
    // file's 100 rows; the open editors don't fit after it.
    chat.view.update(chat.cx, |view, cx| {
        view.set_context_budget(4 + 1200 + 150, cx);
        view.set_context(
            vec![open_editors.clone(), active.clone(), selection.clone()],
            cx,
        );
        view.attach(big.clone(), cx);
    });
    chat.settle();
    assert_eq!(
        chat.chips(),
        [
            "lib.rs:1-2 · ~4",
            "big.txt · ~1.2k",
            "main.rs · ~300 · truncated",
            "Open Editors · ~4 · won't fit",
        ]
    );
    for index in 0..4 {
        assert!(chat.shown(&format!("chat-chip-{index}")), "{index}");
    }
    assert!(!chat.shown("chat-chip-4"));

    // Taking off the file and the selection: neither goes with the next
    // request, and the rest then fit whole.
    chat.click("chat-chip-remove-1");
    chat.click("chat-chip-remove-0");
    assert_eq!(chat.chips(), ["main.rs · ~300", "Open Editors · ~4"]);
    assert!(
        chat.view
            .read_with(chat.cx, |view, _| view.attachments().is_empty())
    );
    // The host gathering again doesn't bring back what was taken off.
    chat.view.update(chat.cx, |view, cx| {
        view.set_context(vec![selection.clone(), active.clone()], cx)
    });
    chat.settle();
    assert_eq!(chat.chips(), ["main.rs · ~300"]);
    chat.send("Why?");
    let requests = provider.requests();
    let sent = tags(&requests[0]);
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0].starts_with("<file path=\"src/main.rs\">\nrow 0000\n"),
        "{}",
        sent[0]
    );
    assert_eq!(
        chat.view
            .read_with(chat.cx, |view, _| view.conversation().turns[0]
                .attachments
                .iter()
                .map(|a| a.label.clone())
                .collect::<Vec<_>>()),
        ["main.rs"]
    );

    // Once sent, the host's chips the user took off come back, and the
    // user's attachments are gone.
    assert_eq!(chat.chips(), ["lib.rs:1-2 · ~4", "main.rs · ~300"]);
    chat.send("And?");
    let requests = provider.requests();
    let sent = tags(&requests[1]);
    assert_eq!(sent.len(), 2);
    assert!(sent[0].starts_with("<selection path=\"src/lib.rs\" rows=\"1-2\">"));

    // An @ typed at the start of a word asks for a file and is taken out;
    // one inside a word is typed.
    chat.view
        .update(chat.cx, |view, cx| view.set_composer_text("", cx));
    chat.settle();
    chat.take_events();
    chat.cx.simulate_input("see @");
    chat.settle();
    assert_eq!(chat.composer(), "see ");
    assert_eq!(chat.take_events(), [ChatViewEvent::AttachFileRequested]);
    chat.cx.simulate_input("x@y");
    chat.settle();
    assert_eq!(chat.composer(), "see x@y");
    assert_eq!(chat.take_events(), []);
    chat.view
        .update(chat.cx, |view, cx| view.set_composer_text("", cx));
    chat.cx.simulate_input("@");
    chat.settle();
    assert_eq!(chat.composer(), "");
    assert_eq!(chat.take_events(), [ChatViewEvent::AttachFileRequested]);

    // As does the Attach button.
    chat.click("chat-attach");
    assert_eq!(chat.take_events(), [ChatViewEvent::AttachFileRequested]);

    // Undo and redo after an @ was taken out neither bring it back nor
    // ask again.
    chat.view
        .update(chat.cx, |view, cx| view.set_composer_text("", cx));
    chat.cx.simulate_input("see @");
    chat.settle();
    assert_eq!(chat.composer(), "see ");
    assert_eq!(chat.take_events(), [ChatViewEvent::AttachFileRequested]);
    let composer = chat
        .view
        .read_with(chat.cx, |view, _| view.composer().clone());
    let mut texts = Vec::new();
    for undo in [true, true, false, false, true] {
        composer.update(chat.cx, |composer, cx| {
            let model = composer.model_mut();
            if undo {
                model.undo().unwrap();
            } else {
                model.redo().unwrap();
            }
            cx.notify();
        });
        chat.settle();
        texts.push(chat.composer());
        assert_eq!(chat.take_events(), []);
    }
    assert!(texts.iter().all(|text| !text.contains('@')), "{texts:?}");
    assert!(texts.contains(&String::new()), "{texts:?}");

    // An @ typed over a selection at a word's start replaces it and asks.
    chat.view
        .update(chat.cx, |view, cx| view.set_composer_text("see that", cx));
    composer.update(chat.cx, |composer, cx| {
        let selection = ResolvedSelection {
            range: 4..8,
            reversed: false,
        };
        composer.model_mut().set_selections([selection]);
        cx.notify();
    });
    chat.settle();
    chat.take_events();
    chat.cx.simulate_input("@");
    chat.settle();
    assert_eq!(chat.composer(), "see ");
    assert_eq!(chat.take_events(), [ChatViewEvent::AttachFileRequested]);

    // Files still being read keep their places, and nothing is sent until
    // they're read.
    let (a, b) = (
        item(file_kind("a.rs"), "a.rs", "a\n"),
        item(file_kind("b.rs"), "b.rs", "b\n"),
    );
    chat.view
        .update(chat.cx, |view, cx| view.set_context(Vec::new(), cx));
    let (first, second) = chat.view.update(chat.cx, |view, cx| {
        (
            view.reserve(file_kind("a.rs"), "a.rs".to_owned(), cx),
            view.reserve(file_kind("b.rs"), "b.rs".to_owned(), cx),
        )
    });
    chat.view
        .update(chat.cx, |view, cx| view.fill(second, Some(b.clone()), cx));
    chat.settle();
    assert!(chat.shown("chat-chip-reading-0"));
    assert_eq!(chat.chips(), ["b.rs · ~1"]);
    chat.send("Both?");
    assert_eq!(provider.requests().len(), 2, "not sent while reading");
    chat.view
        .update(chat.cx, |view, cx| view.fill(first, Some(a.clone()), cx));
    chat.settle();
    assert!(!chat.shown("chat-chip-reading-0"));
    chat.cx.dispatch_action(SendMessage);
    chat.settle();
    let requests = provider.requests();
    assert_eq!(
        tags(&requests[2]),
        [
            "<file path=\"a.rs\">\na\n</file>",
            "<file path=\"b.rs\">\nb\n</file>"
        ]
    );
    // One filled after it was taken off, or after the send, is dropped.
    let late = chat.view.update(chat.cx, |view, cx| {
        view.reserve(file_kind("c.rs"), "c.rs".to_owned(), cx)
    });
    chat.settle();
    chat.click("chat-chip-reading-remove-0");
    chat.view
        .update(chat.cx, |view, cx| view.fill(late, Some(a.clone()), cx));
    assert_eq!(
        chat.view.read_with(chat.cx, |view, _| view.attachments()),
        []
    );

    // A file the user took off while the host had no copy of it (its
    // file being vetted) keeps the copy gathered later off, until the
    // send.
    chat.view.update(chat.cx, |view, cx| {
        view.set_context(Vec::new(), cx);
        view.attach(item(file_kind("b.rs"), "b.rs", "b\n"), cx);
    });
    chat.settle();
    chat.click("chat-chip-remove-0");
    let b_active = item(
        ContextKind::ActiveFile {
            path: "b.rs".to_owned(),
            rows: None,
        },
        "b.rs",
        "b\n",
    );
    chat.view.update(chat.cx, |view, cx| {
        view.set_context(vec![b_active.clone()], cx)
    });
    chat.settle();
    assert_eq!(chat.chips(), Vec::<String>::new(), "gathered after");
    chat.send("Not b?");
    assert_eq!(tags(&provider.requests()[3]), Vec::<String>::new());
    // Once sent, the host's copy is back.
    assert_eq!(chat.chips(), ["b.rs · ~1"]);
    chat.view
        .update(chat.cx, |view, cx| view.set_context(Vec::new(), cx));

    // The active file the user attached too goes once; taking it off
    // takes the host's copy off too, until the send.
    let active = item(
        ContextKind::ActiveFile {
            path: "a.rs".to_owned(),
            rows: None,
        },
        "a.rs",
        "a\n",
    );
    chat.view.update(chat.cx, |view, cx| {
        view.set_context(vec![active.clone()], cx);
        view.attach(a.clone(), cx);
    });
    chat.settle();
    assert_eq!(chat.chips(), ["a.rs · ~1"]);
    chat.click("chat-chip-remove-0");
    assert_eq!(chat.chips(), Vec::<String>::new());
    chat.view.update(chat.cx, |view, cx| {
        view.set_context(vec![active.clone()], cx)
    });
    chat.settle();
    assert_eq!(chat.chips(), Vec::<String>::new(), "gathered again");
    chat.send("Without?");
    assert_eq!(tags(&provider.requests()[4]), Vec::<String>::new());

    // Edit after a refusal brings back none of the host's context, which
    // still stands: neither the active file nor a selection with no
    // newline at its end, which the tag gave it.
    let selected = item(
        ContextKind::Selection {
            path: "a.rs".to_owned(),
            rows: Rows { first: 1, last: 1 },
        },
        "a.rs:1-1",
        "a",
    );
    chat.view.update(chat.cx, |view, cx| {
        view.set_context(vec![selected.clone(), active.clone()], cx)
    });
    chat.settle();
    assert_eq!(chat.chips(), ["a.rs:1-1 · ~1", "a.rs · ~1"]);
    chat.send("Refuse?");
    chat.click("chat-banner-action-5");
    assert_eq!(
        chat.view.read_with(chat.cx, |view, _| view.attachments()),
        []
    );
    assert_eq!(chat.chips(), ["a.rs:1-1 · ~1", "a.rs · ~1"]);
    chat.cx.dispatch_action(SendMessage);
    chat.settle();
    let requests = provider.requests();
    assert_eq!(
        tags(&requests[6]),
        [
            "<selection path=\"a.rs\" rows=\"1-1\">\na\n</selection>",
            "<file path=\"a.rs\">\na\n</file>"
        ]
    );

    // A file taken off while still being read keeps the host's copy of it
    // off too, gathered later, until the send.
    chat.view.update(chat.cx, |view, cx| {
        view.set_context(Vec::new(), cx);
        view.reserve(file_kind("c.rs"), "c.rs".to_owned(), cx);
    });
    chat.settle();
    chat.click("chat-chip-reading-remove-0");
    let c_active = item(
        ContextKind::ActiveFile {
            path: "c.rs".to_owned(),
            rows: None,
        },
        "c.rs",
        "c\n",
    );
    chat.view.update(chat.cx, |view, cx| {
        view.set_context(vec![c_active.clone()], cx)
    });
    chat.settle();
    assert_eq!(chat.chips(), Vec::<String>::new());
    chat.send("Not c?");
    assert_eq!(tags(&provider.requests()[7]), Vec::<String>::new());

    // Edit brings back only what the user attached: none of what the host
    // gathered, whether or not it gathers it now (say while it's vetted
    // again, or once trust is revoked).
    chat.view.update(chat.cx, |view, cx| {
        view.set_context(vec![c_active.clone()], cx);
        view.attach(b.clone(), cx);
    });
    chat.send("Refuse again?");
    assert_eq!(tags(&provider.requests()[8]).len(), 2);
    chat.view
        .update(chat.cx, |view, cx| view.set_context(Vec::new(), cx));
    chat.settle();
    chat.click("chat-banner-action-8");
    assert_eq!(
        chat.view.read_with(chat.cx, |view, _| view.attachments()),
        std::slice::from_ref(&b)
    );
    assert_eq!(chat.chips(), ["b.rs · ~1"]);
    chat.cx.dispatch_action(SendMessage);
    chat.settle();
    assert_eq!(
        tags(&provider.requests()[9]),
        ["<file path=\"b.rs\">\nb\n</file>"]
    );
}
