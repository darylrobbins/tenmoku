//! The hover popover with a [`HoverProvider`]: asked once the pointer has
//! rested on a word, or at once at the caret on `editor.show_hover`, its
//! markdown shown as text with fenced code in the editor's font, below any
//! diagnostics there, and hidden by whatever moves it out of date.

use std::{cell::RefCell, ops::Range, rc::Rc, time::Duration};

use fe_buffer::{Buffer, BufferId, ResolvedSelection, Version};
use fe_editor::{
    EditorModel,
    diagnostics::{Diagnostic, Severity},
};
use gpui::{
    BackgroundExecutor, Entity, Focusable as _, Modifiers, Pixels, Point, ScrollDelta,
    ScrollWheelEvent, SharedString, TestAppContext, TouchPhase, VisualTestContext, point, px,
};

use crate::{
    hover::{Block, HOVER_DELAY, UI_FONT},
    *,
};

const TEXT: &str = "fn main() {\n    let total = alpha + beta;\n}\n";

/// What the test provider answers.
const DOCS: &str = "```rust\nlet alpha: i32\n```\nThe *alpha* value.\n";

fn word(word: &str) -> Range<usize> {
    let start = TEXT.find(word).unwrap();
    start..start + word.len()
}

/// Answers every request with `reply`, after `delay` on the test clock,
/// and records each one.
struct TestProvider {
    executor: BackgroundExecutor,
    reply: RefCell<Option<HoverContent>>,
    delay: RefCell<Duration>,
    calls: RefCell<Vec<(BufferId, Version, usize)>>,
}

impl HoverProvider for TestProvider {
    fn hover(&self, buffer: BufferId, version: Version, offset: usize) -> HoverFuture {
        self.calls.borrow_mut().push((buffer, version, offset));
        let (reply, delay) = (self.reply.borrow().clone(), *self.delay.borrow());
        let executor = self.executor.clone();
        Box::pin(async move {
            if !delay.is_zero() {
                executor.timer(delay).await;
            }
            reply
        })
    }
}

/// A focused editor on [`TEXT`] with the caret at `caret`, matching keys
/// against the default keymap, and the test provider answering [`DOCS`].
fn editor(
    caret: usize,
    cx: &mut TestAppContext,
) -> (Entity<EditorView>, Rc<TestProvider>, &mut VisualTestContext) {
    cx.update(init);
    let provider = Rc::new(TestProvider {
        executor: cx.executor(),
        reply: RefCell::new(Some(HoverContent {
            text: DOCS.into(),
            range: None,
        })),
        delay: RefCell::new(Duration::ZERO),
        calls: RefCell::default(),
    });
    let set = provider.clone();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), None);
        model.set_selections(vec![ResolvedSelection {
            range: caret..caret,
            reversed: false,
        }]);
        let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
        view.set_hover_provider(Some(set));
        view
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (view, provider, cx)
}

/// Mid-row, just right of the caret position at `offset`, in the last
/// frame.
fn at(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    view.update(cx, |view, _| {
        let caret = view.layout.as_mut().unwrap().caret_bounds(offset).unwrap();
        point(caret.left() + px(1.), caret.center().y)
    })
}

/// The last frame's popover: each part, and the font it was set in.
fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(Block, SharedString)> {
    view.read_with(cx, |view, _| view.painted_popover_blocks.clone())
}

fn move_to(position: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(position, None, Modifiers::none());
    cx.run_until_parked();
}

/// Rests the pointer at `position` for the hover delay.
fn rest(position: Point<Pixels>, cx: &mut VisualTestContext) {
    move_to(position, cx);
    cx.executor().advance_clock(HOVER_DELAY);
    cx.run_until_parked();
}

fn calls(provider: &TestProvider) -> Vec<usize> {
    let calls = provider.calls.borrow();
    calls.iter().map(|&(_, _, offset)| offset).collect()
}

