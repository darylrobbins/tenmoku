//! Quick suggestions (docs/decisions/mgacx379x8-completion-as-you-type.md):
//! typing a word asks the provider, as invoked, once
//! `editor.completion.delay_ms` passes with nothing typed since; not in
//! comments or strings, outside vim's insert mode, with more than one
//! caret, in a snippet's tabstops, or with `auto_trigger` off, where
//! `ctrl-space` and trigger characters still ask. A menu that opened by
//! itself lets `enter` type its new line, as `accept_on_enter` says, and
//! `tab` accepts.

use std::{cell::RefCell, rc::Rc, time::Duration};

use fe_buffer::{Buffer, BufferId, ResolvedSelection, Version};
use fe_config::settings::{AcceptOnEnter, CompletionSettings, CompletionWords};
use fe_editor::{
    EditorModel,
    completion::{CompletionItem, CompletionList, CompletionTrigger},
};
use fe_gpui::{
    CompletionFuture, CompletionProvider, EditorSettings, EditorTheme, EditorView, ResolveFuture,
    init, set_vim_enabled,
};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

/// Offers `alpha` and `alter` over the two chars before the caret, and
/// records each ask.
#[derive(Default)]
struct Provider {
    asked: RefCell<Vec<(usize, CompletionTrigger)>>,
}

impl CompletionProvider for Provider {
    fn trigger_characters(&self, _: BufferId) -> Vec<String> {
        vec![".".into()]
    }

    fn completions(
        &self,
        _: BufferId,
        _: Version,
        offset: usize,
        trigger: CompletionTrigger,
    ) -> CompletionFuture {
        self.asked.borrow_mut().push((offset, trigger));
        let start = offset.saturating_sub(2);
        let items = ["alpha", "alter"]
            .into_iter()
            .map(|label| CompletionItem {
                label: label.into(),
                edit: (start..offset, label.into()),
                ..CompletionItem::default()
            })
            .collect();
        Box::pin(async move {
            Some(CompletionList {
                items,
                is_incomplete: false,
            })
        })
    }

    fn resolve(&self, item: CompletionItem) -> ResolveFuture {
        Box::pin(async move { item })
    }
}

const DELAY: Duration = Duration::from_millis(50);

/// The settings under test: words off, so the provider's items are all
/// there is.
fn settings() -> CompletionSettings {
    CompletionSettings {
        words: CompletionWords::Off,
        ..CompletionSettings::default()
    }
}

struct Editor<'a> {
    view: Entity<EditorView>,
    provider: Rc<Provider>,
    cx: &'a mut VisualTestContext,
}

/// An editor on `text`, a file at `path`, the caret at the first `|` (taken
/// out), focused, with the provider and `settings`.
fn editor<'a>(
    text: &str,
    path: Option<&str>,
    settings: CompletionSettings,
    cx: &'a mut TestAppContext,
) -> Editor<'a> {
    cx.update(init);
    let at = text.find('|').expect("a caret");
    let text = text.replacen('|', "", 1);
    let provider = Rc::new(Provider::default());
    let given = provider.clone();
    let path = path.map(Into::into);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), path);
        model.set_selections(vec![ResolvedSelection {
            range: at..at,
            reversed: false,
        }]);
        let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
        view.set_completion_provider(Some(given));
        view.set_completion_settings(&settings);
        view
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    draw(cx);
    Editor { view, provider, cx }
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

impl Editor<'_> {
    fn wait(&mut self, time: Duration) {
        self.cx.executor().advance_clock(time);
        self.cx.run_until_parked();
        draw(self.cx);
    }

    fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        draw(self.cx);
    }

    fn asked(&self) -> Vec<(usize, CompletionTrigger)> {
        self.provider.asked.borrow().clone()
    }

    fn shown(&mut self) -> bool {
        self.view
            .read_with(self.cx, |view, _| view.completion_shown())
    }

    fn text(&mut self) -> String {
        (self.view).read_with(self.cx, |view, _| view.model().text().to_string())
    }

    fn caret(&mut self) -> usize {
        (self.view).read_with(self.cx, |view, _| view.model().primary_selection().head())
    }
}

