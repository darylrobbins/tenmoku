//! Signature help with a test [`SignatureHelpProvider`]: asked by typing a
//! trigger character, asked again as the arguments are typed, hidden by an
//! answer of nothing or `escape`, and placed over the caret's row, clear
//! of the completion menu.

use std::{cell::RefCell, rc::Rc, time::Duration};

use fe_buffer::{Buffer, BufferId, Version};
use fe_command::Invocation;
use fe_editor::{
    EditorModel,
    completion::{CompletionItem, CompletionList, CompletionTrigger},
};
use gpui::{
    BackgroundExecutor, Bounds, Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext,
    px,
};

use super::{Painted, SHOW};
use crate::*;

/// One request: the offset, the trigger, whether it's a retrigger and
/// whether it carried the help shown.
type Call = (usize, SignatureTrigger, bool, bool);

/// Answers every request with `reply`, after `delay` on the test clock,
/// and records each one. `(` triggers, `,` and `)` retrigger.
struct TestProvider {
    executor: BackgroundExecutor,
    reply: RefCell<Option<SignatureHelp>>,
    delay: RefCell<Duration>,
    calls: RefCell<Vec<Call>>,
}

impl SignatureHelpProvider for TestProvider {
    fn triggers(&self, _: BufferId) -> Option<(Vec<char>, Vec<char>)> {
        Some((vec!['('], vec![',', ')']))
    }