#[gpui::test]
fn popover_after_the_delay(cx: &mut TestAppContext) {
    let (view, provider, cx) = editor(0, cx);
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    let docs = vec![
        (
            Block::Code("let alpha: i32".into()),
            theme.font_family.clone(),
        ),
        (Block::Text("The *alpha* value.".into()), UI_FONT.into()),
    ];
    let alpha = word("alpha");

    // Not before the delay is up.
    move_to(at(&view, alpha.start + 1, cx), cx);
    cx.executor()
        .advance_clock(HOVER_DELAY - Duration::from_millis(1));
    cx.run_until_parked();
    assert!(calls(&provider).is_empty());
    assert_eq!(painted(&view, cx), []);

    // Then asked once, about this buffer and version, where the pointer is.
    cx.executor().advance_clock(Duration::from_millis(1));
    cx.run_until_parked();
    let (buffer, version) = view.read_with(cx, |view, _| {
        (view.model.buffer().id(), view.model.snapshot().version())
    });
    assert_eq!(
        *provider.calls.borrow(),
        [(buffer, version, alpha.start + 1)]
    );
    assert_eq!(painted(&view, cx), docs);

    // Moves within the word keep it, without asking again.
    move_to(at(&view, alpha.end - 1, cx), cx);
    cx.executor().advance_clock(HOVER_DELAY * 3);
    cx.run_until_parked();
    assert_eq!(calls(&provider), [alpha.start + 1]);
    assert_eq!(painted(&view, cx), docs);

    // Off the word, onto a space: gone.
    move_to(at(&view, alpha.start - 1, cx), cx);
    assert_eq!(painted(&view, cx), []);

    // Typing.
    rest(at(&view, alpha.start, cx), cx);
    assert_eq!(painted(&view, cx), docs);
    cx.simulate_input("x");
    assert_eq!(
        view.read_with(cx, |view, _| view.model.text().to_string()),
        format!("x{TEXT}")
    );
    assert_eq!(painted(&view, cx), []);
    cx.simulate_keystrokes("backspace");

    // Scrolling.
    let alpha = word("alpha");
    rest(at(&view, alpha.start, cx), cx);
    assert_eq!(painted(&view, cx), docs);
    let position = at(&view, alpha.start, cx);
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(0.), px(0.))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    cx.run_until_parked();
    assert_eq!(painted(&view, cx), []);

    // Escape.
    rest(at(&view, alpha.start + 2, cx), cx);
    assert_eq!(painted(&view, cx), docs);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(painted(&view, cx), []);

    // An edit while the delay runs: nothing is asked about, or shown.
    move_to(at(&view, alpha.start - 1, cx), cx);
    let asked = calls(&provider).len();
    move_to(at(&view, alpha.start, cx), cx);
    cx.executor().advance_clock(HOVER_DELAY / 2);
    view.update(cx, |view, cx| {
        view.model_mut().set_cursor(TEXT.len());
        view.type_text("x", cx);
    });
    cx.run_until_parked();
    cx.executor().advance_clock(HOVER_DELAY);
    cx.run_until_parked();
    assert_eq!(calls(&provider).len(), asked);
    assert_eq!(painted(&view, cx), []);
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();

    // Another word is asked about in turn.
    let beta = word("beta");
    rest(at(&view, beta.start, cx), cx);
    assert_eq!(calls(&provider).last(), Some(&beta.start));
    assert_eq!(painted(&view, cx), docs);

    // An answer for a version the buffer has moved on from by the time
    // it comes isn't shown, though the pointer rests where it was asked.
    move_to(at(&view, alpha.start - 1, cx), cx);
    *provider.delay.borrow_mut() = Duration::from_secs(1);
    let asked = calls(&provider).len();
    rest(at(&view, alpha.start, cx), cx);
    assert_eq!(calls(&provider).len(), asked + 1);
    // An edit past the pointer, which leaves the text under it in place.
    view.update(cx, |view, cx| {
        view.model_mut().set_cursor(TEXT.len());
        view.type_text("x", cx);
    });
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(painted(&view, cx), []);
    // Nor once the pointer moves within the word, which is still what it
    // was asked about: a fresh wait asks again, for the new version.
    move_to(at(&view, alpha.start + 1, cx), cx);
    assert_eq!(painted(&view, cx), []);
    assert_eq!(calls(&provider).len(), asked + 1);
}

