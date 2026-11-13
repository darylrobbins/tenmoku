// SPDX-License-Identifier: Apache-2.0

//! The picker's vim keys, telescope's model: with the host's switch on,
//! escape goes from insert mode to a normal mode where `j`/`k` move,
//! `g g`/`G` jump, enter accepts, escape or `q` close and `i`/`a`/`I`/`A`
//! type again. With the switch off, nothing changes. Commands run as
//! [`run`] dispatches them; letters as keystrokes no binding matched.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{
    AnyElement, App, Context, DismissEvent, Entity, IntoElement, ParentElement as _, SharedString,
    TestAppContext, VisualTestContext, Window, div,
};
use kiln_command::Invocation;
use kiln_platform::{commands::RunCommand, keys};
use kiln_shell::{
    Picker, PickerDelegate,
    picker::{self, KEY_CONTEXT, Mode, VIM_MODE},
};
use kiln_ui::UiTheme;

/// Picks one of `words` containing the query.
struct Words {
    words: Vec<&'static str>,
    matches: Vec<&'static str>,
    picked: Rc<RefCell<Vec<&'static str>>>,
}

impl PickerDelegate for Words {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.matches = self
            .words
            .iter()
            .copied()
            .filter(|word| word.contains(&query))
            .collect();
        cx.notify();
        let picker = cx.entity();
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        self.matches[ix].into()
    }

    fn render_match(&self, ix: usize, _: bool, _: &App) -> AnyElement {
        div().child(self.matches[ix]).into_any_element()
    }

    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        self.matches[ix].into()
    }

    fn confirm(&mut self, ix: usize, _: &mut Window, _: &mut Context<Picker<Self>>) -> bool {
        self.picked.borrow_mut().push(self.matches[ix]);
        true
    }

    fn dismissed(&mut self, _: &mut Window, _: &mut Context<Picker<Self>>) {}
}

const WORDS: [&str; 5] = ["apple", "apricot", "banana", "cherry", "grape"];

thread_local! {
    /// The switch the tests install, as [`switch`] reads it: one per
    /// thread, so one per test even in `cargo test`'s one process.
    static VIM: Cell<bool> = const { Cell::new(false) };
}

fn switch(_: &App) -> bool {
    VIM.with(Cell::get)
}

fn set_vim(on: bool, cx: &mut VisualTestContext) {
    VIM.with(|vim| vim.set(on));
    // The picker reads it live; a redraw isn't owed.
    cx.run_until_parked();
}

/// A picker, what it picked, how many dismiss events it has emitted, and
/// its window.
type Opened<'a> = (
    Entity<Picker<Words>>,
    Rc<RefCell<Vec<&'static str>>>,
    Rc<RefCell<usize>>,
    &'a mut VisualTestContext,
);

/// A window of a picker of [`WORDS`], shown and drawn, with the vim switch
/// installed and `vim`, or none installed.
fn open(cx: &mut TestAppContext, vim: Option<bool>) -> Opened<'_> {
    cx.update(kiln_shell::register);
    VIM.with(|switch| switch.set(vim.unwrap_or(false)));
    if vim.is_some() {
        cx.update(|cx| picker::set_vim_keys(cx, switch));
    }
    let picked = Rc::new(RefCell::new(Vec::new()));
    let words = Words {
        words: WORDS.to_vec(),
        matches: Vec::new(),
        picked: picked.clone(),
    };
    let (picker, cx) =
        cx.add_window_view(|_, cx| Picker::new(words, kiln_l10n::Text::verbatim("Find a word"), UiTheme::default_dark(), cx));
    let events = Rc::new(RefCell::new(0));
    cx.update(|window, cx| {
        let events = events.clone();
        cx.subscribe(&picker, move |_, _: &DismissEvent, _| {
            *events.borrow_mut() += 1
        })
        .detach();
        picker.update(cx, |picker, cx| picker.show(window, cx));
    });
    draw(cx);
    (picker, picked, events, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Shows the picker again.
fn show(picker: &Entity<Picker<Words>>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| picker.update(cx, |picker, cx| picker.show(window, cx)));
    draw(cx);
}

fn matches(picker: &Entity<Picker<Words>>, cx: &mut VisualTestContext) -> Vec<&'static str> {
    picker.read_with(cx, |picker, _| picker.delegate.matches.clone())
}

fn selected(picker: &Entity<Picker<Words>>, cx: &mut VisualTestContext) -> usize {
    picker.read_with(cx, |picker, _| picker.selected())
}

