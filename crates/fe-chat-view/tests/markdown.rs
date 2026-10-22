//! Replies drawn as Markdown in a test window, with a text system that
//! keeps each line it lays out: a reply streaming in, and the code block
//! headers and links of one finished.

use std::{
    borrow::Cow,
    cell::Cell,
    rc::Rc,
    sync::{Arc, Mutex},
};

use fe_chat::{
    BlockKind, ChatEvent, ChatProvider, ChatRequest, ChatSettings, ChatStream, Conversation,
    ScriptedProvider, StopReason, TurnState,
};
use fe_chat_view::{ChatOptions, ChatView, ChatViewEvent, CodeBlock, SendMessage};
use futures::channel::mpsc;
use gpui::{
    AppContext as _, Bounds, Context, DevicePixels, Entity, Focusable as _, IntoElement, Modifiers,
    MouseButton, NoopTextSystem, ParentElement as _, PlatformTextSystem, Render, Size, Styled as _,
    TestAppContext, TestDispatcher, VisualTestContext, Window, div, point, px, size,
};
use serde_json::json;

/// Each line of text laid out, in order.
type Lines = Arc<Mutex<Vec<String>>>;

/// The test platform's text system, which lays text out but paints no
/// glyph, keeping each line it lays out in `lines`.
fn text_system(lines: Lines) -> Arc<dyn PlatformTextSystem> {
    struct Recording(NoopTextSystem, Lines);

    impl PlatformTextSystem for Recording {
        fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> gpui::Result<()> {
            self.0.add_fonts(fonts)
        }
        fn all_font_names(&self) -> Vec<String> {
            self.0.all_font_names()
        }
        fn font_id(&self, descriptor: &gpui::Font) -> gpui::Result<gpui::FontId> {
            self.0.font_id(descriptor)
        }
        fn font_metrics(&self, font_id: gpui::FontId) -> gpui::FontMetrics {
            self.0.font_metrics(font_id)
        }
        fn typographic_bounds(
            &self,
            font_id: gpui::FontId,
            glyph_id: gpui::GlyphId,
        ) -> gpui::Result<Bounds<f32>> {
            self.0.typographic_bounds(font_id, glyph_id)
        }
        fn advance(
            &self,
            font_id: gpui::FontId,
            glyph_id: gpui::GlyphId,
        ) -> gpui::Result<Size<f32>> {
            self.0.advance(font_id, glyph_id)
        }
        fn glyph_for_char(&self, font_id: gpui::FontId, ch: char) -> Option<gpui::GlyphId> {
            self.0.glyph_for_char(font_id, ch)
        }
        fn glyph_raster_bounds(
            &self,
            _: &gpui::RenderGlyphParams,
        ) -> gpui::Result<Bounds<DevicePixels>> {
            Ok(Bounds {
                origin: Default::default(),
                size: size(DevicePixels(8), DevicePixels(8)),
            })
        }
        fn rasterize_glyph(
            &self,
            _: &gpui::RenderGlyphParams,
            bounds: Bounds<DevicePixels>,
        ) -> gpui::Result<(Size<DevicePixels>, Vec<u8>)> {
            Ok((bounds.size, vec![255; 64]))
        }
        fn layout_line(
            &self,
            text: &str,
            font_size: gpui::Pixels,
            runs: &[gpui::FontRun],
        ) -> gpui::LineLayout {
            self.1.lock().expect("the lines").push(text.to_owned());
            self.0.layout_line(text, font_size, runs)
        }
        fn recommended_rendering_mode(
            &self,
            font_id: gpui::FontId,
            font_size: gpui::Pixels,
        ) -> gpui::TextRenderingMode {
            self.0.recommended_rendering_mode(font_id, font_size)
        }
    }

    Arc::new(Recording(NoopTextSystem::new(), lines))
}

/// Runs `test` in an app whose text system keeps the lines it lays out,
/// then shuts the app down as `#[gpui::test]` does.
fn with_app(name: &'static str, test: impl FnOnce(&mut TestAppContext, Lines)) {
    let lines = Lines::default();
    let mut cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        Some(name),
        text_system(lines.clone()),
    );
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_chat_view::init(cx);
    });
    test(&mut cx, lines);
    cx.run_until_parked();
    cx.update(|cx| {
        cx.background_executor().forbid_parking();
        cx.quit();
    });
    cx.run_until_parked();
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

/// The window's root: the chat, counting the frames drawn.
struct Host {
    chat: Entity<ChatView>,
    frames: Rc<Cell<usize>>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.frames.set(self.frames.get() + 1);
        div().size_full().child(self.chat.clone())
    }
}

struct Chat<'a> {
    view: Entity<ChatView>,
    cx: &'a mut VisualTestContext,
    frames: Rc<Cell<usize>>,
    events: Rc<std::cell::RefCell<Vec<ChatViewEvent>>>,
    lines: Lines,
}