#[gpui::test]
fn show_hover_command(cx: &mut TestAppContext) {
    let beta = word("beta");
    let (view, provider, cx) = editor(beta.start + 2, cx);
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    let docs = vec![
        (
            Block::Code("let alpha: i32".into()),
            theme.font_family.clone(),
        ),
        (Block::Text("The *alpha* value.".into()), UI_FONT.into()),
    ];

    // At the caret, at once: no delay.
    cx.simulate_keystrokes("cmd-k cmd-i");
    cx.run_until_parked();
    assert_eq!(calls(&provider), [beta.start + 2]);
    assert_eq!(painted(&view, cx), docs);
    // The pointer moving over nothing to show leaves it be; the caret
    // moving hides it.
    move_to(at(&view, word("main").start - 1, cx), cx);
    assert_eq!(painted(&view, cx), docs);
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert_eq!(painted(&view, cx), []);

    // With a diagnostic under the caret: its message, then the hover's.
    view.update(cx, |view, cx| {
        let version = view.model().snapshot().version();
        let items = vec![(beta.clone(), Diagnostic::new(Severity::Warning, "unused"))];
        assert!(
            view.model_mut()
                .apply_diagnostics(1u32, (1, 1), version, items, Some((1, 1)))
        );
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("cmd-k cmd-i");
    cx.run_until_parked();
    assert_eq!(calls(&provider), [beta.start + 2, beta.start + 3]);
    let mut both = vec![(Block::Text("unused".into()), SharedString::from(UI_FONT))];
    both.extend(docs.clone());
    assert_eq!(painted(&view, cx), both);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(painted(&view, cx), []);

    // Just past the word's end, the word before it is asked about.
    view.update(cx, |view, cx| {
        view.model_mut().set_cursor(beta.end);
        cx.notify();
    });
    cx.simulate_keystrokes("cmd-k cmd-i");
    cx.run_until_parked();
    assert_eq!(calls(&provider).last(), Some(&beta.start));
}

#[test]
fn markdown_fences() {
    use crate::hover::markdown_blocks;
    let text = |s: &str| Block::Text(s.into());
    let code = |s: &str| Block::Code(s.into());
    assert_eq!(
        markdown_blocks("\nIntro.\n\n```rust\nfn f() {}\n\nlet x;\n```\n\n---\nOutro *as is*.\n"),
        [
            text("Intro."),
            code("fn f() {}\n\nlet x;"),
            text("---\nOutro *as is*.")
        ]
    );
    // Tildes, a longer fence holding a shorter one, and one never closed.
    assert_eq!(
        markdown_blocks("~~~\na\n~~~\n````\n```\n````\n   ```\nopen"),
        [code("a"), code("```"), code("open")]
    );
    // Not fences: two backticks, four spaces in, a backtick in the info.
    assert_eq!(
        markdown_blocks("``x``\n    ```\n``` a`b"),
        [text("``x``\n    ```\n``` a`b")]
    );
}

#[gpui::test]
fn ranges_nothing_to_say_and_vim(cx: &mut TestAppContext) {
    let (view, provider, cx) = editor(0, cx);
    let (alpha, plus, beta) = (word("alpha"), word("+"), word("beta"));
    let docs = |text: &str| (Block::Text(text.into()), SharedString::from(UI_FONT));
    let reply = |text: &str, range: Option<Range<usize>>| {
        *provider.reply.borrow_mut() = Some(HoverContent {
            text: text.into(),
            range,
        });
    };

    // An answer's range: the popover stays across it, onto other words,
    // without asking again; off it, it goes.
    reply("the sum", Some(alpha.start..beta.end));
    rest(at(&view, alpha.start, cx), cx);
    assert_eq!(painted(&view, cx), [docs("the sum")]);
    move_to(at(&view, beta.start + 1, cx), cx);
    cx.executor().advance_clock(HOVER_DELAY);
    cx.run_until_parked();
    assert_eq!(painted(&view, cx), [docs("the sum")]);
    assert_eq!(calls(&provider), [alpha.start]);
    move_to(at(&view, word("total").start, cx), cx);
    assert_eq!(painted(&view, cx), []);

    // Nothing to say: nothing shows, and moves within the word don't ask
    // again.
    *provider.reply.borrow_mut() = None;
    let asked = calls(&provider).len();
    rest(at(&view, beta.start, cx), cx);
    move_to(at(&view, beta.start + 2, cx), cx);
    cx.executor().advance_clock(HOVER_DELAY * 2);
    cx.run_until_parked();
    assert_eq!(calls(&provider).len(), asked + 1);
    assert_eq!(painted(&view, cx), []);

    // Within one diagnostic, onto another word: the message stays, and
    // the new word is asked about after the delay.
    reply("about it", None);
    view.update(cx, |view, cx| {
        let version = view.model().snapshot().version();
        let items = vec![(
            alpha.start..beta.end,
            Diagnostic::new(Severity::Error, "mismatched types"),
        )];
        assert!(
            view.model_mut()
                .apply_diagnostics(1u32, (1, 1), version, items, Some((1, 1)))
        );
        cx.notify();
    });
    cx.run_until_parked();
    rest(at(&view, alpha.start, cx), cx);
    let message = docs("mismatched types");
    assert_eq!(painted(&view, cx), [message.clone(), docs("about it")]);
    move_to(at(&view, plus.start, cx), cx);
    assert_eq!(painted(&view, cx), std::slice::from_ref(&message));
    cx.executor().advance_clock(HOVER_DELAY);
    cx.run_until_parked();
    assert_eq!(calls(&provider).last(), Some(&plus.start));
    assert_eq!(painted(&view, cx), [message, docs("about it")]);

    // In vim's insert mode, where `escape` is bound to leave it, `escape`
    // hides it too; and cmd-k cmd-i there keeps the insert session.
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.simulate_keystrokes("i");
    cx.run_until_parked();
    let mode = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.vim_mode());
    assert_eq!(mode(cx), Some("insert"));
    assert_ne!(painted(&view, cx), []);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(mode(cx), Some("normal"));
    assert_eq!(painted(&view, cx), []);
    cx.simulate_keystrokes("i cmd-k cmd-i");
    cx.run_until_parked();
    assert_eq!(mode(cx), Some("insert"));
    assert_eq!(calls(&provider).last(), Some(&0));
    assert_eq!(painted(&view, cx), [docs("about it")]);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(painted(&view, cx), []);
}