#[gpui::test]
fn typing_a_word_asks_as_invoked_once_the_delay_passes(cx: &mut TestAppContext) {
    let mut e = editor("let x = |;\n", None, settings(), cx);
    e.keys("a l");
    assert_eq!(e.asked(), []);
    assert!(e.view.read_with(e.cx, |view, _| view.completion_pending()));
    e.wait(DELAY - Duration::from_millis(1));
    assert_eq!(e.asked(), []);
    e.wait(Duration::from_millis(1));
    assert_eq!(e.asked(), [(10, CompletionTrigger::Invoked)]);
    assert!(e.shown());
    assert!(
        e.view
            .read_with(e.cx, |view, _| view.completion_opened_itself())
    );
    let labels: Vec<String> = e
        .view
        .read_with(e.cx, |view, _| view.completion_matches())
        .into_iter()
        .map(|item| item.label)
        .collect();
    assert_eq!(labels, ["alpha", "alter"]);

    // Typing on narrows the menu, asking nothing more.
    e.keys("p");
    e.wait(DELAY * 2);
    assert_eq!(e.asked().len(), 1);
    assert_eq!(
        e.view
            .read_with(e.cx, |view, _| view.completion_matches().len()),
        1
    );
}

#[gpui::test]
fn a_key_within_the_delay_drops_the_ask(cx: &mut TestAppContext) {
    let mut e = editor("let x = |;\n", None, settings(), cx);
    // A word char typed sooner waits again, from there.
    e.keys("a");
    e.wait(DELAY / 2);
    e.keys("l");
    e.wait(DELAY / 2);
    assert_eq!(e.asked(), []);
    e.wait(DELAY / 2);
    assert_eq!(e.asked(), [(10, CompletionTrigger::Invoked)]);
    e.keys("escape");
    assert!(!e.shown());

    // A space, a caret moved, or escape, sooner: nothing is asked.
    for after in ["space", "left", "escape"] {
        e.keys("b");
        e.wait(DELAY / 2);
        e.keys(after);
        e.wait(DELAY * 4);
        assert_eq!(e.asked().len(), 1, "after {after}");
        assert!(!e.shown(), "after {after}");
    }
}

#[gpui::test]
fn the_delay_and_min_word_chars_are_the_settings(cx: &mut TestAppContext) {
    let wanted = CompletionSettings {
        delay_ms: 200,
        min_word_chars: 3,
        ..settings()
    };
    let mut e = editor("let x = |;\n", None, wanted, cx);
    e.keys("a l");
    e.wait(Duration::from_millis(400));
    assert_eq!(e.asked(), [], "two chars of three");
    e.keys("p");
    e.wait(Duration::from_millis(199));
    assert_eq!(e.asked(), []);
    e.wait(Duration::from_millis(1));
    assert_eq!(e.asked(), [(11, CompletionTrigger::Invoked)]);
}

#[gpui::test]
fn no_quick_suggestion_in_comments_or_strings(cx: &mut TestAppContext) {
    let text = "fn main() {\n    // note |\n    let s = \"\";\n    \n}\n";
    let mut e = editor(text, Some("main.rs"), settings(), cx);
    assert!(e.view.read_with(e.cx, |view, _| view.syntax().is_some()));
    let in_string = e.text().find("\"\"").unwrap() + 1;
    let in_code = e.text().find("\n    \n").unwrap() + 5;

    e.keys("a l");
    e.wait(DELAY * 2);
    assert_eq!(e.asked(), [], "in a comment");

    let at = |e: &mut Editor, offset: usize| {
        e.view.update(e.cx, |view, cx| {
            view.model_mut().set_cursor(offset);
            cx.notify();
        });
        draw(e.cx);
    };
    at(&mut e, in_string + 2);
    e.keys("a l");
    e.wait(DELAY * 2);
    assert_eq!(e.asked(), [], "in a string");

    // In code, it asks.
    let in_code = in_code + 4;
    at(&mut e, in_code);
    e.keys("a l");
    e.wait(DELAY * 2);
    assert_eq!(e.asked(), [(in_code + 2, CompletionTrigger::Invoked)]);
    e.keys("escape");

    // `ctrl-space` asks in a comment all the same.
    let comment = e.text().find("note ").unwrap() + "note al".len();
    at(&mut e, comment);
    e.keys("ctrl-space");
    e.wait(DELAY);
    assert_eq!(e.asked().len(), 2);
    assert!(e.shown());
}

