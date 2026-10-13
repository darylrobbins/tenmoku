//! The completion menu with a test [`CompletionProvider`]: opened by
//! `ctrl-space` or a trigger character at the word's start, narrowed by
//! typing, driven by its keys, closed without editing by `escape` or the
//! caret leaving the word, and building only the rows that fit.

use std::{cell::RefCell, rc::Rc, time::Duration};

use fe_buffer::{Buffer, BufferId, Version};
use fe_editor::{
    EditorModel,
    completion::{CompletionItem, CompletionList, CompletionTrigger},
};
use gpui::{BackgroundExecutor, Entity, Focusable as _, TestAppContext, VisualTestContext, point};

use crate::{
    completion_menu::{MAX_ROWS, Painted},
    *,
};

pub(super) const TEXT: &str = "fn main() {\n    va\n}\n";

/// How long the test provider takes to resolve.
const RESOLVE: Duration = Duration::from_millis(100);

/// Offers `labels` over the word of `back` bytes before the offset asked
/// at, resolves after [`RESOLVE`] with documentation, and records each
/// request.
pub(super) struct TestProvider {
    executor: BackgroundExecutor,
    labels: RefCell<Vec<String>>,
    back: RefCell<usize>,
    pub(super) calls: RefCell<Vec<(usize, CompletionTrigger)>>,
    /// Answer after this long, with lists this incomplete, or nothing.
    delay: RefCell<Duration>,
    incomplete: RefCell<bool>,
    nothing: RefCell<bool>,
}

impl CompletionProvider for TestProvider {
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
        self.calls.borrow_mut().push((offset, trigger));
        let range = offset - *self.back.borrow()..offset;
        let items = (self.labels.borrow().iter())
            .map(|label| CompletionItem {
                label: label.clone(),
                kind: Some("variable".into()),
                detail: Some(format!("let {label}: u8")),
                edit: (range.clone(), label.clone()),
                ..CompletionItem::default()
            })
            .collect();
        let (executor, delay) = (self.executor.clone(), *self.delay.borrow());
        let (is_incomplete, nothing) = (*self.incomplete.borrow(), *self.nothing.borrow());
        Box::pin(async move {
            if !delay.is_zero() {
                executor.timer(delay).await;
            }
            (!nothing).then_some(CompletionList {
                items,
                is_incomplete,
            })
        })
    }

    fn resolve(&self, mut item: CompletionItem) -> ResolveFuture {
        let executor = self.executor.clone();
        Box::pin(async move {
            executor.timer(RESOLVE).await;
            item.documentation = Some(format!("The *{}*.", item.label));
            item
        })
    }
}

/// A focused editor on [`TEXT`], the caret after `va`, with the test
/// provider offering `value`, `values` and `vary`.
pub(super) fn editor(
    cx: &mut TestAppContext,
) -> (Entity<EditorView>, Rc<TestProvider>, &mut VisualTestContext) {
    cx.update(init);
    let provider = Rc::new(TestProvider {
        executor: cx.executor(),
        labels: RefCell::new(["value", "values", "vary"].map(String::from).to_vec()),
        back: RefCell::new(2),
        calls: RefCell::default(),
        delay: RefCell::default(),
        incomplete: RefCell::default(),
        nothing: RefCell::default(),
    });
    let set = provider.clone();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), None);
        model.set_cursor(TEXT.find("va\n").unwrap() + 2);
        let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
        view.set_completion_provider(Some(set));
        view
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (view, provider, cx)
}

pub(super) fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<Painted> {
    view.read_with(cx, |view, _| view.painted_completion.clone())
}

