//! A reply's SEARCH/REPLACE code block drawn as a diff, in a test window
//! with a text system that keeps each line it lays out: its rows, its
//! header's Copy and Apply, the blocks that stay code, and the block still
//! streaming in.

use std::{
    borrow::Cow,
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
};

use fe_chat::{
    BlockKind, ChatEvent, ChatProvider, ChatRequest, ChatSettings, ChatStream, Conversation,
    ScriptedProvider, StopReason,
};
use fe_chat_view::{ChatOptions, ChatView, ChatViewEvent, SendMessage};
use fe_gpui::EditorSettings;
use futures::channel::mpsc;
use gpui::{
    Bounds, DevicePixels, Entity, Focusable as _, Modifiers, NoopTextSystem, PlatformTextSystem,
    Size, TestAppContext, TestDispatcher, VisualTestContext, size,
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

struct Chat<'a> {
    view: Entity<ChatView>,
    cx: &'a mut VisualTestContext,
    events: Rc<RefCell<Vec<ChatViewEvent>>>,
    lines: Lines,
}

/// A focused chat over `provider`, a question sent.
fn open(cx: &mut TestAppContext, provider: Arc<dyn ChatProvider>, lines: Lines) -> Chat<'_> {
    let conversation = Conversation::new(&ChatSettings::default());
    let (view, cx) = cx.add_window_view(move |window, cx| {
        ChatView::new(provider, conversation, ChatOptions::default(), window, cx)
    });
    let events: Rc<RefCell<Vec<ChatViewEvent>>> = Rc::default();
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
        events,
        lines,
    };
    chat.view
        .update(chat.cx, |view, cx| view.set_composer_text("Fix it", cx));
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

    fn bounds(&mut self, selector: String) -> Option<Bounds<gpui::Pixels>> {
        self.cx.debug_bounds(selector.leak())
    }

    fn drawn(&mut self, selector: &str) -> bool {
        self.bounds(selector.to_owned()).is_some()
    }

    fn click(&mut self, selector: &str) {
        let bounds =
            (self.bounds(selector.to_owned())).unwrap_or_else(|| panic!("no {selector} drawn"));
        self.cx.simulate_click(bounds.center(), Modifiers::none());
        self.settle();
    }

    fn laid_out(&self, line: &str) -> bool {
        self.lines.lock().unwrap().iter().any(|laid| laid == line)
    }

    /// Forgets the lines laid out so far and lays the code out again, in
    /// a code font a size larger: the window keeps the lines it last laid
    /// out, so a line drawn again isn't laid out again.
    fn relayout_code(&mut self) {
        let composer = self
            .view
            .read_with(self.cx, |view, _| view.composer().clone());
        let size = composer.read_with(self.cx, |composer, _| composer.theme().font_size);
        let settings = EditorSettings {
            font_size: f64::from(f32::from(size)) + 2.,
            ..EditorSettings::default()
        };
        // Cleared first: a window can draw as soon as it's notified.
        self.lines.lock().unwrap().clear();
        composer.update(self.cx, |composer, cx| {
            composer.apply_settings(&settings, cx)
        });
        self.settle();
    }

    /// How many rows of `kind` block `block` of turn 0 draws as a diff.
    fn rows(&mut self, block: usize, kind: &str) -> usize {
        (0..)
            .take_while(|k| self.drawn(&format!("chat-diff-0-{block}-{kind}-{k}")))
            .count()
    }

    fn take_events(&mut self) -> Vec<ChatViewEvent> {
        std::mem::take(&mut self.events.borrow_mut())
    }
}

/// A reply whose text is `markdown`, finished.
fn finished(markdown: &str) -> Vec<ChatEvent> {
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
            text: markdown.to_owned(),
        },
        ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "text", "text": markdown}),
        },
        ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        },
    ]
}

/// Two sections for `deny.toml`: two lines for two, then one for two.
const SECTIONS: &str = "<<<<<<< SEARCH
[bans]
multiple-versions = \"warn\"
=======
[bans]
multiple-versions = \"deny\"
>>>>>>> REPLACE

<<<<<<< SEARCH
[licenses]
=======
[licenses]
confidence-threshold = 0.9
>>>>>>> REPLACE
";