#[gpui::test]
fn in_comments_and_in_strings_turn_them_on(cx: &mut TestAppContext) {
    let wanted = CompletionSettings {
        in_comments: true,
        in_strings: true,
        ..settings()
    };
    let text = "fn main() {\n    // note |\n    let s = \"\";\n}\n";
    let mut e = editor(text, Some("main.rs"), wanted, cx);
    e.keys("a l");
    e.wait(DELAY * 2);
    assert_eq!(e.asked().len(), 1, "in a comment");
    e.keys("escape");
    let in_string = e.text().find("\"\"").unwrap() + 1;
    e.view.update(e.cx, |view, cx| {
        view.model_mut().set_cursor(in_string);
        cx.notify();
    });
    draw(e.cx);
    e.keys("a l");
    e.wait(DELAY * 2);
    assert_eq!(e.asked().len(), 2, "in a string");
}

#[gpui::test]
fn auto_trigger_off_leaves_ctrl_space_and_trigger_characters(cx: &mut TestAppContext) {
    let wanted = CompletionSettings {
        auto_trigger: false,
        ..settings()
    };
    let mut e = editor("let x = |;\n", None, wanted, cx);
    e.keys("a l");
    e.wait(DELAY * 4);
    assert_eq!(e.asked(), []);
    assert!(!e.shown());

    e.keys("ctrl-space");
    e.wait(DELAY);
    assert_eq!(e.asked(), [(10, CompletionTrigger::Invoked)]);
    assert!(e.shown());
    assert!(
        !e.view
            .read_with(e.cx, |view, _| view.completion_opened_itself())
    );
    e.keys("escape");

    e.keys(".");
    e.wait(DELAY);
    assert_eq!(
        e.asked()[1..],
        [(11, CompletionTrigger::Character(".".into()))]
    );
}

#[gpui::test]
fn settings_applied_live_turn_it_off_and_on(cx: &mut TestAppContext) {
    let mut e = editor("let x = |;\n", None, settings(), cx);
    let apply = |e: &mut Editor, auto_trigger: bool| {
        let settings = EditorSettings {
            completion: CompletionSettings {
                auto_trigger,
                ..settings()
            },
            ..EditorSettings::default()
        };
        e.view
            .update(e.cx, |view, cx| view.apply_settings(&settings, cx));
    };
    // Turned off while an ask waits: it's dropped.
    e.keys("a");
    apply(&mut e, false);
    assert!(!e.view.read_with(e.cx, |view, _| view.completion_pending()));
    e.wait(DELAY * 2);
    e.keys("l");
    e.wait(DELAY * 2);
    assert_eq!(e.asked(), []);
    apply(&mut e, true);
    e.keys("p");
    e.wait(DELAY);
    assert_eq!(e.asked(), [(11, CompletionTrigger::Invoked)]);
}