/// The labels of the last frame's rows, and the selected one's.
pub(super) fn rows(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (Vec<String>, Option<String>) {
    let Some(painted) = painted(view, cx) else {
        return (Vec::new(), None);
    };
    let labels: Vec<String> = painted.rows.iter().map(|row| row.0.clone()).collect();
    let selected = painted.selected.map(|ix| labels[ix].clone());
    (labels, selected)
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

#[gpui::test]
fn menu_opens_narrows_and_accepts(cx: &mut TestAppContext) {
    let (view, provider, cx) = editor(cx);
    let va = TEXT.find("va\n").unwrap();

    // `ctrl-space` asks at the caret, and the menu opens below the word's
    // start, its first item selected and being resolved.
    keys("ctrl-space", cx);
    assert_eq!(
        *provider.calls.borrow(),
        [(va + 2, CompletionTrigger::Invoked)]
    );
    let shown = painted(&view, cx).expect("the menu shows");
    let word = view.update(cx, |view, _| {
        view.layout.as_mut().unwrap().caret_bounds(va).unwrap()
    });
    assert!(!shown.place.above);
    assert_eq!(shown.place.at, point(word.left(), word.bottom()));
    // Laid out there, but for its border.
    let bounds = shown.bounds.unwrap();
    assert!((bounds.left() - word.left()).abs() <= px(1.), "{bounds:?}");
    assert!((bounds.top() - word.bottom()).abs() <= px(1.), "{bounds:?}");
    assert_eq!(
        shown.rows,
        ["value", "values", "vary"].map(|label| (
            label.into(),
            format!("let {label}: u8"),
            "variable".into()
        ))
    );
    assert_eq!(shown.selected, Some(0));
    assert_eq!(shown.documentation, None);
    // Its documentation, once resolved.
    cx.executor().advance_clock(RESOLVE);
    cx.run_until_parked();
    let documentation = painted(&view, cx).unwrap().documentation;
    assert_eq!(documentation.as_deref(), Some("The *value*."));

    // Typing narrows it; `down` and `up` move the selection.
    cx.simulate_input("l");
    cx.run_until_parked();
    assert_eq!(
        rows(&view, cx),
        (vec!["value".into(), "values".into()], Some("value".into()))
    );
    keys("down", cx);
    assert_eq!(rows(&view, cx).1.as_deref(), Some("values"));
    cx.executor().advance_clock(RESOLVE);
    cx.run_until_parked();
    let documentation = painted(&view, cx).unwrap().documentation;
    assert_eq!(documentation.as_deref(), Some("The *values*."));
    keys("up", cx);
    assert_eq!(rows(&view, cx).1.as_deref(), Some("value"));

    // `enter` accepts, over the word typed since; one undo takes it back.
    keys("enter", cx);
    assert_eq!(text(&view, cx), "fn main() {\n    value\n}\n");
    assert_eq!(painted(&view, cx), None);
    keys("cmd-z", cx);
    assert_eq!(text(&view, cx), "fn main() {\n    val\n}\n");

    // `tab` accepts too.
    *provider.back.borrow_mut() = 3;
    keys("ctrl-space", cx);
    keys("down", cx);
    keys("tab", cx);
    assert_eq!(text(&view, cx), "fn main() {\n    values\n}\n");
    keys("cmd-z", cx);

    // `escape` closes it without editing, and so does the caret leaving
    // the word.
    keys("ctrl-space", cx);
    assert!(painted(&view, cx).is_some());
    keys("escape", cx);
    assert_eq!(painted(&view, cx), None);
    assert_eq!(text(&view, cx), "fn main() {\n    val\n}\n");
    keys("ctrl-space", cx);
    assert!(painted(&view, cx).is_some());
    keys("cmd-left", cx);
    assert_eq!(painted(&view, cx), None);
    assert_eq!(text(&view, cx), "fn main() {\n    val\n}\n");
    // A key the menu took nothing from moves the caret as before.
    keys("down", cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.model().primary_selection().head()),
        text(&view, cx).find("}\n").unwrap() + 1
    );

    // Typing a trigger character opens it, at the caret.
    keys("cmd-up cmd-right", cx);
    *provider.back.borrow_mut() = 0;
    provider.calls.borrow_mut().clear();
    cx.simulate_input(".");
    cx.run_until_parked();
    let dot = "fn main() {.".len();
    assert_eq!(
        *provider.calls.borrow(),
        [(dot, CompletionTrigger::Character(".".into()))]
    );
    assert_eq!(rows(&view, cx).0.len(), 3);
    keys("escape", cx);

    // With 1,000 items, a frame builds the rows that fit, and follows the
    // selection.
    *provider.labels.borrow_mut() = (0..1000).map(|ix| format!("item{ix:04}")).collect();
    keys("ctrl-space", cx);
    let (labels, selected) = rows(&view, cx);
    assert_eq!(labels.len(), MAX_ROWS);
    assert_eq!(selected.as_deref(), Some("item0000"));
    keys("up", cx);
    let (labels, selected) = rows(&view, cx);
    assert_eq!(labels.len(), MAX_ROWS);
    assert_eq!(labels.last().map(String::as_str), Some("item0999"));
    assert_eq!(selected.as_deref(), Some("item0999"));

    // In a short window, fewer: the menu's laid out within it.
    let height = px(160.);
    cx.simulate_resize(gpui::size(px(800.), height));
    cx.run_until_parked();
    keys("down", cx);
    let shown = painted(&view, cx).unwrap();
    let bounds = shown.bounds.unwrap();
    assert!(
        (1..MAX_ROWS).contains(&shown.rows.len()),
        "{}",
        shown.rows.len()
    );
    assert_eq!(shown.rows.len(), shown.place.rows);
    assert!(
        bounds.top() >= px(0.) && bounds.bottom() <= height,
        "{bounds:?}"
    );
    assert_eq!(rows(&view, cx).1.as_deref(), Some("item0000"));
    keys("up", cx);
    let (labels, selected) = rows(&view, cx);
    assert_eq!(labels.last().map(String::as_str), Some("item0999"));
    assert_eq!(selected.as_deref(), Some("item0999"));

    // In a font so large no row fits by the word, it closes, and its keys
    // are the editor's again.
    view.update(cx, |view, cx| {
        let settings = EditorSettings {
            font_size: 72.,
            ..EditorSettings::default()
        };
        view.apply_settings(&settings, cx);
    });
    cx.run_until_parked();
    assert_eq!(painted(&view, cx), None);
    assert!(!view.read_with(cx, |view, _| view.completion_shown()));
}

#[gpui::test]
fn menu_closes_when_its_session_ends(cx: &mut TestAppContext) {
    let (view, _, cx) = editor(cx);
    let shown = |cx: &mut VisualTestContext| painted(&view, cx).is_some();

    // A jump moves the caret out of the word: `enter` is a line break,
    // not an accept.
    keys("ctrl-space", cx);
    assert!(shown(cx));
    view.update(cx, |view, cx| view.select_and_reveal(0..0, cx));
    cx.run_until_parked();
    assert!(!shown(cx));
    keys("enter", cx);
    assert_eq!(text(&view, cx), format!("\n{TEXT}"));
    keys("cmd-z", cx);

    // Keys go by the menu as it is when they come, though no frame has
    // been drawn since it closed: the caret moved, so `enter` is a line
    // break.
    let va = TEXT.find("va\n").unwrap() + 2;
    view.update(cx, |view, cx| view.select_and_reveal(va..va, cx));
    keys("ctrl-space", cx);
    assert!(shown(cx));
    cx.update(|window, cx| {
        view.update(cx, |view, _| view.model_mut().set_cursor(0));
        window.dispatch_keystroke(gpui::Keystroke::parse("enter").unwrap(), cx);
    });
    assert_eq!(text(&view, cx), format!("\n{TEXT}"));
    keys("cmd-z", cx);

    // `escape` drops a request not answered yet.
    view.update(cx, |view, cx| view.select_and_reveal(va..va, cx));
    cx.simulate_keystrokes("ctrl-space escape");
    cx.run_until_parked();
    assert!(!shown(cx));

    // Losing focus closes it.
    keys("ctrl-space", cx);
    assert!(shown(cx));
    cx.update(|window, cx| {
        window.focus(&cx.focus_handle(), cx);
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    assert!(!shown(cx));
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));

    // In vim: `escape` closes the menu and stays in insert mode; leaving
    // insert mode closes it, and `down` is vim's again.
    cx.update(|_, cx| set_vim_enabled(cx, true));
    keys("a", cx);
    let mode = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.vim_mode());
    assert_eq!(mode(cx), Some("insert"));
    keys("ctrl-space", cx);
    assert!(shown(cx));
    keys("escape", cx);
    assert!(!shown(cx));
    assert_eq!(mode(cx), Some("insert"));
    keys("ctrl-space", cx);
    assert!(shown(cx));
    keys("ctrl-[", cx);
    assert!(!shown(cx));
    assert_eq!(mode(cx), Some("normal"));
    let row = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let model = view.model();
            model
                .text()
                .offset_to_point(model.primary_selection().head())
                .row
        })
    };
    keys("down", cx);
    assert_eq!(row(cx), 2);
    assert_eq!(text(&view, cx), TEXT);
    // Even with no frame drawn between leaving insert mode and `down`.
    view.update(cx, |view, cx| view.select_and_reveal(va..va, cx));
    keys("i ctrl-space", cx);
    assert!(shown(cx));
    cx.update(|window, cx| {
        for key in ["ctrl-[", "down"] {
            window.dispatch_keystroke(gpui::Keystroke::parse(key).unwrap(), cx);
        }
    });
    assert_eq!(mode(cx), Some("normal"));
    assert_eq!(text(&view, cx), TEXT);
    assert_eq!(row(cx), 2);
}