#[test]
fn a_search_replace_block_is_drawn_as_a_diff() {
    with_app("a_search_replace_block_is_drawn_as_a_diff", |cx, lines| {
        let markdown = format!("Tighten the bans:\n\n```toml path=deny.toml\n{SECTIONS}```\n");
        let provider = Arc::new(ScriptedProvider::new([finished(&markdown)]));
        let mut chat = open(cx, provider, lines);
        assert_eq!(chat.take_events(), [ChatViewEvent::TurnFinished]);
        assert!(chat.laid_out("deny.toml"));
        // Frames drew the block as code while it streamed in. Laid out
        // again at another size, the finished block lays out only its
        // rows.
        chat.relayout_code();

        // A row for each line of each side, in the block's body.
        let diff = chat.bounds("chat-diff-0-0".into()).expect("the diff");
        let code = chat.bounds("md-code-0".into()).expect("the block's body");
        assert!(code.contains(&diff.origin), "{diff:?} in {code:?}");
        assert_eq!(chat.rows(0, "removed"), 3);
        assert_eq!(chat.rows(0, "added"), 4);
        for line in [
            "[bans]",
            "multiple-versions = \"warn\"",
            "multiple-versions = \"deny\"",
            "[licenses]",
            "confidence-threshold = 0.9",
        ] {
            assert!(chat.laid_out(line), "{line:?} laid out");
        }
        for marker in ["<<<<<<< SEARCH", "=======", ">>>>>>> REPLACE"] {
            assert!(!chat.laid_out(marker), "{marker:?} laid out");
        }
        // Tinted from edge to edge of the body, removed rows over added.
        let removed = chat.bounds("chat-diff-0-0-removed-0".into()).unwrap();
        let added = chat.bounds("chat-diff-0-0-added-0".into()).unwrap();
        assert_eq!(
            (removed.left(), removed.right()),
            (code.left(), code.right())
        );
        assert_eq!(added.size.width, removed.size.width);
        assert!(removed.bottom() <= added.top(), "{removed:?} {added:?}");
        // The second section's rows come after a gap.
        let second = chat.bounds("chat-diff-0-0-removed-2".into()).unwrap();
        let last_added = chat.bounds("chat-diff-0-0-added-1".into()).unwrap();
        assert!(
            second.top() > last_added.bottom(),
            "{second:?} {last_added:?}"
        );

        // The header is the block's: its file, Copy of the block as
        // written, markers and all, and Apply.
        let header = chat.bounds("chat-code-0-0".into()).expect("the header");
        assert!(header.bottom() <= code.top());
        chat.click("chat-copy-0-0");
        let copied = chat.cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(copied.as_deref(), Some(SECTIONS));
        chat.click("chat-apply-0-0");
        assert_eq!(
            chat.take_events(),
            [ChatViewEvent::ApplyBlock { turn: 0, block: 0 }]
        );
    });
}

#[test]
fn whole_file_and_malformed_blocks_stay_code() {
    with_app("whole_file_and_malformed_blocks_stay_code", |cx, lines| {
        let markdown = "```toml path=deny.toml\n[bans]\nmultiple-versions = \"deny\"\n```\n\n\
                        ```toml path=deny.toml\n<<<<<<< SEARCH\n[bans]\n```\n";
        let provider = Arc::new(ScriptedProvider::new([finished(markdown)]));
        let mut chat = open(cx, provider, lines);
        for block in 0..2 {
            assert!(chat.drawn(&format!("md-code-{block}")), "block {block}");
            assert!(
                chat.drawn(&format!("md-code-{block}-text")),
                "block {block}"
            );
            assert!(
                !chat.drawn(&format!("chat-diff-0-{block}")),
                "block {block}"
            );
            assert!(chat.drawn(&format!("chat-code-0-{block}")), "block {block}");
        }
        assert!(chat.laid_out("multiple-versions = \"deny\""));
        // The malformed block as written.
        assert!(chat.laid_out("<<<<<<< SEARCH"));
    });
}

#[test]
fn the_block_still_streaming_stays_code() {
    with_app("the_block_still_streaming_stays_code", |cx, lines| {
        let (sender, receiver) = mpsc::unbounded();
        let provider = Arc::new(Channel(Mutex::new(Some(receiver))));
        let mut chat = open(cx, provider, lines);
        let send = |event| sender.unbounded_send(event).expect("the stream is open");
        send(ChatEvent::Started {
            model: "claude-opus-5-5".to_owned(),
            message_id: "msg".to_owned(),
        });
        send(ChatEvent::BlockStarted {
            index: 0,
            kind: BlockKind::Text,
        });
        let section = |name: &str| {
            format!("<<<<<<< SEARCH\n{name} = 1\n=======\n{name} = 2\n>>>>>>> REPLACE\n")
        };
        // The first block closed, the second one whole section in with its
        // fence still open.
        let streamed = format!(
            "First:\n\n```toml path=a.toml\n{}```\n\nThen:\n\n```toml path=b.toml\n{}",
            section("a"),
            section("b"),
        );
        send(ChatEvent::TextDelta {
            index: 0,
            text: streamed.clone(),
        });
        chat.settle();
        assert!(chat.drawn("chat-diff-0-0"), "the closed block");
        assert_eq!((chat.rows(0, "removed"), chat.rows(0, "added")), (1, 1));
        assert!(chat.drawn("md-code-1-text"), "the block still streaming");
        assert!(!chat.drawn("chat-diff-0-1"));
        assert!(chat.laid_out("b = 1"));
        assert!(chat.laid_out(">>>>>>> REPLACE"));

        let whole = format!("{streamed}```\n");
        send(ChatEvent::TextDelta {
            index: 0,
            text: "```\n".to_owned(),
        });
        send(ChatEvent::BlockFinished {
            index: 0,
            block: json!({"type": "text", "text": whole}),
        });
        send(ChatEvent::Finished {
            stop_reason: StopReason::EndTurn,
            stop_details: None,
        });
        chat.settle();
        assert_eq!(chat.take_events(), [ChatViewEvent::TurnFinished]);
        for block in 0..2 {
            assert!(chat.drawn(&format!("chat-diff-0-{block}")), "block {block}");
            assert!(
                !chat.drawn(&format!("md-code-{block}-text")),
                "block {block}"
            );
            let rows = (chat.rows(block, "removed"), chat.rows(block, "added"));
            assert_eq!(rows, (1, 1), "block {block}");
        }
    });
}