#[gpui::test]
fn vim_asks_only_in_insert_mode(cx: &mut TestAppContext) {
    let mut e = editor("let x = |;\n", None, settings(), cx);
    e.cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(e.cx);

    // Normal mode's keys are commands, which ask nothing.
    e.keys("w b e");
    e.wait(DELAY * 4);
    assert_eq!(e.asked(), []);

    // Typed in insert mode, then back to normal or on to visual mode
    // within the delay: nothing.
    e.keys("i a l escape");
    e.wait(DELAY * 4);
    assert_eq!(e.asked(), []);
    e.keys("a x y escape v");
    e.wait(DELAY * 4);
    assert_eq!(e.asked(), []);
    assert!(!e.shown());
    e.keys("escape");

    // In insert mode, it asks.
    e.keys("a space a l");
    e.wait(DELAY);
    assert_eq!(e.asked().len(), 1);
    assert_eq!(e.asked()[0].1, CompletionTrigger::Invoked);
    assert!(e.shown());
}

#[gpui::test]
fn several_carets_or_a_snippet_under_way_ask_nothing(cx: &mut TestAppContext) {
    let mut e = editor("let x = |;\nlet y = ;\n", None, settings(), cx);
    let second = e.text().rfind(';').unwrap();
    e.view.update(e.cx, |view, cx| {
        let first = view.model().primary_selection().head();
        view.model_mut().set_selections(vec![
            ResolvedSelection {
                range: first..first,
                reversed: false,
            },
            ResolvedSelection {
                range: second..second,
                reversed: false,
            },
        ]);
        cx.notify();
    });
    draw(e.cx);
    e.keys("a l");
    e.wait(DELAY * 4);
    assert_eq!(e.asked(), [], "two carets");
    e.keys("escape");

    // In a snippet's first tabstop.
    e.view.update(e.cx, |view, cx| {
        view.model_mut().insert_snippet(
            0..0,
            &fe_editor::snippet::Snippet::parse("f(${1:x}, ${2:y})"),
        );
        cx.notify();
    });
    draw(e.cx);
    assert!(e.view.read_with(e.cx, |view, _| view.model().in_snippet()));
    e.keys("a l");
    e.wait(DELAY * 4);
    assert_eq!(e.asked(), [], "in a snippet");
    // `ctrl-space` asks there all the same.
    e.keys("ctrl-space");
    e.wait(DELAY);
    assert_eq!(e.asked().len(), 1);
}

/// Types `al` and waits for the menu to open by itself.
fn auto_opened(e: &mut Editor) {
    e.keys("a l");
    e.wait(DELAY);
    assert!(e.shown());
}

#[gpui::test]
fn enter_types_a_new_line_after_a_menu_that_opened_itself(cx: &mut TestAppContext) {
    let mut e = editor("let x = |;\n", None, settings(), cx);
    auto_opened(&mut e);
    e.keys("enter");
    assert_eq!(e.text(), "let x = al\n;\n");
    assert!(!e.shown());

    // `tab` accepts.
    e.keys("backspace");
    assert_eq!(e.text(), "let x = al;\n");
    e.view.update(e.cx, |view, cx| {
        view.model_mut().set_cursor(10);
        cx.notify();
    });
    e.keys("space a l");
    e.wait(DELAY);
    assert!(e.shown());
    e.keys("tab");
    assert_eq!(e.text(), "let x = al alpha;\n");
    assert!(!e.shown());
}

#[gpui::test]
fn enter_accepts_once_the_selection_moved_or_ctrl_space_opened_it(cx: &mut TestAppContext) {
    let mut e = editor("let x = |;\n", None, settings(), cx);
    auto_opened(&mut e);
    e.keys("down");
    e.keys("enter");
    assert_eq!(e.text(), "let x = alter;\n");
    assert_eq!(e.caret(), "let x = alter".len());

    e.keys("space a l ctrl-space");
    e.wait(DELAY);
    assert!(e.shown());
    e.keys("enter");
    assert_eq!(e.text(), "let x = alter alpha;\n");
}

