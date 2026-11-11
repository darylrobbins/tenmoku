// SPDX-License-Identifier: Apache-2.0

//! The picker over a delegate: the input's text matched as it changes, a
//! selection the picker's commands move, confirm and dismiss, a click
//! picking the match its row showed, and the input's editing commands.
//! Generalised from the editor's quick open tests.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    AnyElement, App, Context, DismissEvent, Entity, Focusable as _, IntoElement, Modifiers,
    ParentElement as _, SharedString, TestAppContext, VisualTestContext, Window, div, point, px,
};
use kiln_command::Invocation;
use kiln_platform::{commands::RunCommand, keys};
use kiln_shell::{
    Picker, PickerDelegate,
    picker::{self, PickerOp, VISIBLE_ROWS},
};
use kiln_ui::{TextInput, UiTheme};

/// What happened to a picker of words.
#[derive(Debug, Default, PartialEq)]
struct Log {
    picked: Vec<String>,
    dismissed: usize,
}

/// Picks one of `words` containing the query.
struct Words {
    words: Vec<&'static str>,
    matches: Vec<&'static str>,
    log: Rc<RefCell<Log>>,
    /// Refuses picks of these, staying open.
    refuse: Vec<&'static str>,
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
        // The picker is being updated now.
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
        let word = self.matches[ix];
        if self.refuse.contains(&word) {
            return false;
        }
        self.log.borrow_mut().picked.push(word.to_owned());
        true
    }

    fn dismissed(&mut self, _: &mut Window, _: &mut Context<Picker<Self>>) {
        self.log.borrow_mut().dismissed += 1;
    }

    fn status(&self) -> Option<SharedString> {
        Some(format!("{} of {}", self.matches.len(), self.words.len()).into())
    }
}

const WORDS: [&str; 5] = ["apple", "apricot", "banana", "cherry", "grape"];

/// A picker, what it did, how many dismiss events it has emitted, and its
/// window.
type Opened<'a> = (
    Entity<Picker<Words>>,
    Rc<RefCell<Log>>,
    Rc<RefCell<usize>>,
    &'a mut VisualTestContext,
);

/// A window of a picker of [`WORDS`], shown.
fn open<'a>(cx: &'a mut TestAppContext, refuse: Vec<&'static str>) -> Opened<'a> {
    cx.update(kiln_shell::register);
    let log = Rc::new(RefCell::new(Log::default()));
    let words = Words {
        words: WORDS.to_vec(),
        matches: Vec::new(),
        log: log.clone(),
        refuse,
    };
    let (picker, cx) =
        cx.add_window_view(|_, cx| Picker::new(words, "Find a word", UiTheme::default_dark(), cx));
    let events = Rc::new(RefCell::new(0));
    cx.update(|window, cx| {
        let events = events.clone();
        cx.subscribe(&picker, move |_, _: &DismissEvent, _| {
            *events.borrow_mut() += 1
        })
        .detach();
        picker.update(cx, |picker, cx| picker.show(window, cx));
    });
    cx.run_until_parked();
    (picker, log, events, cx)
}

fn matches(picker: &Entity<Picker<Words>>, cx: &mut VisualTestContext) -> Vec<&'static str> {
    picker.read_with(cx, |picker, _| picker.delegate.matches.clone())
}

fn selected(picker: &Entity<Picker<Words>>, cx: &mut VisualTestContext) -> usize {
    picker.read_with(cx, |picker, _| picker.selected())
}

/// Runs `command` as a key would: on the focused element, in the context
/// its key contexts make.
fn run(command: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let context = keys::context_stack(&window.context_stack(), cx);
        let run = RunCommand(Invocation::of(command).unwrap().context(context));
        window.dispatch_action(Box::new(run), cx);
    });
    cx.run_until_parked();
}