    fn signature_help(
        &self,
        _: BufferId,
        _: Version,
        offset: usize,
        context: SignatureContext,
    ) -> SignatureHelpFuture {
        let SignatureContext {
            trigger,
            is_retrigger,
            active,
        } = context;
        (self.calls.borrow_mut()).push((offset, trigger, is_retrigger, active.is_some()));
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

/// `add(a: i32, b: i32)`, documented, with parameter `active` active.
fn add(active: usize) -> Signature {
    let label = "add(a: i32, b: i32)";
    let range = |text: &str| {
        let start = label.find(text).unwrap();
        start..start + text.len()
    };
    Signature {
        label: label.into(),
        documentation: Some("Adds them.".into()),
        parameters: vec![
            Parameter {
                range: range("a: i32"),
                documentation: Some("The first.".into()),
            },
            Parameter {
                range: range("b: i32"),
                documentation: None,
            },
        ],
        active_parameter: Some(active),
    }
}

fn help(signatures: Vec<Signature>, active_signature: usize) -> Option<SignatureHelp> {
    Some(SignatureHelp {
        signatures,
        active_signature,
        active_parameter: None,
    })
}

/// A focused editor on `text`, its caret at `caret`, with the test
/// provider answering [`add`]`(0)`.
fn editor<'a>(
    text: &str,
    caret: usize,
    cx: &'a mut TestAppContext,
) -> (
    Entity<EditorView>,
    Rc<TestProvider>,
    &'a mut VisualTestContext,
) {
    cx.update(init);
    let provider = Rc::new(TestProvider {
        executor: cx.executor(),
        reply: RefCell::new(help(vec![add(0)], 0)),
        delay: RefCell::default(),
        calls: RefCell::default(),
    });
    let set = provider.clone();
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_cursor(caret);
        let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
        view.set_signature_help_provider(Some(set));
        view
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (view, provider, cx)
}

fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<Painted> {
    view.read_with(cx, |view, _| view.signature_help.painted.clone())
}

fn bold(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    painted(view, cx).and_then(|painted| painted.bold)
}

fn calls(provider: &TestProvider) -> Vec<Call> {
    provider.calls.borrow().clone()
}

fn input(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(text);
    cx.run_until_parked();
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

/// The caret's bounds in the last frame.
fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    view.update(cx, |view, _| {
        let head = view.model.primary_selection().head();
        view.layout.as_mut().unwrap().caret_bounds(head).unwrap()
    })
}

const CALL: &str = "fn main() {\n    add\n}\n";

#[gpui::test]
fn trigger_retrigger_and_hide(cx: &mut TestAppContext) {
    let at = CALL.find("add\n").unwrap() + 3;
    let (view, provider, cx) = editor(CALL, at, cx);
    use SignatureTrigger::*;

    // `(` asks, and the answer shows with its first parameter in bold, its
    // documentation then the signature's below.
    input("(", cx);
    assert_eq!(calls(&provider), [(at + 1, Character('('), false, false)]);
    let shown = painted(&view, cx).expect("the help shows");
    assert_eq!(shown.label, "add(a: i32, b: i32)");
    assert_eq!(shown.bold.as_deref(), Some("a: i32"));
    assert_eq!(shown.counter, None);
    assert_eq!(shown.documentation, ["The first.", "Adds them."]);

    // Typing an argument asks again as a content change; `,`, a retrigger
    // character, asks with it, and the bold parameter moves.
    input("1", cx);
    assert_eq!(calls(&provider)[1], (at + 2, ContentChange, true, true));
    *provider.reply.borrow_mut() = help(vec![add(1)], 0);
    input(",", cx);
    assert_eq!(calls(&provider)[2], (at + 3, Character(','), true, true));
    assert_eq!(bold(&view, cx).as_deref(), Some("b: i32"));
    assert_eq!(painted(&view, cx).unwrap().documentation, ["Adds them."]);
    // So does moving the caret within its rows.
    keys("left", cx);
    assert_eq!(calls(&provider)[3], (at + 2, ContentChange, true, true));
    keys("right", cx);

    // `)` past the call: the provider has nothing, which hides it, and
    // typing on asks no more.
    *provider.reply.borrow_mut() = None;
    input("2)", cx);
    assert_eq!(
        calls(&provider).last(),
        Some(&(at + 5, Character(')'), true, true))
    );
    assert_eq!(painted(&view, cx), None);
    let asked = calls(&provider).len();
    input(";", cx);
    assert_eq!(calls(&provider).len(), asked);

    // `escape` hides it, and drops its request.
    *provider.reply.borrow_mut() = help(vec![add(0)], 0);
    input("(", cx);
    assert!(painted(&view, cx).is_some());
    keys("escape", cx);
    assert_eq!(painted(&view, cx), None);
    input("x", cx);
    assert_eq!(calls(&provider).len(), asked + 1);

    // So does moving off the rows it was asked on, and a selection.
    input("(", cx);
    assert!(painted(&view, cx).is_some());
    keys("down", cx);
    assert_eq!(painted(&view, cx), None);
    keys("up", cx);
    input("(", cx);
    assert!(painted(&view, cx).is_some());
    keys("shift-left", cx);
    assert_eq!(painted(&view, cx), None);
    keys("right", cx);
    // A selection that leaves the caret where it was, too.
    input("(", cx);
    assert!(painted(&view, cx).is_some());
    view.update(cx, |view, cx| {
        let head = view.model.primary_selection().head();
        view.model_mut()
            .set_selections([fe_buffer::ResolvedSelection {
                range: head - 2..head,
                reversed: false,
            }]);
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(painted(&view, cx), None);
    keys("right", cx);

    // Losing focus hides it, with no input after to draw again.
    input("(", cx);
    assert!(painted(&view, cx).is_some());
    cx.update(|window, cx| {
        window.focus(&cx.focus_handle(), cx);
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    assert_eq!(painted(&view, cx), None);
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();

    // An answer for a version the view has moved on from is dropped, and
    // so is one landing on a selection.
    let delay = Duration::from_millis(100);
    *provider.delay.borrow_mut() = delay;
    input("(", cx);
    assert_eq!(painted(&view, cx), None);
    view.update(cx, |view, _| view.model_mut().insert("y"));
    cx.executor().advance_clock(delay);
    cx.run_until_parked();
    let shown = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.signature_help_shown().cloned())
    };
    assert_eq!(shown(cx), None);
    input("(", cx);
    view.update(cx, |view, _| {
        let head = view.model.primary_selection().head();
        view.model_mut()
            .set_selections([fe_buffer::ResolvedSelection {
                range: head - 1..head,
                reversed: false,
            }]);
    });
    cx.executor().advance_clock(delay);
    cx.run_until_parked();
    assert_eq!(shown(cx), None);
}

/// Rows of `text`: a call on each of rows 0, 5 and 25 of 40.
fn rows_text() -> String {
    (0..40)
        .map(|row| match row {
            0 | 5 | 25 => "add\n".to_owned(),
            _ => format!("let row{row} = {row};\n"),
        })
        .collect()
}

fn row_end(text: &str, row: usize) -> usize {
    let start: usize = text.lines().take(row).map(|line| line.len() + 1).sum();
    start + text.lines().nth(row).unwrap().len()
}

/// Resizes the window to `rows` rows high.
fn resize(view: &Entity<EditorView>, rows: f32, cx: &mut VisualTestContext) -> Pixels {
    let line_height = view.read_with(cx, |view, _| {
        px(view.layout.as_ref().unwrap().frame.line_height)
    });
    cx.simulate_resize(gpui::size(px(800.), line_height * rows));
    cx.run_until_parked();
    line_height
}

/// Puts the caret at `offset`, and has `completions` offer what's asked.
fn complete_at(
    view: &Entity<EditorView>,
    offset: usize,
    completions: Completions,
    cx: &mut VisualTestContext,
) {
    keys("escape", cx);
    view.update(cx, |view, cx| {
        view.set_completion_provider(Some(Rc::new(completions)));
        view.select_and_reveal(offset..offset, cx);
    });
    cx.run_until_parked();
}

/// The last frame's completion menu's bounds, and whether it's above the
/// caret.
fn menu(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (Bounds<Pixels>, bool) {
    let menu = view.read_with(cx, |view, _| view.painted_completion.clone());
    let menu = menu.expect("the menu shows");
    (menu.bounds.unwrap(), menu.place.above)
}

/// The help shows clear of the menu, within the editor's `height`.
fn assert_clear(
    view: &Entity<EditorView>,
    height: Pixels,
    cx: &mut VisualTestContext,
) -> Option<Painted> {
    let (menu, _) = menu(view, cx);
    let shown = painted(view, cx)?;
    let bounds = shown.bounds;
    let overlap = bounds.intersect(&menu);
    let clear = overlap.size.width <= px(1.) || overlap.size.height <= px(1.);
    assert!(clear, "{bounds:?} over {menu:?}");
    assert!(
        bounds.top() >= px(0.) && bounds.bottom() <= height,
        "{bounds:?}"
    );
    Some(shown)
}

#[gpui::test]
fn popover_placement(cx: &mut TestAppContext) {
    let text = rows_text();
    let (view, provider, cx) = editor(&text, row_end(&text, 5), cx);
    resize(&view, 30., cx);

    // Over the caret's row, from the caret.
    input("(", cx);
    let caret_at = caret(&view, cx);
    let shown = painted(&view, cx).expect("the help shows");
    assert!(shown.above);
    let bounds = shown.bounds;
    assert!(
        (bounds.bottom() - caret_at.top()).abs() <= px(1.),
        "{bounds:?}"
    );
    assert!(
        (bounds.left() - caret_at.left()).abs() <= px(1.),
        "{bounds:?}"
    );

    // On the first visible row, below it.
    keys("escape", cx);
    let first = row_end(&text, 0);
    view.update(cx, |view, cx| view.select_and_reveal(first..first, cx));
    cx.run_until_parked();
    input("(", cx);
    let caret_at = caret(&view, cx);
    let shown = painted(&view, cx).expect("the help shows");
    assert!(!shown.above);
    assert!(
        (shown.bounds.top() - caret_at.bottom()).abs() <= px(1.),
        "{:?}",
        shown.bounds
    );

    // With several signatures, `n/m` and the active one's label.
    let other = Signature {
        label: "add(a: f64)".into(),
        documentation: None,
        parameters: Vec::new(),
        active_parameter: None,
    };
    *provider.reply.borrow_mut() = help(vec![add(0), other.clone(), add(1)], 1);
    view.update(cx, |view, cx| {
        let show = Invocation::of(SHOW).unwrap();
        view.run_command(show, None, cx)
    });
    cx.run_until_parked();
    // `editor.show_signature_help` asks as invoked, a retrigger.
    let last = calls(&provider).last().cloned().unwrap();
    assert_eq!(last, (first + 1, SignatureTrigger::Invoked, true, true));
    let shown = painted(&view, cx).unwrap();
    assert_eq!(shown.counter.as_deref(), Some("2/3"));
    assert_eq!(shown.label, "add(a: f64)");
    assert_eq!(shown.bold, None);

    // Near the window's bottom the completion menu opens above the caret,
    // and the help above the menu, a wrapped label and all.
    let height = resize(&view, 30., cx) * 30.;
    let call = row_end(&text, 25);
    complete_at(&view, call, Completions::new(2, None), cx);
    let mut long = add(0);
    long.label = format!("add(a: i32, b: i32{})", ", more: i32".repeat(20));
    *provider.reply.borrow_mut() = help(vec![long], 0);
    input("(", cx);
    keys("ctrl-space", cx);
    let (menu_at, menu_above) = menu(&view, cx);
    assert!(menu_above);
    let shown = assert_clear(&view, height, cx).expect("the help shows");
    assert!(shown.above);
    assert!(shown.bounds.bottom() <= menu_at.top() + px(1.));
    // The label wraps: more than one row for it, and the docs' two.
    let line_height = height / 30.;
    assert!(shown.bounds.size.height > line_height * 3.5, "{shown:?}");
    // Tall documentation beside the menu's rows makes it taller than them:
    // the help still clears it.
    let docs = (0..30).map(|n| format!("Line {n}.")).collect::<Vec<_>>();
    complete_at(
        &view,
        call + 1,
        Completions::new(2, Some(docs.join("\n"))),
        cx,
    );
    *provider.reply.borrow_mut() = help(vec![add(0)], 0);
    input("(", cx);
    keys("ctrl-space", cx);
    cx.executor().advance_clock(Duration::from_millis(10));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| {
        (view.painted_completion.as_ref()).is_some_and(|menu| menu.documentation.is_some())
    }));
    let shown = assert_clear(&view, height, cx).expect("the help shows");
    assert!(shown.bounds.bottom() <= menu(&view, cx).0.top() + px(1.));

    // In a short window, with no room for it by the caret on either side
    // of a long menu, it's cut to fit where there's more, still clear of
    // the menu: below it on the first row, the menu below too.
    let height = resize(&view, 15., cx) * 15.;
    complete_at(&view, first, Completions::new(12, None), cx);
    input("(", cx);
    keys("ctrl-space", cx);
    let (_, menu_above) = menu(&view, cx);
    assert!(!menu_above);
    // Cut from the label's row and the docs' two to fit.
    let shown = assert_clear(&view, height, cx).expect("the help shows");
    assert!(!shown.above);
    assert!(shown.bounds.size.height < line_height * 2.5, "{shown:?}");
    // And near the bottom, the menu above.
    let low = row_end(&text, 14);
    complete_at(&view, low, Completions::new(12, None), cx);
    input("(", cx);
    keys("ctrl-space", cx);
    let (_, menu_above) = menu(&view, cx);
    assert!(menu_above);
    assert_clear(&view, height, cx);

    // Soft wrap has begun the argument being completed a display row above
    // the caret, near the editor's top: the menu, below the word's start,
    // spans the caret's row, and the help goes below the menu, shown.
    let height = resize(&view, 30., cx) * 30.;
    cx.simulate_resize(gpui::size(px(400.), height));
    cx.run_until_parked();
    let argument = "x".repeat(120);
    view.update(cx, |view, cx| {
        let model = view.model_mut();
        model.set_cursor(0);
        model.insert(&format!("add({argument})\n"));
        model.set_soft_wrap(true);
        cx.notify();
    });
    cx.run_until_parked();
    let end = "add(".len() + argument.len();
    let completions = Completions {
        prefix: argument.clone(),
        ..Completions::new(12, None)
    };
    complete_at(&view, end, completions, cx);
    view.update(cx, |view, cx| {
        let show = Invocation::of(SHOW).unwrap();
        view.run_command(show, None, cx)
    });
    cx.run_until_parked();
    keys("ctrl-space", cx);
    let (word, caret_at) = view.update(cx, |view, _| {
        let layout = view.layout.as_mut().unwrap();
        let word = layout.caret_bounds("add(".len()).unwrap();
        (word, layout.caret_bounds(end).unwrap())
    });
    assert!(word.top() < caret_at.top(), "{word:?} {caret_at:?}");
    let (menu_at, _) = menu(&view, cx);
    assert!(menu_at.top() < caret_at.bottom() && menu_at.bottom() > caret_at.top());
    let shown = assert_clear(&view, height, cx).expect("the help shows");
    assert!(!shown.above);
    assert!(shown.bounds.top() >= menu_at.bottom() - px(1.), "{shown:?}");
}

#[gpui::test]
fn vim_insert_only(cx: &mut TestAppContext) {
    let at = CALL.find("add\n").unwrap() + 2;
    let (view, provider, cx) = editor(CALL, at, cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    let mode = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.vim_mode());

    // In normal mode, `(` is vim's: nothing is asked.
    keys("(", cx);
    assert_eq!(calls(&provider), []);
    view.update(cx, |view, cx| view.select_and_reveal(at..at, cx));
    view.update(cx, |view, cx| {
        let show = Invocation::of(SHOW).unwrap();
        view.run_command(show, None, cx)
    });
    cx.run_until_parked();
    assert_eq!(calls(&provider), []);
    // Nor in replace mode.
    keys("l R (", cx);
    assert_eq!(mode(cx), Some("replace"));
    assert_eq!(calls(&provider), []);
    keys("escape u", cx);
    view.update(cx, |view, cx| view.select_and_reveal(at..at, cx));
    cx.run_until_parked();

    // In insert mode, typing it asks; leaving insert mode hides it.
    keys("a", cx);
    assert_eq!(mode(cx), Some("insert"));
    keys("(", cx);
    assert_eq!(
        calls(&provider),
        [(at + 2, SignatureTrigger::Character('('), false, false)]
    );
    assert!(painted(&view, cx).is_some());
    keys("ctrl-[", cx);
    assert_eq!(mode(cx), Some("normal"));
    assert_eq!(painted(&view, cx), None);
    assert_eq!(calls(&provider).len(), 1);

    // Turning vim on hides help shown, and drops an answer awaited.
    cx.update(|_, cx| set_vim_enabled(cx, false));
    keys("end (", cx);
    assert!(painted(&view, cx).is_some());
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    assert_eq!(painted(&view, cx), None);
    cx.update(|_, cx| set_vim_enabled(cx, false));
    let delay = Duration::from_millis(100);
    *provider.delay.borrow_mut() = delay;
    keys("(", cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.executor().advance_clock(delay);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.signature_help_shown().cloned()),
        None
    );
}

/// Offers `count` items at the caret, each `prefix` and a number, with
/// `documentation` once resolved.
struct Completions {
    count: usize,
    documentation: Option<String>,
    prefix: String,
}

impl Completions {
    fn new(count: usize, documentation: Option<String>) -> Self {
        Self {
            count,
            documentation,
            prefix: "item".into(),
        }
    }
}

impl CompletionProvider for Completions {
    fn trigger_characters(&self, _: BufferId) -> Vec<String> {
        Vec::new()
    }

    fn completions(
        &self,
        _: BufferId,
        _: Version,
        offset: usize,
        _: CompletionTrigger,
    ) -> CompletionFuture {
        let items = (0..self.count)
            .map(|n| CompletionItem {
                label: format!("{}{n}", self.prefix),
                edit: (offset..offset, format!("{}{n}", self.prefix)),
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

    fn resolve(&self, mut item: CompletionItem) -> ResolveFuture {
        item.documentation = self.documentation.clone();
        Box::pin(async move { item })
    }
}