#[gpui::test]
fn accept_on_enter_always_and_never(cx: &mut TestAppContext) {
    let always = CompletionSettings {
        accept_on_enter: AcceptOnEnter::Always,
        ..settings()
    };
    let mut e = editor("let x = |;\n", None, always, cx);
    auto_opened(&mut e);
    e.keys("enter");
    assert_eq!(e.text(), "let x = alpha;\n");

    let never = CompletionSettings {
        accept_on_enter: AcceptOnEnter::Never,
        ..settings()
    };
    e.view
        .update(e.cx, |view, _| view.set_completion_settings(&never));
    // Not even when asked for with `ctrl-space` and moved.
    e.keys("space a l ctrl-space");
    e.wait(DELAY);
    assert!(e.shown());
    e.keys("down enter");
    assert_eq!(e.text(), "let x = alpha al\n;\n");
    assert!(!e.shown());
    // `tab` accepts.
    e.keys("backspace ctrl-space");
    e.wait(DELAY);
    e.keys("tab");
    assert_eq!(e.text(), "let x = alpha alpha;\n");
}

#[gpui::test]
fn without_a_provider_the_buffer_s_words_open_the_menu(cx: &mut TestAppContext) {
    let wanted = CompletionSettings {
        words: CompletionWords::CurrentBuffer,
        ..CompletionSettings::default()
    };
    let mut e = editor("alpha alright be beta\n|", None, wanted, cx);
    e.view
        .update(e.cx, |view, _| view.set_completion_provider(None));
    e.keys("a l");
    e.wait(DELAY);
    assert!(e.shown());
    let items = e.view.read_with(e.cx, |view, _| view.completion_matches());
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, ["alpha", "alright"]);
    assert_eq!(items[0].kind.as_deref(), Some("text"));
    e.keys("tab");
    assert_eq!(e.text(), "alpha alright be beta\nalpha");
}

#[gpui::test]
fn enter_types_vim_s_new_line_in_insert_mode(cx: &mut TestAppContext) {
    let mut e = editor("let x = |;\n", None, settings(), cx);
    e.cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(e.cx);
    e.keys("i a l");
    e.wait(DELAY);
    assert!(e.shown());
    e.keys("enter");
    assert_eq!(e.text(), "let x = al\n;\n");
    assert!(!e.shown());
    // Still in insert mode: typing goes on.
    e.keys("z");
    assert_eq!(e.text(), "let x = al\nz;\n");
}

#[gpui::test]
fn accept_run_from_no_key_accepts(cx: &mut TestAppContext) {
    let mut e = editor("let x = |;\n", None, settings(), cx);
    auto_opened(&mut e);
    // As the command palette runs it: no keystroke to say it was `enter`.
    let frame = fe_command::ContextFrame::new(fe_gpui::KEY_CONTEXT).flag("completion");
    let invocation = fe_command::Invocation::of("completion.accept")
        .unwrap()
        .context(fe_command::ContextStack::new().with(frame));
    e.cx.update(|window, cx| {
        window.dispatch_action(Box::new(fe_gpui::commands::RunCommand(invocation)), cx)
    });
    e.cx.run_until_parked();
    draw(e.cx);
    assert_eq!(e.text(), "let x = alpha;\n");
    assert!(!e.shown());
}

#[gpui::test]
fn the_context_is_the_typed_char_s(cx: &mut TestAppContext) {
    // After a string's escape, still in the string; right after a block
    // comment's end, in code.
    let text = "fn main() {\n    let s = \"\\n|\";\n    /* c */\n}\n";
    let mut e = editor(text, Some("main.rs"), settings(), cx);
    e.keys("a");
    e.wait(DELAY * 4);
    assert_eq!(e.asked(), [], "after an escape, in a string");

    let after_comment = e.text().find("*/").unwrap() + 2;
    e.view.update(e.cx, |view, cx| {
        view.model_mut().set_cursor(after_comment);
        cx.notify();
    });
    draw(e.cx);
    e.keys("a l");
    e.wait(DELAY);
    assert_eq!(
        e.asked(),
        [(after_comment + 2, CompletionTrigger::Invoked)],
        "after a comment's end"
    );
}