/// A focused chat over `provider`, showing thinking or not, a question
/// sent.
fn open<'a>(
    cx: &'a mut TestAppContext,
    provider: Arc<dyn ChatProvider>,
    show_thinking: bool,
    lines: Lines,
) -> Chat<'a> {
    let conversation = Conversation::new(&ChatSettings {
        show_thinking,
        ..ChatSettings::default()
    });
    let frames = Rc::new(Cell::new(0));
    let counter = frames.clone();
    let (host, cx) = cx.add_window_view(move |window, cx| {
        let chat =
            cx.new(|cx| ChatView::new(provider, conversation, ChatOptions::default(), window, cx));
        Host {
            chat,
            frames: counter,
        }
    });
    let view = host.read_with(cx, |host, _| host.chat.clone());
    let events: Rc<std::cell::RefCell<Vec<ChatViewEvent>>> = Rc::default();
    let log = Rc::clone(&events);
    cx.update(|window, cx| {
        cx.subscribe(&view, move |_, event: &ChatViewEvent, _| {
            log.borrow_mut().push(event.clone())
        })
        .detach();
        view.focus_handle(cx).focus(window, cx);
    });
    let mut chat = Chat {
        view,
        cx,
        frames,
        events,
        lines,
    };
    chat.send("Show me");
    chat
}

impl Chat<'_> {
    fn send(&mut self, question: &str) {
        self.view
            .update(self.cx, |view, cx| view.set_composer_text(question, cx));
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(SendMessage), cx));
        self.settle();
    }

    fn settle(&mut self) {
        for _ in 0..3 {
            self.cx.run_until_parked();
            self.cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    }

    fn bounds(&mut self, selector: String) -> Option<Bounds<gpui::Pixels>> {
        self.cx.debug_bounds(selector.leak())
    }

    fn click(&mut self, selector: String) {
        let bounds =
            (self.bounds(selector.clone())).unwrap_or_else(|| panic!("no {selector} drawn"));
        self.cx.simulate_click(bounds.center(), Modifiers::none());
        self.settle();
    }

    fn laid_out(&self, line: &str) -> bool {
        self.lines.lock().unwrap().iter().any(|laid| laid == line)
    }

    fn laid_out_containing(&self, text: &str) -> Vec<String> {
        let lines = self.lines.lock().unwrap();
        lines
            .iter()
            .filter(|line| line.contains(text))
            .cloned()
            .collect()
    }

    fn take_events(&mut self) -> Vec<ChatViewEvent> {
        std::mem::take(&mut self.events.borrow_mut())
    }

    /// Frames drawn and parses made so far.
    fn counts(&mut self) -> (usize, usize) {
        let parses = self.view.read_with(self.cx, |view, _| view.parses());
        (self.frames.get(), parses)
    }
}

fn delta(text: &str) -> ChatEvent {
    ChatEvent::TextDelta {
        index: 0,
        text: text.to_owned(),
    }
}