fn query(picker: &Entity<Picker<Words>>, cx: &mut VisualTestContext) -> String {
    picker.read_with(cx, |picker, cx| picker.input().read(cx).text().to_owned())
}

fn mode(picker: &Entity<Picker<Words>>, cx: &mut VisualTestContext) -> Mode {
    picker.read_with(cx, |picker, _| picker.mode())
}

/// The picker's key context's `vim_mode`, as last drawn.
fn vim_mode(cx: &mut VisualTestContext) -> Option<String> {
    draw(cx);
    cx.update(|window, _| {
        let stack = window.context_stack();
        let picker = stack
            .iter()
            .find(|context| context.contains(KEY_CONTEXT))
            .expect("the picker's context is on the focus path");
        picker.get(VIM_MODE).map(ToString::to_string)
    })
}

/// Runs `command` as a key would: on the focused element, in the context
/// its key contexts make.
fn run(command: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let context = keys::context_stack(&window.context_stack(), cx);
        let run = RunCommand(Invocation::of(command).unwrap().context(context));
        window.dispatch_action(Box::new(run), cx);
    });
    draw(cx);
}

/// Presses `keys`, which no binding matches, as the picker's key listener
/// gets them.
fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

#[gpui::test]
fn normal_mode_moves_and_accepts(cx: &mut TestAppContext) {
    let (picker, picked, events, cx) = open(cx, Some(true));
    cx.simulate_input("ap");
    assert_eq!(matches(&picker, cx), ["apple", "apricot", "grape"]);

    // Escape goes to normal mode, the picker open and the query kept.
    run("picker.dismiss", cx);
    assert_eq!(*events.borrow(), 0, "not dismissed");
    assert_eq!(mode(&picker, cx), Mode::Normal);
    assert_eq!(query(&picker, cx), "ap");
    assert_eq!(selected(&picker, cx), 0);

    // j and k move, clamped at the ends.
    keys("j", cx);
    assert_eq!(selected(&picker, cx), 1);
    keys("j j", cx);
    assert_eq!(selected(&picker, cx), 2, "stays on the last");
    keys("k", cx);
    assert_eq!(selected(&picker, cx), 1);
    keys("k k", cx);
    assert_eq!(selected(&picker, cx), 0, "stays on the first");

    // G the last, g g the first; a g then something else is dropped.
    keys("shift-g", cx);
    assert_eq!(selected(&picker, cx), 2);
    keys("g", cx);
    assert_eq!(selected(&picker, cx), 2, "a g waits");
    keys("g", cx);
    assert_eq!(selected(&picker, cx), 0);
    keys("j g x g", cx);
    assert_eq!(selected(&picker, cx), 1, "g x dropped, then a g waiting");
    keys("k", cx);
    assert_eq!(selected(&picker, cx), 0, "the g was dropped for k");
    keys("j", cx);
    assert_eq!(mode(&picker, cx), Mode::Normal);
    assert_eq!(query(&picker, cx), "ap");

    // Enter accepts the selected match.
    run("picker.confirm", cx);
    assert_eq!(*picked.borrow(), ["apricot"]);
    assert_eq!(*events.borrow(), 1);

    // Shown afresh: escape then q dismisses.
    show(&picker, cx);
    run("picker.dismiss", cx);
    assert_eq!(*events.borrow(), 1);
    keys("q", cx);
    assert_eq!(*events.borrow(), 2, "q dismissed");

    // And so does escape twice.
    show(&picker, cx);
    run("picker.dismiss", cx);
    assert_eq!(*events.borrow(), 2);
    run("picker.dismiss", cx);
    assert_eq!(*events.borrow(), 3, "escape dismissed");

    // An unbound escape, or ctrl-[, does as picker.dismiss does.
    show(&picker, cx);
    keys("ctrl-[", cx);
    assert_eq!(mode(&picker, cx), Mode::Normal);
    assert_eq!(*events.borrow(), 3);
    keys("escape", cx);
    assert_eq!(*events.borrow(), 4);
    assert_eq!(*picked.borrow(), ["apricot"], "nothing more picked");
}