#[gpui::test]
fn moving_away_and_back_drops_the_ask(cx: &mut TestAppContext) {
    let mut e = editor("let x = |;\n", None, settings(), cx);
    e.keys("a l");
    e.wait(DELAY / 2);
    e.keys("left right");
    e.wait(DELAY * 4);
    assert_eq!(e.asked(), []);
    assert!(!e.view.read_with(e.cx, |view, _| view.completion_pending()));
}

/// Answers each ask once the test lets it: `alpha` over the two chars
/// before the caret.
#[derive(Default)]
struct Gated {
    asked: RefCell<Vec<CompletionTrigger>>,
    gates: RefCell<Vec<futures::channel::oneshot::Sender<()>>>,
}

impl Gated {
    fn release(&self) {
        for gate in self.gates.borrow_mut().drain(..) {
            gate.send(()).ok();
        }
    }
}

impl CompletionProvider for Gated {
    fn trigger_characters(&self, _: BufferId) -> Vec<String> {
        Vec::new()
    }

    fn completions(
        &self,
        _: BufferId,
        _: Version,
        offset: usize,
        trigger: CompletionTrigger,
    ) -> CompletionFuture {
        self.asked.borrow_mut().push(trigger);
        let (send, receive) = futures::channel::oneshot::channel();
        self.gates.borrow_mut().push(send);
        let item = CompletionItem {
            label: "alpha".into(),
            edit: (offset.saturating_sub(2)..offset, "alpha".into()),
            ..CompletionItem::default()
        };
        Box::pin(async move {
            receive.await.ok()?;
            Some(CompletionList {
                items: vec![item],
                is_incomplete: false,
            })
        })
    }

    fn resolve(&self, item: CompletionItem) -> ResolveFuture {
        Box::pin(async move { item })
    }
}

#[gpui::test]
fn turning_auto_trigger_off_drops_an_automatic_menu_awaiting_its_answer(cx: &mut TestAppContext) {
    let mut e = editor("let x = |;\n", None, settings(), cx);
    let gated = Rc::new(Gated::default());
    let given = gated.clone();
    e.view
        .update(e.cx, |view, _| view.set_completion_provider(Some(given)));
    let off = CompletionSettings {
        auto_trigger: false,
        ..settings()
    };

    // Asked by itself, the answer comes after the setting turned off.
    e.keys("a l");
    e.wait(DELAY);
    assert_eq!(gated.asked.borrow().len(), 1);
    e.view
        .update(e.cx, |view, _| view.set_completion_settings(&off));
    gated.release();
    e.wait(DELAY);
    assert!(!e.shown());

    // Asked for, it stays.
    e.keys("ctrl-space");
    e.wait(DELAY);
    e.view
        .update(e.cx, |view, _| view.set_completion_settings(&off));
    gated.release();
    e.wait(DELAY);
    assert!(e.shown());
}

#[gpui::test]
fn a_long_buffer_s_words_follow_the_caret(cx: &mut TestAppContext) {
    let wanted = CompletionSettings {
        words: CompletionWords::CurrentBuffer,
        ..CompletionSettings::default()
    };
    // Over a MiB of rows with no word between the two.
    let filler = "zz\n".repeat(fe_editor::word_completion::SCAN_BYTES / 2);
    let text = format!("startword\n|{filler}endword\n");
    let mut e = editor(&text, None, wanted, cx);
    e.view
        .update(e.cx, |view, _| view.set_completion_provider(None));
    let labels = |e: &mut Editor| -> Vec<String> {
        (e.view
            .read_with(e.cx, |view, _| view.completion_matches())
            .into_iter())
        .map(|item| item.label)
        .collect()
    };
    e.keys("ctrl-space");
    e.wait(DELAY);
    assert_eq!(labels(&mut e), ["startword"]);
    e.keys("escape");

    // Moved, not edited: the same version, scanned again where the caret is.
    let frame = fe_command::ContextFrame::new(fe_gpui::KEY_CONTEXT);
    let invocation = fe_command::Invocation::of("editor.move_document_end")
        .unwrap()
        .context(fe_command::ContextStack::new().with(frame));
    e.cx.update(|window, cx| {
        window.dispatch_action(Box::new(fe_gpui::commands::RunCommand(invocation)), cx)
    });
    draw(e.cx);
    assert_eq!(e.caret(), e.text().len());
    e.keys("ctrl-space");
    e.wait(DELAY);
    assert_eq!(labels(&mut e), ["endword"]);
}