#[gpui::test]
fn typing_matches_and_the_keys_move_the_selection(cx: &mut TestAppContext) {
    let (picker, log, events, cx) = open(cx, Vec::new());
    assert!(cx.update(|window, cx| picker.focus_handle(cx).is_focused(window)));
    assert_eq!(matches(&picker, cx), WORDS);
    let status = picker.read_with(cx, |picker, _| picker.delegate.status());
    assert_eq!(status.as_deref(), Some("5 of 5"));

    cx.simulate_input("ap");
    assert_eq!(matches(&picker, cx), ["apple", "apricot", "grape"]);
    assert_eq!(selected(&picker, cx), 0);
    run("picker.select_next", cx);
    run("picker.select_next", cx);
    assert_eq!(selected(&picker, cx), 2);
    run("picker.select_next", cx);
    assert_eq!(selected(&picker, cx), 2, "stays on the last");
    run("picker.select_prev", cx);
    assert_eq!(selected(&picker, cx), 1);

    // A new query selects the first again.
    cx.simulate_input("r");
    assert_eq!(matches(&picker, cx), ["apricot"]);
    assert_eq!(selected(&picker, cx), 0);

    run("picker.confirm", cx);
    assert_eq!(log.borrow().picked, ["apricot"]);
    assert_eq!(*events.borrow(), 1, "dismissed for its host to hide");
}

#[gpui::test]
fn dismiss_and_a_refused_pick(cx: &mut TestAppContext) {
    let (picker, log, events, cx) = open(cx, vec!["banana"]);
    cx.simulate_input("an");
    assert_eq!(matches(&picker, cx), ["banana"]);
    run("picker.confirm", cx);
    assert_eq!(*log.borrow(), Log::default(), "refused");
    assert_eq!(*events.borrow(), 0, "stays open");

    run("picker.dismiss", cx);
    assert_eq!(log.borrow().dismissed, 1);
    assert_eq!(*events.borrow(), 1);

    // Shown again, it starts afresh.
    cx.update(|window, cx| picker.update(cx, |picker, cx| picker.show(window, cx)));
    cx.run_until_parked();
    let text = picker.read_with(cx, |picker, cx| picker.input().read(cx).text().to_owned());
    assert_eq!(text, "");
    assert_eq!(matches(&picker, cx), WORDS);
}

#[gpui::test]
fn the_inputs_commands_edit_it(cx: &mut TestAppContext) {
    let (picker, _, _, cx) = open(cx, Vec::new());
    let text = |cx: &mut VisualTestContext| {
        picker.read_with(cx, |picker, cx| picker.input().read(cx).text().to_owned())
    };
    cx.simulate_input("red apple");
    run("text_input.backspace", cx);
    assert_eq!(text(cx), "red appl");
    run("text_input.delete_word_left", cx);
    assert_eq!(text(cx), "red ");
    run("text_input.clear", cx);
    assert_eq!(text(cx), "");
    assert_eq!(matches(&picker, cx), WORDS, "matched as it changes");

    // Other views with an input run them through `op`.
    let op = cx.update(|_, cx| {
        let run = RunCommand(
            Invocation::of("text_input.clear").unwrap().context(
                [kiln_command::ContextFrame::new(TextInput::KEY_CONTEXT)]
                    .into_iter()
                    .collect(),
            ),
        );
        picker::op(&run, cx)
    });
    assert_eq!(op, Some(PickerOp::Clear));
    assert!(op.unwrap().edit().is_some());
    assert!(PickerOp::Confirm.edit().is_none());
    // Not in its context, it doesn't apply.
    let op =
        cx.update(|_, cx| picker::op(&RunCommand(Invocation::of("picker.confirm").unwrap()), cx));
    assert_eq!(op, None);
}

#[gpui::test]
fn a_click_picks_the_row_clicked(cx: &mut TestAppContext) {
    let (picker, log, events, cx) = open(cx, Vec::new());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (list, row) = picker.read_with(cx, |picker, _| {
        (picker.list_bounds(), picker.row_bounds(2).expect("drawn"))
    });
    assert!(list.size.height > row.size.height * 4.9, "{list:?} {row:?}");
    assert!(WORDS.len() < VISIBLE_ROWS, "every row shows");
    // Near its start: a row is only as wide as its text.
    let at = row.origin + point(px(4.), row.size.height / 2.);
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(log.borrow().picked, ["banana"]);
    assert_eq!(*events.borrow(), 1);
}

#[test]
fn the_catalog_knows_the_commands() {
    let catalog = kiln_shell::catalog();
    for command in [
        "menu_bar.open",
        "picker.select_prev",
        "picker.select_next",
        "picker.confirm",
        "picker.dismiss",
        "text_input.backspace",
        "text_input.delete_word_left",
        "text_input.clear",
        "text_input.paste",
    ] {
        let id = kiln_command::CommandId::parse(command).unwrap();
        assert_eq!(
            catalog.check(&id, &kiln_command::Value::Null),
            Ok(()),
            "{command}"
        );
    }
}