#[test]
fn replies_render_as_markdown_while_streaming() {
    with_app("replies_render_as_markdown_while_streaming", |cx, lines| {
        let (sender, receiver) = mpsc::unbounded();
        let provider = Arc::new(Channel(Mutex::new(Some(receiver))));
        let mut chat = open(cx, provider, false, lines);
        let send = |event| sender.unbounded_send(event).expect("the stream is open");
        send(ChatEvent::Started {
            model: "claude-opus-5-5".to_owned(),
            message_id: "msg_1".to_owned(),
        });
        send(ChatEvent::BlockStarted {
            index: 0,
            kind: BlockKind::Text,
        });
        chat.settle();
        let (frames, parses) = chat.counts();

        // The deltas split the fence's backticks. With the first two come,
        // they're a paragraph after the list; with the rest of the line,
        // the block, half come, is drawn as code under its header.
        let first = ["# Title\n\n- one\n- two\n\n``", "`rust\nfn main() {\n"];
        send(delta(first[0]));
        chat.settle();
        assert!(chat.bounds("md-block-2".into()).is_some(), "the backticks");
        assert!(chat.bounds("md-code-0".into()).is_none());
        send(delta(first[1]));
        chat.settle();
        assert!(chat.bounds("md-code-0".into()).is_some(), "the code");
        assert!(
            chat.bounds("chat-code-0-0".into()).is_some(),
            "the code header"
        );
        assert!(chat.bounds("md-block-3".into()).is_none());
        assert!(chat.laid_out("fn main() {"));
        assert!(chat.laid_out("rust"), "the header's language");
        // Apply waits for the block to finish.
        chat.click("chat-apply-0-0".into());
        assert_eq!(chat.take_events(), []);
        let (frames_now, parses_now) = chat.counts();
        let (drawn, parsed) = (frames_now - frames, parses_now - parses);
        assert!(
            (1..=drawn).contains(&parsed),
            "{parsed} parses in {drawn} frames"
        );

        // Frames with nothing new parse nothing.
        chat.settle();
        assert_eq!(chat.counts().1, parses_now);

        let (frames, parses) = chat.counts();
        // The rest in deltas that come together.
        let rest = ["    println!(\"hi\");\n}\n``", "`\n\nDone.\n"];
        for text in rest {
            send(delta(text));
        }
        chat.settle();
        let (frames_now, parses_now) = chat.counts();
        let (drawn, parsed) = (frames_now - frames, parses_now - parses);
        assert!(
            (1..=drawn).contains(&parsed),
            "{parsed} parses in {drawn} frames"
        );

        // The turn finishing in a later frame, with no more text, parses it
        // once more.
        let parses = parses_now;
        let rest = rest.concat();
        let whole = format!("{}{rest}", first.concat());
        send(ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "text", "text": whole}),
        });
        send(ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        });
        chat.settle();
        let state = chat.view.read_with(chat.cx, |view, _| {
            view.conversation().turns[0].state.clone()
        });
        assert_eq!(state, TurnState::Done);
        assert_eq!(chat.counts().1, parses + 1);
        chat.settle();
        assert_eq!(chat.counts().1, parses + 1);

        // The finished reply: the heading, the list, the code and the
        // paragraph after it, with no Markdown drawn as text.
        for ix in 0..4 {
            assert!(
                chat.bounds(format!("md-block-{ix}")).is_some(),
                "block {ix}"
            );
        }
        assert!(chat.bounds("md-block-4".into()).is_none());
        let heading = chat.bounds("md-block-0".into()).unwrap();
        let paragraph = chat.bounds("md-block-3".into()).unwrap();
        assert!(
            heading.size.height > paragraph.size.height * 1.5,
            "{heading:?} {paragraph:?}"
        );
        for line in [
            "Title",
            "•",
            "one",
            "two",
            "fn main() {",
            "    println!(\"hi\");",
            "}",
            "Done.",
        ] {
            assert!(chat.laid_out(line), "{line:?} laid out");
        }
        assert_eq!(chat.laid_out_containing("```"), Vec::<String>::new());
        assert_eq!(chat.laid_out_containing("# "), Vec::<String>::new());
        assert_eq!(chat.laid_out_containing("- "), Vec::<String>::new());
        assert!(chat.bounds("chat-code-0-0".into()).is_some());
        assert!(chat.bounds("chat-stop".into()).is_none());
        assert_eq!(chat.take_events(), [ChatViewEvent::TurnFinished]);
    });
}

#[test]
fn code_blocks_get_copy_and_apply() {
    with_app("code_blocks_get_copy_and_apply", |cx, lines| {
        let code = "pub fn one() -> u32 {\n    1\n}\n";
        let markdown = format!(
            "See [the docs](https://example.com/docs) or [lib](src/lib.rs#L12).\n\n\
             ```sh\ncargo test\n```\n\n\
             ```rust path=src/lib.rs\n{code}```\n"
        );
        let reply = vec![
            ChatEvent::Started {
                model: "claude-opus-5-5".to_owned(),
                message_id: "msg_1".to_owned(),
            },
            ChatEvent::BlockStarted {
                index: 0,
                kind: BlockKind::Text,
            },
            delta(&markdown),
            ChatEvent::BlockFinished {
                index: 0,
                block: json!({"type": "text", "text": markdown}),
            },
            ChatEvent::Finished {
                stop_reason: StopReason::EndTurn,
                stop_details: None,
            },
        ];
        let provider = Arc::new(ScriptedProvider::new([reply]));
        let mut chat = open(cx, provider, false, lines);
        assert_eq!(chat.take_events(), [ChatViewEvent::TurnFinished]);

        // Each fence has a header; the second names its language and file.
        assert!(chat.bounds("chat-code-0-0".into()).is_some());
        let header = chat
            .bounds("chat-code-0-1".into())
            .expect("the second header");
        let code_box = chat.bounds("md-code-1".into()).expect("the second block");
        assert!(header.bottom() <= code_box.top());
        assert!(chat.laid_out("sh"));
        assert!(chat.laid_out("rust"));
        assert!(chat.laid_out("src/lib.rs"));
        let block = chat
            .view
            .read_with(chat.cx, |view, _| view.code_block(0, 1));
        assert_eq!(
            block,
            Some(CodeBlock {
                language: Some("rust".to_owned()),
                path: Some("src/lib.rs".to_owned()),
                code: code.to_owned(),
            })
        );

        // Copy puts the code on the clipboard.
        chat.click("chat-copy-0-1".into());
        let copied = chat.cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(copied.as_deref(), Some(code));

        // Apply asks the host to apply that block of that turn.
        chat.click("chat-apply-0-1".into());
        assert_eq!(
            chat.take_events(),
            [ChatViewEvent::ApplyBlock { turn: 0, block: 1 }]
        );

        // Across the paragraph: the URL opens in the browser, and the
        // path asks the host to open it at row 12.
        let paragraph = chat.bounds("md-block-0".into()).expect("the paragraph");
        let y = paragraph.origin.y + px(8.);
        let mut x = paragraph.origin.x;
        while x < paragraph.right() {
            chat.cx.simulate_click(point(x, y), Modifiers::none());
            x += px(4.);
        }
        chat.settle();
        assert_eq!(
            chat.cx.opened_url().as_deref(),
            Some("https://example.com/docs")
        );
        let mut events = chat.take_events();
        events.dedup();
        assert_eq!(
            events,
            [ChatViewEvent::OpenPath {
                path: "src/lib.rs".to_owned(),
                row: Some(12),
            }]
        );
    });
}