#[gpui::test]
fn a_doc_comment_s_link_text_is_in_the_comment(cx: &mut TestAppContext) {
    // Its Markdown's `markup.link` capture hides the comment's from the
    // highlights; the host tree still has it.
    let text = "/// See [|](https://example.com).\nfn main() {}\n";
    let mut e = editor(text, Some("main.rs"), settings(), cx);
    for _ in 0..5 {
        e.wait(DELAY);
    }
    e.keys("a");
    e.wait(DELAY * 40);
    assert_eq!(e.asked(), [], "in a doc comment's link");

    // With comments on, it asks there.
    let comments = CompletionSettings {
        in_comments: true,
        ..settings()
    };
    e.view
        .update(e.cx, |view, _| view.set_completion_settings(&comments));
    e.keys("l");
    e.wait(DELAY * 40);
    assert_eq!(e.asked().len(), 1);
}

#[gpui::test]
fn a_file_too_big_to_parse_whole_asks_nothing_by_itself(cx: &mut TestAppContext) {
    // Its window's tree can't say what the caret is in: nothing is asked,
    // as in a comment; `ctrl-space` asks.
    let rows = "plain words here\n".repeat(16 * 1024 * 1024 / 17);
    let text = format!("|{rows}");
    let mut e = editor(&text, Some("big.md"), settings(), cx);
    e.wait(DELAY);
    assert!(e.view.read_with(e.cx, |view, _| {
        view.syntax().is_some_and(|syntax| syntax.tree().is_none())
    }));
    e.keys("a l");
    e.wait(DELAY * 40);
    assert_eq!(e.asked(), []);
    e.keys("ctrl-space");
    e.wait(DELAY);
    assert_eq!(e.asked().len(), 1);
}

#[gpui::test]
fn a_markdown_file_parsed_whole_asks(cx: &mut TestAppContext) {
    let mut e = editor("Some words.\n\n|\n", Some("small.md"), settings(), cx);
    e.wait(DELAY);
    e.keys("a l");
    e.wait(DELAY * 4);
    assert_eq!(e.asked().len(), 1);
}

/// An editor on `text` set to Rust, its caret at `at`, with `settings` and
/// `provider`, drawn and focused but nothing in the background run: its
/// grammar, loaded for the first time in this test's process, still loads.
fn loading_rust_editor<'a>(
    text: &str,
    at: usize,
    settings: CompletionSettings,
    provider: Option<Rc<dyn CompletionProvider>>,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_cursor(at);
        let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
        view.set_completion_provider(provider);
        view.set_completion_settings(&settings);
        view
    });
    // Set where nothing runs until parked, as making the window does.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.set_language(Some("rust"), cx));
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    draw(cx);
    assert!(
        view.read_with(cx, |view, _| view.is_loading_language()),
        "the grammar loads in the background"
    );
    (view, cx)
}

/// Dispatches `key` without running anything in the background.
fn press(key: &str, cx: &mut VisualTestContext) {
    let window = cx.update(|window, _| window.window_handle());
    cx.dispatch_keystroke(window, gpui::Keystroke::parse(key).unwrap());
}

