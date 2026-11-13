//! Tool rows, plans and permission prompts in a chat's transcript (the AI
//! agents record, `docs/decisions/50t8wt9we2-ai-agents.md`, card AG9), in a
//! test window whose text system keeps each line it lays out: rows with
//! their status, updated in place; output folded until opened; locations as
//! links; compact diff previews; plan checklists; and the permission row's
//! Allow Once, Allow for This Chat and Deny, with Enter, Escape and Stop.

use std::{
    borrow::Cow,
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use fe_chat::{
    BlockKind, ChatEvent, ChatProvider, ChatSettings, Conversation, ErrorKind, ProviderError,
    ScriptedProvider, StopReason, permissions::Class,
};
use fe_chat_view::{
    ChatOptions, ChatView, ChatViewEvent, CommandOutcome, DiffPreview, OfferedOption, PREVIEW_ROWS,
    PermissionAnswer, PermissionChoices, PermissionPrompt, PlanEntry, PlanStatus, SendMessage,
    StopReply, ToolEvent, ToolKind, ToolLocation, ToolRow, ToolStatus, ToolTitle,
};
use gpui::{
    Bounds, DevicePixels, Entity, Focusable as _, Modifiers, NoopTextSystem, PlatformTextSystem,
    Size, TestAppContext, TestDispatcher, VisualTestContext, size,
};
use serde_json::json;

const MODEL: &str = "claude-opus-5-5";

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

/// A reply from `model` whose text is `text`, finished.
fn reply_from(model: &str, text: &str) -> Vec<ChatEvent> {
    vec![
        ChatEvent::Started {
            model: model.to_owned(),
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
    tool_events: Rc<RefCell<Vec<ToolEvent>>>,
    lines: Lines,
}

/// A focused chat over `provider`.
fn open_with(cx: &mut TestAppContext, provider: Arc<ScriptedProvider>, lines: Lines) -> Chat<'_> {
    let conversation = Conversation::new(&ChatSettings::default());
    let provider: Arc<dyn ChatProvider> = provider;
    let (view, cx) = cx.add_window_view(move |window, cx| {
        ChatView::new(provider, conversation, ChatOptions::default(), window, cx)
    });
    let events: Rc<RefCell<Vec<ChatViewEvent>>> = Rc::default();
    let log = Rc::clone(&events);
    let tool_events: Rc<RefCell<Vec<ToolEvent>>> = Rc::default();
    let tool_log = Rc::clone(&tool_events);
    cx.update(|window, cx| {
        cx.subscribe(&view, move |_, event: &ChatViewEvent, _| {
            log.borrow_mut().push(event.clone())
        })
        .detach();
        cx.subscribe(&view, move |_, event: &ToolEvent, _| {
            tool_log.borrow_mut().push(event.clone())
        })
        .detach();
        view.focus_handle(cx).focus(window, cx);
    });
    let mut chat = Chat {
        view,
        cx,
        events,
        tool_events,
        lines,
    };
    chat.settle();
    chat
}

/// A focused chat with one turn, answered and finished: the turn the
/// tests' rows go in.
fn open(cx: &mut TestAppContext, lines: Lines) -> Chat<'_> {
    let provider = Arc::new(ScriptedProvider::new([reply_from(MODEL, "All done.")]));
    let mut chat = open_with(cx, provider, lines);
    chat.send("Run the tests");
    chat.take_events();
    chat
}

impl Chat<'_> {
    fn settle(&mut self) {
        for _ in 0..3 {
            self.cx.run_until_parked();
            self.cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    }

    fn send(&mut self, text: &str) {
        self.type_text(text);
        self.cx.dispatch_action(SendMessage);
        self.settle();
    }

    fn type_text(&mut self, text: &str) {
        self.view
            .update(self.cx, |view, cx| view.set_composer_text(text, cx));
    }

    fn drawn(&mut self, selector: &str) -> bool {
        self.cx.debug_bounds(selector.to_owned().leak()).is_some()
    }

    fn click(&mut self, selector: &str) {
        let bounds = (self.cx.debug_bounds(selector.to_owned().leak()))
            .unwrap_or_else(|| panic!("no {selector} drawn"));
        self.cx.simulate_click(bounds.center(), Modifiers::none());
        self.settle();
    }

    /// Whether `line` was laid out since the lines were last forgotten.
    fn laid_out(&self, line: &str) -> bool {
        self.lines.lock().unwrap().iter().any(|laid| laid == line)
    }

    /// Forgets the lines laid out so far, and redraws everything: the
    /// window keeps the lines it last laid out, so a line drawn again isn't
    /// laid out again unless the view is drawn anew.
    fn forget_lines(&mut self) {
        self.lines.lock().unwrap().clear();
        self.cx.update(|window, _| window.refresh());
        self.settle();
    }

    fn take_events(&mut self) -> Vec<ChatViewEvent> {
        std::mem::take(&mut self.events.borrow_mut())
    }

    fn take_tool_events(&mut self) -> Vec<ToolEvent> {
        std::mem::take(&mut self.tool_events.borrow_mut())
    }

    fn update<R>(&mut self, f: impl FnOnce(&mut ChatView, &mut gpui::Context<ChatView>) -> R) -> R {
        let result = self.view.update(self.cx, f);
        self.settle();
        result
    }

    fn read<R>(&mut self, f: impl FnOnce(&ChatView) -> R) -> R {
        self.view.read_with(self.cx, |view, _| f(view))
    }

    fn set_row(&mut self, turn: usize, row: ToolRow) {
        self.update(|view, cx| view.set_tool_row(turn, row, cx));
    }

    fn ask(&mut self, turn: usize, tool: &str, prompt: PermissionPrompt) {
        self.update(|view, cx| view.ask_permission(turn, tool, prompt, cx));
    }

    /// How many rows of `kind` diff `d` of row `k` of turn 0 draws.
    fn diff_rows(&mut self, k: usize, d: usize, kind: &str) -> usize {
        (0..)
            .take_while(|r| self.drawn(&format!("chat-tool-diff-0-{k}-{d}-{kind}-{r}")))
            .count()
    }
}

fn run_row(status: ToolStatus, outcome: Option<CommandOutcome>) -> ToolRow {
    let title = ToolTitle::RunCommand {
        command: "cargo test".to_owned(),
        outcome,
    };
    ToolRow::new("toolu_1", ToolKind::Execute, title, status)
}

fn answered(turn: usize, tool: &str, answer: PermissionAnswer) -> ToolEvent {
    ToolEvent::PermissionAnswered {
        turn,
        tool: tool.to_owned(),
        answer,
    }
}

#[test]
fn titles_read_as_the_record_says() {
    with_app("titles", |_, _| {
        let title = |title: ToolTitle, status: ToolStatus| {
            ToolRow::new("t", ToolKind::Other, title, status).title_text()
        };
        let read = ToolTitle::ReadFile {
            path: "src/lib.rs".to_owned(),
            rows: Some((1, 200)),
        };
        assert_eq!(
            title(read, ToolStatus::Done),
            "Read `src/lib.rs` rows 1–200"
        );
        let whole = ToolTitle::ReadFile {
            path: "src/lib.rs".to_owned(),
            rows: None,
        };
        assert_eq!(title(whole, ToolStatus::Pending), "Read `src/lib.rs`");

        let search = |files| ToolTitle::Search {
            pattern: "foo".to_owned(),
            files,
        };
        assert_eq!(
            title(search(Some(12)), ToolStatus::Done),
            "Searched `foo` · 12 files"
        );
        assert_eq!(
            title(search(Some(1)), ToolStatus::Done),
            "Searched `foo` · 1 file"
        );
        assert_eq!(title(search(None), ToolStatus::Done), "Searched `foo`");
        assert_eq!(title(search(None), ToolStatus::Running), "Search `foo`");

        let list = |dir: Option<&str>| ToolTitle::ListFiles {
            dir: dir.map(str::to_owned),
        };
        assert_eq!(title(list(Some("src/")), ToolStatus::Done), "Listed `src/`");
        assert_eq!(title(list(Some("src/")), ToolStatus::Denied), "List `src/`");
        assert_eq!(
            title(list(None), ToolStatus::Done),
            "Listed the project folder"
        );

        let propose = |changes| ToolTitle::ProposeEdit {
            path: "src/x.rs".to_owned(),
            changes,
        };
        assert_eq!(
            title(propose(3), ToolStatus::Done),
            "Proposed 3 changes to `src/x.rs`"
        );
        assert_eq!(
            title(propose(1), ToolStatus::Done),
            "Proposed 1 change to `src/x.rs`"
        );
        assert_eq!(
            title(propose(3), ToolStatus::Pending),
            "Propose changes to `src/x.rs`"
        );

        let exited = CommandOutcome::Exited {
            code: 0,
            elapsed: Duration::from_millis(3_180),
        };
        assert_eq!(
            run_row(ToolStatus::Done, Some(exited)).title_text(),
            "Ran `cargo test` · exit 0 · 3.2 s"
        );
        let killed = CommandOutcome::Killed {
            elapsed: Duration::from_secs(120),
        };
        assert_eq!(
            run_row(ToolStatus::Failed, Some(killed)).title_text(),
            "Ran `cargo test` · killed after 120.0 s"
        );
        assert_eq!(
            run_row(ToolStatus::Running, None).title_text(),
            "Run `cargo test`"
        );
        assert_eq!(
            run_row(ToolStatus::Denied, Some(exited)).title_text(),
            "Run `cargo test`"
        );

        // Claude Agent's calls keep the adapter's title.
        let titled = ToolTitle::Titled("Edit src/main.rs".to_owned());
        assert_eq!(title(titled, ToolStatus::Done), "Edit src/main.rs");

        assert_eq!(ToolStatus::Pending.label(), "Waiting");
        assert_eq!(ToolStatus::Running.label(), "Running");
        assert_eq!(ToolStatus::Done.label(), "Done");
        assert_eq!(ToolStatus::Failed.label(), "Failed");
        assert_eq!(ToolStatus::Denied.label(), "Denied");
    });
}

#[test]
fn a_row_shows_its_status_and_is_updated_in_place() {
    with_app("status", |cx, lines| {
        let mut chat = open(cx, lines);
        assert!(!chat.drawn("chat-tools-0"));
        chat.forget_lines();
        chat.set_row(0, run_row(ToolStatus::Running, None));
        assert!(chat.drawn("chat-tools-0"));
        assert!(chat.drawn("chat-tool-0-0"));
        assert!(chat.drawn("chat-tool-status-0-0"));
        assert!(chat.laid_out("Run `cargo test`"));
        assert!(chat.laid_out("Running"));
        // The reply is still drawn, after the rows.
        assert!(chat.drawn("chat-reply-0"));
        let rows = chat.cx.debug_bounds("chat-tool-0-0").unwrap();
        let reply = chat.cx.debug_bounds("chat-reply-0").unwrap();
        assert!(rows.bottom() <= reply.top(), "{rows:?} above {reply:?}");

        // The same id again: in place, not a second row.
        let exited = CommandOutcome::Exited {
            code: 0,
            elapsed: Duration::from_millis(3_200),
        };
        chat.set_row(0, run_row(ToolStatus::Done, Some(exited)));
        assert_eq!(chat.read(|view| view.tool_rows(0).len()), 1);
        assert!(!chat.drawn("chat-tool-0-1"));
        assert!(chat.laid_out("Ran `cargo test` · exit 0 · 3.2 s"));
        assert!(chat.laid_out("Done"));

        // Another id: a second row, after the first.
        let read = ToolRow::new(
            "toolu_2",
            ToolKind::Read,
            ToolTitle::ReadFile {
                path: "src/lib.rs".to_owned(),
                rows: Some((1, 200)),
            },
            ToolStatus::Failed,
        );
        chat.set_row(0, read);
        assert!(chat.drawn("chat-tool-0-1"));
        assert!(chat.laid_out("Read `src/lib.rs` rows 1–200"));
        assert!(chat.laid_out("Failed"));
        let ids = chat.read(|view| {
            (view.tool_rows(0).iter())
                .map(|row| row.id.clone())
                .collect::<Vec<_>>()
        });
        assert_eq!(ids, ["toolu_1", "toolu_2"]);

        // Denied.
        chat.set_row(0, run_row(ToolStatus::Denied, None));
        assert!(chat.laid_out("Denied"));
        assert!(chat.take_events().is_empty());
        assert!(chat.take_tool_events().is_empty());
    });
}

#[test]
fn output_is_folded_until_opened() {
    with_app("output", |cx, lines| {
        let mut chat = open(cx, lines);
        let mut row = run_row(ToolStatus::Done, None);
        row.output = Some("test result: ok. 12 passed".to_owned());
        chat.forget_lines();
        chat.set_row(0, row.clone());
        assert!(chat.drawn("chat-tool-output-toggle-0-0"));
        assert!(!chat.drawn("chat-tool-output-0-0"));
        assert!(chat.laid_out("▸ Output"));
        assert!(!chat.laid_out("test result: ok. 12 passed"));

        chat.click("chat-tool-output-toggle-0-0");
        assert!(chat.read(|view| view.tool_output_open(0, "toolu_1")));
        assert!(chat.drawn("chat-tool-output-0-0"));
        assert!(chat.laid_out("▾ Output"));
        assert!(chat.laid_out("test result: ok. 12 passed"));

        // An update with more output keeps it open.
        row.output = Some("test result: ok. 13 passed".to_owned());
        chat.set_row(0, row);
        assert!(chat.drawn("chat-tool-output-0-0"));
        assert!(chat.laid_out("test result: ok. 13 passed"));

        chat.click("chat-tool-output-toggle-0-0");
        assert!(!chat.read(|view| view.tool_output_open(0, "toolu_1")));
        assert!(!chat.drawn("chat-tool-output-0-0"));

        // No output, no toggle.
        chat.set_row(0, run_row(ToolStatus::Running, None));
        assert!(!chat.drawn("chat-tool-output-toggle-0-0"));
    });
}

#[test]
fn locations_are_links_to_files() {
    with_app("locations", |cx, lines| {
        let mut chat = open(cx, lines);
        let mut row = ToolRow::new(
            "call_1",
            ToolKind::Edit,
            ToolTitle::Titled("Edit src/lib.rs".to_owned()),
            ToolStatus::Done,
        );
        row.locations = vec![
            ToolLocation {
                path: "src/lib.rs".to_owned(),
                row: Some(12),
            },
            ToolLocation {
                path: "README.md".to_owned(),
                row: None,
            },
        ];
        chat.forget_lines();
        chat.set_row(0, row);
        assert!(chat.laid_out("Edit src/lib.rs"));
        assert!(chat.laid_out("src/lib.rs:12"));
        assert!(chat.laid_out("README.md"));
        chat.click("chat-tool-location-0-0-0");
        chat.click("chat-tool-location-0-0-1");
        assert_eq!(
            chat.take_events(),
            [
                ChatViewEvent::OpenPath {
                    path: "src/lib.rs".to_owned(),
                    row: Some(12)
                },
                ChatViewEvent::OpenPath {
                    path: "README.md".to_owned(),
                    row: None
                },
            ]
        );
    });
}

#[test]
fn diffs_preview_compactly_under_their_row() {
    with_app("diffs", |cx, lines| {
        let mut chat = open(cx, lines);
        let mut row = ToolRow::new(
            "call_1",
            ToolKind::Edit,
            ToolTitle::Titled("Edit files".to_owned()),
            ToolStatus::Done,
        );
        let many: String = (0..PREVIEW_ROWS + 10)
            .map(|n| format!("row {n}\n"))
            .collect();
        row.diffs = vec![
            // Two hunks: one row changed, one added at the end.
            DiffPreview {
                path: "src/a.rs".to_owned(),
                old_text: Some("one\ntwo\nthree\nfour\nfive\n".to_owned()),
                new_text: "one\nTWO\nthree\nfour\nfive\nsix\n".to_owned(),
            },
            // A new file: all added.
            DiffPreview {
                path: "src/new.rs".to_owned(),
                old_text: None,
                new_text: "fn main() {}\n// done\n".to_owned(),
            },
            // Too many rows: cut, with how many more.
            DiffPreview {
                path: "big.txt".to_owned(),
                old_text: None,
                new_text: many,
            },
        ];
        chat.forget_lines();
        chat.set_row(0, row);
        assert!(chat.drawn("chat-tool-diff-0-0-0"));
        assert_eq!(chat.diff_rows(0, 0, "removed"), 1);
        assert_eq!(chat.diff_rows(0, 0, "added"), 2);
        assert!(chat.laid_out("src/a.rs"));
        assert!(chat.laid_out("two"));
        assert!(chat.laid_out("TWO"));
        assert!(chat.laid_out("six"));
        // Compact: the rows around the changes aren't shown.
        assert!(!chat.laid_out("three"));
        let removed = chat
            .cx
            .debug_bounds("chat-tool-diff-0-0-0-removed-0")
            .unwrap();
        let added = chat
            .cx
            .debug_bounds("chat-tool-diff-0-0-0-added-0")
            .unwrap();
        assert!(removed.top() < added.top());

        assert_eq!(chat.diff_rows(0, 1, "removed"), 0);
        assert_eq!(chat.diff_rows(0, 1, "added"), 2);
        assert!(chat.laid_out("fn main() {}"));
        assert!(!chat.drawn("chat-tool-diff-more-0-0-1"));

        assert_eq!(chat.diff_rows(0, 2, "added"), PREVIEW_ROWS);
        assert!(chat.drawn("chat-tool-diff-more-0-0-2"));
        assert!(chat.laid_out("… 10 more rows"));
    });
}

#[test]
fn a_plan_is_a_checklist_updated_in_place() {
    with_app("plan", |cx, lines| {
        let mut chat = open(cx, lines);
        let entry = |text: &str, status| PlanEntry {
            text: text.to_owned(),
            status,
        };
        chat.forget_lines();
        chat.update(|view, cx| {
            view.set_plan(
                0,
                vec![
                    entry("Read the failing test", PlanStatus::InProgress),
                    entry("Fix the parser", PlanStatus::Pending),
                ],
                cx,
            )
        });
        assert!(chat.drawn("chat-plan-0"));
        assert!(chat.drawn("chat-plan-0-0"));
        assert!(chat.drawn("chat-plan-0-1"));
        assert!(!chat.drawn("chat-plan-0-2"));
        assert!(chat.laid_out("Plan"));
        assert!(chat.laid_out("Read the failing test"));
        assert!(chat.laid_out("Fix the parser"));

        // Updated: one plan, its steps replaced.
        let updated = vec![
            entry("Read the failing test", PlanStatus::Completed),
            entry("Fix the parser", PlanStatus::InProgress),
            entry("Run the tests", PlanStatus::Pending),
        ];
        chat.update(|view, cx| view.set_plan(0, updated.clone(), cx));
        assert_eq!(chat.read(|view| view.plan(0).to_vec()), updated);
        assert!(chat.drawn("chat-plan-0-2"));
        assert!(chat.laid_out("Run the tests"));
        assert_eq!(PlanStatus::Completed.label(), "Done");
        assert_eq!(PlanStatus::InProgress.label(), "In progress");
        assert_eq!(PlanStatus::Pending.label(), "Not started");

        // Above the rows.
        chat.set_row(0, run_row(ToolStatus::Running, None));
        let plan = chat.cx.debug_bounds("chat-plan-0").unwrap();
        let row = chat.cx.debug_bounds("chat-tool-0-0").unwrap();
        assert!(plan.bottom() <= row.top());

        chat.update(|view, cx| view.set_plan(0, Vec::new(), cx));
        assert!(!chat.drawn("chat-plan-0"));
        assert!(chat.read(|view| view.plan(0).is_empty()));
    });
}

#[test]
fn the_permission_row_allows_once_for_the_chat_or_denies() {
    with_app("permission", |cx, lines| {
        let mut chat = open(cx, lines);
        chat.set_row(0, run_row(ToolStatus::Pending, None));
        let prompt = PermissionPrompt {
            cwd: Some("crates/fe-chat".to_owned()),
            in_project: true,
            ..PermissionPrompt::new(Class::RunCommand, "/usr/bin/cargo test")
        };
        chat.forget_lines();
        chat.ask(0, "toolu_1", prompt.clone());
        assert!(chat.drawn("chat-permission-0-0"));
        assert!(chat.laid_out("Allow running `/usr/bin/cargo test`?"));
        assert!(chat.laid_out("In `crates/fe-chat`"));
        assert!(chat.laid_out("This runs in your project folder and can change files."));
        assert!(chat.laid_out("Allow Once"));
        assert!(chat.laid_out("Allow for This Chat"));
        assert!(chat.laid_out("Deny"));
        // Under the row it's about.
        let row = chat.cx.debug_bounds("chat-tool-0-0").unwrap();
        let asked = chat.cx.debug_bounds("chat-permission-0-0").unwrap();
        assert!(row.bottom() <= asked.top());
        assert_eq!(
            chat.read(|view| view.waiting_permissions()),
            [(0, "toolu_1".to_owned())]
        );

        chat.click("chat-permission-allow-once-0-0");
        assert_eq!(
            chat.take_tool_events(),
            [answered(0, "toolu_1", PermissionAnswer::AllowOnce)]
        );
        assert!(!chat.drawn("chat-permission-0-0"));
        assert!(chat.read(|view| view.permission(0, "toolu_1").is_none()));

        chat.ask(0, "toolu_1", prompt.clone());
        chat.click("chat-permission-allow-chat-0-0");
        assert_eq!(
            chat.take_tool_events(),
            [answered(0, "toolu_1", PermissionAnswer::AllowForChat)]
        );

        chat.ask(0, "toolu_1", prompt.clone());
        chat.click("chat-permission-deny-0-0");
        assert_eq!(
            chat.take_tool_events(),
            [answered(0, "toolu_1", PermissionAnswer::Deny)]
        );
        assert!(chat.read(|view| view.waiting_permissions().is_empty()));

        // Each class asks its own way.
        let question = |class| PermissionPrompt::new(class, "src/lib.rs").question();
        assert_eq!(question(Class::Read), "Allow reading `src/lib.rs`?");
        assert_eq!(
            question(Class::ProposeEdit),
            "Allow proposing changes to `src/lib.rs`?"
        );
        assert_eq!(question(Class::Search), "Allow searching for `src/lib.rs`?");
        assert_eq!(question(Class::Fetch), "Allow fetching `src/lib.rs`?");
        assert_eq!(question(Class::Other), "Allow src/lib.rs?");

        // Withdrawn by the host: gone, nothing emitted.
        chat.ask(0, "toolu_1", prompt);
        chat.update(|view, cx| view.withdraw_permission(0, "toolu_1", cx));
        assert!(!chat.drawn("chat-permission-0-0"));
        assert!(chat.take_events().is_empty());
        assert!(chat.take_tool_events().is_empty());
    });
}

#[test]
fn a_once_prompt_and_offered_options() {
    with_app("choices", |cx, lines| {
        let mut chat = open(cx, lines);
        chat.set_row(0, run_row(ToolStatus::Pending, None));
        let once = PermissionPrompt {
            choices: PermissionChoices::Once,
            ..PermissionPrompt::new(Class::Other, "network access to example.com")
        };
        chat.ask(0, "toolu_1", once);
        assert!(chat.drawn("chat-permission-allow-once-0-0"));
        assert!(!chat.drawn("chat-permission-allow-chat-0-0"));
        assert!(chat.drawn("chat-permission-deny-0-0"));
        // Not offered, so not taken.
        let taken = chat.update(|view, cx| {
            view.answer_permission(0, "toolu_1", PermissionAnswer::AllowForChat, cx)
        });
        assert!(!taken);
        assert!(chat.take_events().is_empty());
        assert!(chat.take_tool_events().is_empty());
        chat.click("chat-permission-deny-0-0");
        assert_eq!(
            chat.take_tool_events(),
            [answered(0, "toolu_1", PermissionAnswer::Deny)]
        );

        // An ACP request with no allow_once: its options as offered.
        let offered = PermissionPrompt {
            choices: PermissionChoices::Offered(vec![
                OfferedOption {
                    id: "plan".to_owned(),
                    label: "Keep planning".to_owned(),
                },
                OfferedOption {
                    id: "accept".to_owned(),
                    label: "Yes, and accept edits".to_owned(),
                },
            ]),
            ..PermissionPrompt::new(Class::Other, "ExitPlanMode")
        };
        chat.forget_lines();
        chat.ask(0, "toolu_1", offered);
        assert!(chat.laid_out("Keep planning"));
        assert!(chat.laid_out("Yes, and accept edits"));
        assert!(!chat.drawn("chat-permission-allow-once-0-0"));
        assert!(!chat.drawn("chat-permission-deny-0-0"));
        let taken = chat.update(|view, cx| {
            view.answer_permission(0, "toolu_1", PermissionAnswer::AllowOnce, cx)
        });
        assert!(!taken);
        // Enter doesn't pick one.
        chat.cx.dispatch_action(SendMessage);
        chat.settle();
        assert!(chat.take_events().is_empty());
        assert!(chat.take_tool_events().is_empty());
        chat.click("chat-permission-option-0-0-1");
        assert_eq!(
            chat.take_tool_events(),
            [answered(
                0,
                "toolu_1",
                PermissionAnswer::Option("accept".to_owned())
            )]
        );
    });
}

#[test]
fn enter_allows_once_and_escape_denies() {
    with_app("keys", |cx, lines| {
        let provider = Arc::new(ScriptedProvider::new([
            reply_from(MODEL, "All done."),
            reply_from(MODEL, "Again."),
        ]));
        let mut chat = open_with(cx, provider.clone(), lines);
        chat.send("Run the tests");
        chat.take_events();
        chat.set_row(0, run_row(ToolStatus::Pending, None));
        let prompt = PermissionPrompt::new(Class::RunCommand, "cargo test");
        chat.ask(0, "toolu_1", prompt.clone());

        // The composer's Enter is chat.send, and its Escape chat.stop
        // (fastedit.toml's chat bindings).
        chat.cx.dispatch_action(SendMessage);
        chat.settle();
        assert_eq!(
            chat.take_tool_events(),
            [answered(0, "toolu_1", PermissionAnswer::AllowOnce)]
        );

        chat.ask(0, "toolu_1", prompt.clone());
        chat.cx.dispatch_action(StopReply);
        chat.settle();
        assert_eq!(
            chat.take_tool_events(),
            [answered(0, "toolu_1", PermissionAnswer::Deny)]
        );

        // With text in the composer, Enter neither allows nor sends while a
        // prompt waits: the text stays.
        chat.ask(0, "toolu_1", prompt.clone());
        chat.send("And then?");
        assert!(chat.take_events().is_empty());
        assert!(chat.take_tool_events().is_empty());
        assert_eq!(provider.requests().len(), 1);
        assert_eq!(chat.read(|view| view.conversation().turns.len()), 1);
        let text = chat
            .view
            .read_with(chat.cx, |view, cx| view.composer_text(cx));
        assert_eq!(text, "And then?");
        assert!(chat.read(|view| view.permission(0, "toolu_1").is_some()));

        // Two waiting: the first asked is the one answered.
        chat.set_row(
            0,
            ToolRow::new(
                "toolu_2",
                ToolKind::Read,
                ToolTitle::ReadFile {
                    path: "src/lib.rs".to_owned(),
                    rows: None,
                },
                ToolStatus::Pending,
            ),
        );
        chat.ask(
            0,
            "toolu_2",
            PermissionPrompt::new(Class::Read, "src/lib.rs"),
        );
        chat.type_text("");
        chat.cx.dispatch_action(SendMessage);
        chat.settle();
        assert_eq!(
            chat.take_tool_events(),
            [answered(0, "toolu_1", PermissionAnswer::AllowOnce)]
        );
        assert_eq!(
            chat.read(|view| view.waiting_permissions()),
            [(0, "toolu_2".to_owned())]
        );
        chat.cx.dispatch_action(StopReply);
        chat.settle();
        assert_eq!(
            chat.take_tool_events(),
            [answered(0, "toolu_2", PermissionAnswer::Deny)]
        );

        // Nothing waits: Enter sends again.
        chat.send("And then?");
        assert_eq!(provider.requests().len(), 2);
    });
}

#[test]
fn stop_closes_waiting_prompts_as_refused() {
    with_app("stop", |cx, lines| {
        let mut chat = open(cx, lines);
        assert!(!chat.drawn("chat-stop"));
        chat.set_row(0, run_row(ToolStatus::Pending, None));
        chat.ask(
            0,
            "toolu_1",
            PermissionPrompt::new(Class::RunCommand, "cargo test"),
        );
        // A prompt keeps the turn waiting, so Stop shows.
        assert!(chat.drawn("chat-stop"));
        // A prompt with no row yet still shows, after the turn's rows.
        chat.ask(
            0,
            "toolu_9",
            PermissionPrompt::new(Class::Fetch, "https://example.com"),
        );
        assert!(chat.drawn("chat-permission-0-1"));
        let row = chat.cx.debug_bounds("chat-tool-0-0").unwrap();
        let orphan = chat.cx.debug_bounds("chat-permission-0-1").unwrap();
        assert!(row.bottom() <= orphan.top());

        chat.click("chat-stop");
        assert_eq!(
            chat.take_tool_events(),
            [
                answered(0, "toolu_1", PermissionAnswer::Stopped),
                answered(0, "toolu_9", PermissionAnswer::Stopped),
            ]
        );
        assert!(!chat.drawn("chat-permission-0-0"));
        assert!(!chat.drawn("chat-permission-0-1"));
        assert!(!chat.drawn("chat-stop"));
    });
}

#[test]
fn a_proposal_row_offers_review() {
    with_app("review", |cx, lines| {
        let mut chat = open(cx, lines);
        let mut row = ToolRow::new(
            "toolu_3",
            ToolKind::Edit,
            ToolTitle::ProposeEdit {
                path: "src/x.rs".to_owned(),
                changes: 3,
            },
            ToolStatus::Done,
        );
        chat.set_row(0, row.clone());
        assert!(!chat.drawn("chat-tool-review-0-0"));
        row.review = true;
        chat.forget_lines();
        chat.set_row(0, row);
        assert!(chat.laid_out("Proposed 3 changes to `src/x.rs`"));
        assert!(chat.laid_out("Review"));
        chat.click("chat-tool-review-0-0");
        assert_eq!(
            chat.take_tool_events(),
            [ToolEvent::ReviewProposal {
                turn: 0,
                tool: "toolu_3".to_owned()
            }]
        );
    });
}

/// T604's carried finding: a turn's fallback attribution and a NotFound
/// banner name the model that turn asked for, not the one picked since.
#[test]
fn earlier_turns_keep_the_model_they_asked_for() {
    with_app("asked", |cx, lines| {
        let provider = Arc::new(ScriptedProvider::new([
            reply_from("claude-sonnet-5-5", "Fell back."),
            vec![ChatEvent::Failed(ProviderError::new(
                ErrorKind::NotFound,
                "model not found",
            ))],
        ]));
        let mut chat = open_with(cx, provider, lines);
        chat.send("First");
        chat.send("Second");
        let shown = |chat: &mut Chat<'_>, turn| chat.read(|view| view.turn_display(turn).unwrap());
        assert_eq!(
            shown(&mut chat, 0).answered_by.as_deref(),
            Some("claude-sonnet-5-5")
        );
        let not_found = format!("Model `{MODEL}` isn't available to this key");
        assert_eq!(shown(&mut chat, 1).banner.unwrap().text, not_found);

        // The picker changes the next turn's model only.
        chat.update(|view, cx| view.set_model("claude-sonnet-5-5".to_owned(), cx));
        assert_eq!(
            shown(&mut chat, 0).answered_by.as_deref(),
            Some("claude-sonnet-5-5")
        );
        assert_eq!(shown(&mut chat, 1).banner.unwrap().text, not_found);
    });
}