#[gpui::test]
fn asking_again(cx: &mut TestAppContext) {
    let (view, provider, cx) = editor(cx);
    let va = TEXT.find("va\n").unwrap() + 2;
    let shown = |cx: &mut VisualTestContext| painted(&view, cx).is_some();
    let delay = Duration::from_millis(100);
    *provider.delay.borrow_mut() = delay;
    *provider.incomplete.borrow_mut() = true;

    // Typed into while the first answer is awaited, an incomplete list
    // asks again at once.
    keys("ctrl-space", cx);
    cx.simulate_input("l");
    cx.run_until_parked();
    assert!(!shown(cx));
    cx.executor().advance_clock(delay);
    cx.run_until_parked();
    assert!(shown(cx));
    assert_eq!(
        *provider.calls.borrow(),
        [
            (va, CompletionTrigger::Invoked),
            (va + 1, CompletionTrigger::Incomplete)
        ]
    );
    cx.executor().advance_clock(delay);
    cx.run_until_parked();
    assert!(shown(cx));
    // An answer of nothing (a timeout, say) to asking again closes it.
    *provider.nothing.borrow_mut() = true;
    cx.simulate_input("u");
    cx.run_until_parked();
    assert_eq!(provider.calls.borrow().len(), 3);
    assert!(shown(cx));
    cx.executor().advance_clock(delay);
    cx.run_until_parked();
    assert!(!shown(cx));

    // Cutting back the word asked about asks again where the caret is.
    *provider.nothing.borrow_mut() = false;
    *provider.incomplete.borrow_mut() = false;
    *provider.delay.borrow_mut() = Duration::ZERO;
    keys("ctrl-space", cx);
    assert!(shown(cx));
    provider.calls.borrow_mut().clear();
    keys("backspace", cx);
    assert_eq!(text(&view, cx), "fn main() {\n    val\n}\n");
    assert_eq!(
        *provider.calls.borrow(),
        [(va + 1, CompletionTrigger::Invoked)]
    );
    assert_eq!(rows(&view, cx).0, ["value", "values"]);
}
