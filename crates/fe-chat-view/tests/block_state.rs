//! What came of applying a reply's code block, on its header, in a test
//! window with a text system that keeps each line it lays out: each
//! state's label, and the reason a block wasn't applied in a row under it.

use std::{
    borrow::Cow,
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
};

use fe_chat::{BlockKind, ChatEvent, ChatSettings, Conversation, ScriptedProvider, StopReason};
use fe_chat_view::{BlockState, ChatOptions, ChatView, ChatViewEvent, SendMessage};
use gpui::{
    Bounds, DevicePixels, Entity, Focusable as _, NoopTextSystem, PlatformTextSystem, Size,
    TestAppContext, TestDispatcher, VisualTestContext, size,
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

struct Chat<'a> {
    view: Entity<ChatView>,
    cx: &'a mut VisualTestContext,
    events: Rc<RefCell<Vec<ChatViewEvent>>>,
    lines: Lines,
}

/// A focused chat over `provider`, a question sent.
fn open(cx: &mut TestAppContext, provider: Arc<ScriptedProvider>, lines: Lines) -> Chat<'_> {
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

    fn laid_out(&self, line: &str) -> bool {
        self.lines.lock().unwrap().iter().any(|laid| laid == line)
    }

    /// Sets block 0's state, the lines laid out so far forgotten first: a
    /// window can draw as soon as it's notified.
    fn set(&mut self, state: Option<BlockState>) {
        self.lines.lock().unwrap().clear();
        self.view
            .update(self.cx, |view, cx| view.set_block_state(0, 0, state, cx));
        self.settle();
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

#[test]
fn block_states_label_the_header() {
    with_app("block_states_label_the_header", |cx, lines| {
        let markdown = "Rename it:\n\n```go path=src/lib.go\nfunc Renamed() {}\n```\n";
        let provider = Arc::new(ScriptedProvider::new([finished(markdown)]));
        let mut chat = open(cx, provider, lines);
        assert_eq!(chat.take_events(), [ChatViewEvent::TurnFinished]);
        assert!(chat.drawn("chat-code-0-0"));
        assert!(!chat.drawn("chat-block-state-0-0"), "no state yet");

        let labels = [
            (
                BlockState::Proposed {
                    pending: 3,
                    hunks: 3,
                },
                "Proposed",
            ),
            (
                BlockState::Proposed {
                    pending: 2,
                    hunks: 3,
                },
                "Proposed · 2 left",
            ),
            (
                BlockState::Decided {
                    accepted: 3,
                    hunks: 3,
                },
                "Accepted",
            ),
            (
                BlockState::Decided {
                    accepted: 0,
                    hunks: 3,
                },
                "Rejected",
            ),
            (
                BlockState::Decided {
                    accepted: 1,
                    hunks: 3,
                },
                "Accepted 1 of 3",
            ),
            (BlockState::NoChange, "No changes"),
        ];
        for (state, label) in labels {
            assert_eq!(state.label(), label);
            chat.set(Some(state.clone()));
            assert_eq!(
                chat.view
                    .read_with(chat.cx, |view, _| view.block_state(0, 0).cloned()),
                Some(state.clone())
            );
            // Between the path and Copy, on the header.
            let header = chat.bounds("chat-code-0-0".into()).expect("the header");
            let shown = chat
                .bounds("chat-block-state-0-0".into())
                .expect("the label");
            let copy = chat.bounds("chat-copy-0-0".into()).expect("Copy");
            assert!(header.contains(&shown.center()), "{state:?}");
            assert!(shown.right() <= copy.left(), "{state:?}");
            assert!(chat.laid_out(label), "{label:?} laid out");
            assert!(!chat.drawn("chat-block-error-0-0"), "{state:?}");
            // Apply stays on.
            assert!(chat.drawn("chat-apply-0-0"));
        }

        let why = "SEARCH 1 (\"func Old() {}\") isn't in it";
        chat.set(Some(BlockState::Failed(why.to_owned())));
        assert!(chat.laid_out("Not applied"));
        let header = chat.bounds("chat-code-0-0".into()).expect("the header");
        let error = chat
            .bounds("chat-block-error-0-0".into())
            .expect("the reason");
        assert!(header.bottom() <= error.top(), "{header:?} over {error:?}");
        assert!(chat.laid_out(why), "the reason laid out");

        chat.set(None);
        assert!(!chat.drawn("chat-block-state-0-0"));
        assert!(!chat.drawn("chat-block-error-0-0"));
        assert_eq!(
            chat.view
                .read_with(chat.cx, |view, _| view.block_state(0, 0).cloned()),
            None
        );
    });
}