#[gpui::test]
fn normal_mode_types_nothing(cx: &mut TestAppContext) {
    let (picker, _, events, cx) = open(cx, Some(true));
    assert_eq!(vim_mode(cx).as_deref(), Some("insert"));
    cx.simulate_input("ap");
    run("picker.dismiss", cx);
    assert_eq!(vim_mode(cx).as_deref(), Some("normal"));
    let typing = |cx: &mut VisualTestContext| {
        picker.read_with(cx, |picker, cx| picker.input().read(cx).typing())
    };
    assert!(!typing(cx), "the input takes no text");

    // Neither an unbound letter nor the input's commands edit the query.
    keys("x", cx);
    keys("j", cx);
    run("text_input.backspace", cx);
    run("text_input.delete_word_left", cx);
    run("text_input.clear", cx);
    assert_eq!(query(&picker, cx), "ap");
    assert_eq!(matches(&picker, cx), ["apple", "apricot", "grape"]);
    assert_eq!(selected(&picker, cx), 1, "j moved");

    // i, a, I and A each go back to insert mode, typing at the end.
    for (key, typed, expected) in [
        ("i", "r", "apr"),
        ("a", "i", "apri"),
        ("shift-i", "c", "apric"),
        ("shift-a", "o", "aprico"),
    ] {
        assert_eq!(mode(&picker, cx), Mode::Normal, "{key}");
        keys(key, cx);
        assert_eq!(mode(&picker, cx), Mode::Insert, "{key}");
        assert_eq!(vim_mode(cx).as_deref(), Some("insert"), "{key}");
        assert_eq!(query(&picker, cx), &expected[..expected.len() - 1], "{key}");
        cx.simulate_input(typed);
        draw(cx);
        assert_eq!(query(&picker, cx), expected, "{key}");
        run("picker.dismiss", cx);
    }
    assert_eq!(matches(&picker, cx), ["apricot"]);
    // The input's commands edit again.
    keys("a", cx);
    assert!(typing(cx));
    run("text_input.backspace", cx);
    assert_eq!(query(&picker, cx), "apric");
    assert_eq!(*events.borrow(), 0);

    // Shown again, it's in insert mode.
    run("picker.dismiss", cx);
    assert_eq!(mode(&picker, cx), Mode::Normal);
    show(&picker, cx);
    assert_eq!(mode(&picker, cx), Mode::Insert);
    assert_eq!(vim_mode(cx).as_deref(), Some("insert"));
    cx.simulate_input("j");
    assert_eq!(query(&picker, cx), "j");

    // With the switch off, no vim mode is in the key context.
    set_vim(false, cx);
    assert_eq!(vim_mode(cx), None);
}

#[gpui::test]
fn switch_off_is_unchanged(cx: &mut TestAppContext) {
    // No switch installed: escape dismisses at once, and j types.
    let (picker, _, events, cx) = open(cx, None);
    assert!(!cx.update(|_, cx| picker::vim_keys(cx)));
    keys("j", cx);
    assert_eq!(query(&picker, cx), "j");
    assert_eq!(vim_mode(cx), None);
    run("picker.dismiss", cx);
    assert_eq!(*events.borrow(), 1);
    assert_eq!(mode(&picker, cx), Mode::Insert);

    // A switch that returns false: the same.
    cx.update(|_, cx| picker::set_vim_keys(cx, switch));
    assert!(!cx.update(|_, cx| picker::vim_keys(cx)));
    show(&picker, cx);
    keys("j", cx);
    assert_eq!(query(&picker, cx), "j");
    assert_eq!(vim_mode(cx), None);
    run("picker.dismiss", cx);
    assert_eq!(*events.borrow(), 2);

    // Turned off in normal mode: the next key types into the query.
    show(&picker, cx);
    set_vim(true, cx);
    assert!(cx.update(|_, cx| picker::vim_keys(cx)));
    cx.simulate_input("ap");
    run("picker.dismiss", cx);
    assert_eq!(mode(&picker, cx), Mode::Normal);
    set_vim(false, cx);
    cx.simulate_keystrokes("j");
    assert_eq!(query(&picker, cx), "apj");
    assert_eq!(mode(&picker, cx), Mode::Insert);
    // And from a redraw: the next frame is in insert mode.
    set_vim(true, cx);
    run("picker.dismiss", cx);
    assert_eq!(mode(&picker, cx), Mode::Normal);
    set_vim(false, cx);
    cx.update(|window, _| window.refresh());
    draw(cx);
    assert_eq!(mode(&picker, cx), Mode::Insert);
    cx.simulate_input("x");
    assert_eq!(query(&picker, cx), "apjx");
    run("picker.dismiss", cx);
    assert_eq!(*events.borrow(), 3, "escape dismissed");
}