/// A reply whose text is `markdown`, finished.
fn finished(markdown: &str, thinking: Option<&str>) -> Vec<ChatEvent> {
    let mut events = vec![ChatEvent::Started {
        model: "claude-opus-5-5".to_owned(),
        message_id: "msg".to_owned(),
    }];
    let mut index = 0;
    if let Some(thinking) = thinking {
        events.extend([
            ChatEvent::BlockStarted {
                index,
                kind: BlockKind::Thinking,
            },
            ChatEvent::ThinkingDelta {
                index,
                text: thinking.to_owned(),
            },
            ChatEvent::BlockFinished {
                index,
                block: json!({"type": "thinking", "thinking": thinking, "signature": "sig"}),
            },
        ]);
        index += 1;
    }
    events.extend([
        ChatEvent::BlockStarted {
            index,
            kind: BlockKind::Text,
        },
        ChatEvent::TextDelta {
            index,
            text: markdown.to_owned(),
        },
        ChatEvent::BlockFinished {
            index,
            block: json!({"type": "text", "text": markdown}),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        },
    ]);
    events
}

#[test]
fn each_reply_keeps_its_own_link_state() {
    with_app("each_reply_keeps_its_own_link_state", |cx, lines| {
        // The same text in each document, which numbers its texts alike.
        let provider = Arc::new(ScriptedProvider::new([
            finished("[lib](src/a.rs)\n\n```rust\nfn a() {}\n```\n", None),
            finished(
                "[lib](src/b.rs)\n\n```rust\nfn b() {}\n```\n",
                Some("[lib](src/c.rs)"),
            ),
        ]));
        let mut chat = open(cx, provider, true, lines);
        chat.send("And again");
        chat.view
            .update(chat.cx, |view, cx| view.set_thinking_open(1, true, cx));
        chat.settle();
        chat.take_events();
        let first = chat.bounds("chat-reply-0".into()).expect("the first reply");
        let second = chat
            .bounds("chat-reply-1".into())
            .expect("the second reply");
        let thinking = (chat.bounds("chat-thinking-text-1".into())).expect("the thinking");
        let on_link = |bounds: Bounds<gpui::Pixels>, indent: f32| {
            point(bounds.origin.x + px(indent + 2.), bounds.origin.y + px(8.))
        };
        let (first, second, thinking) = (
            on_link(first, 0.),
            on_link(second, 0.),
            on_link(thinking, 13.),
        );

        // Pressed on one document's link and let go on another's is no
        // click, as it would be if the two texts shared their state. (GPUI
        // keeps a text's press until the mouse comes up over that text, so
        // no release here is over a text pressed before.)
        for (down, up) in [(first, second), (first, thinking), (thinking, second)] {
            chat.cx
                .simulate_mouse_down(down, MouseButton::Left, Modifiers::none());
            chat.settle();
            chat.cx
                .simulate_mouse_up(up, MouseButton::Left, Modifiers::none());
            chat.settle();
            assert_eq!(
                chat.take_events(),
                [],
                "pressed at {down:?}, let go at {up:?}"
            );
        }

        // A click on each goes to its own link.
        for (at, path) in [
            (first, "src/a.rs"),
            (second, "src/b.rs"),
            (thinking, "src/c.rs"),
        ] {
            chat.cx.simulate_click(at, Modifiers::none());
            chat.settle();
            let open = ChatViewEvent::OpenPath {
                path: path.to_owned(),
                row: None,
            };
            assert_eq!(chat.take_events(), [open]);
        }

        // Each reply's code blocks are numbered from 0 in its own scope.
        for turn in 0..2 {
            assert!(chat.bounds(format!("chat-code-{turn}-0")).is_some());
        }
    });
}