#[gpui::test]
fn a_language_still_loading_isnt_plain_text(cx: &mut TestAppContext) {
    cx.update(init);
    let provider = Rc::new(Provider::default());
    let wanted = CompletionSettings {
        delay_ms: 0,
        ..settings()
    };
    let text = "fn main() {}\n// note \n";
    let (view, cx) = loading_rust_editor(text, text.len() - 1, wanted, Some(provider.clone()), cx);
    // Typed in a comment before the grammar is there: nothing is asked,
    // then or once it is.
    press("a", cx);
    press("l", cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.model().text().to_string()),
        "fn main() {}\n// note al\n"
    );
    for _ in 0..40 {
        cx.executor().advance_clock(DELAY);
        cx.run_until_parked();
        draw(cx);
    }
    assert!(view.read_with(cx, |view, _| view.syntax().is_some()));
    assert_eq!(*provider.asked.borrow(), []);
}

#[gpui::test]
fn a_buffer_whose_language_loads_shares_no_words(cx: &mut TestAppContext) {
    cx.update(init);
    let wanted = CompletionSettings {
        auto_trigger: false,
        ..CompletionSettings::default()
    };
    // Plain text, with a word.
    let (_notes, cx_notes) = cx.add_window_view({
        let wanted = wanted.clone();
        move |_, cx| {
            let model = EditorModel::new(Buffer::new("alphabet\n"), Some("notes.txt".into()));
            let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
            view.set_completion_settings(&wanted);
            view
        }
    });
    cx_notes.run_until_parked();
    let text = "fn main() {}\nal";
    let (view, cx) = loading_rust_editor(text, text.len(), wanted, None, cx);
    press("ctrl-space", cx);
    cx.run_until_parked();
    draw(cx);
    let labels: Vec<String> = (view
        .read_with(cx, |view, _| view.completion_matches())
        .into_iter())
    .map(|item| item.label)
    .collect();
    assert!(!labels.contains(&"alphabet".to_owned()), "{labels:?}");
}

/// Has the input method start composing `text` at the caret.
fn compose(e: &mut Editor, text: &str) {
    use gpui::EntityInputHandler as _;
    e.cx.update(|window, cx| {
        e.view.update(cx, |view, cx| {
            view.replace_and_mark_text_in_range(None, text, None, window, cx)
        })
    });
    draw(e.cx);
}

fn marked(e: &mut Editor) -> Option<std::ops::Range<usize>> {
    use gpui::EntityInputHandler as _;
    e.cx.update(|window, cx| {
        e.view
            .update(cx, |view, cx| view.marked_text_range(window, cx))
    })
}

#[gpui::test]
fn a_composition_begun_within_the_delay_asks_nothing(cx: &mut TestAppContext) {
    let mut e = editor("let x = |;\n", None, settings(), cx);
    e.keys("a l");
    e.wait(DELAY / 2);
    compose(&mut e, "か");
    assert!(!e.view.read_with(e.cx, |view, _| view.completion_pending()));
    e.wait(DELAY * 4);
    assert_eq!(e.asked(), []);
    assert!(!e.shown());
    assert!(marked(&mut e).is_some(), "still composing");
}

#[gpui::test]
fn a_composition_closes_a_menu_that_opened_itself(cx: &mut TestAppContext) {
    let mut e = editor("let x = |;\n", None, settings(), cx);
    auto_opened(&mut e);
    compose(&mut e, "か");
    assert!(!e.shown());
    // `tab` isn't the menu's: no item goes in.
    e.keys("tab");
    assert!(!e.text().contains("alpha"), "{}", e.text());
}

#[gpui::test]
fn an_answer_landing_during_a_composition_opens_nothing(cx: &mut TestAppContext) {
    let mut e = editor("let x = |;\n", None, settings(), cx);
    let gated = Rc::new(Gated::default());
    let given = gated.clone();
    e.view
        .update(e.cx, |view, _| view.set_completion_provider(Some(given)));
    e.keys("a l");
    e.wait(DELAY);
    assert_eq!(gated.asked.borrow().len(), 1);
    compose(&mut e, "か");
    gated.release();
    e.wait(DELAY);
    assert!(!e.shown());
    assert!(marked(&mut e).is_some(), "still composing");
    // `enter` isn't the menu's either.
    e.keys("enter");
    assert!(!e.text().contains("alpha"), "{}", e.text());
}
