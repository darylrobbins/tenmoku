// SPDX-License-Identifier: LicenseRef-Proprietary

//! The picker's pending `g` prefix: anything but a second `g` drops it,
//! whether a keystroke with no character, a modified one, or a command that
//! goes on to the host.

use std::cell::Cell;

use gpui::{
    AnyElement, App, Context, Entity, IntoElement as _, ParentElement as _, SharedString,
    TestAppContext, VisualTestContext, Window, div,
};
use kiln_command::Invocation;
use kiln_platform::{commands::RunCommand, keys};
use kiln_shell::{Picker, PickerDelegate, picker};
use kiln_ui::UiTheme;

/// Picks one of `words`.
struct Words {
    words: Vec<&'static str>,
}

impl PickerDelegate for Words {
    fn update_matches(&mut self, _: String, cx: &mut Context<Picker<Self>>) {
        cx.notify();
        let picker = cx.entity();
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        self.words.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        self.words[ix].into()
    }

    fn render_match(&self, ix: usize, _: bool, _: &App) -> AnyElement {
        div().child(self.words[ix]).into_any_element()
    }

    fn confirm(&mut self, _: usize, _: &mut Window, _: &mut Context<Picker<Self>>) -> bool {
        true
    }

    fn dismissed(&mut self, _: &mut Window, _: &mut Context<Picker<Self>>) {}
}

const WORDS: [&str; 4] = ["apple", "banana", "cherry", "grape"];

thread_local! {
    /// One per thread, so one per test even in `cargo test`'s one process.
    static VIM: Cell<bool> = const { Cell::new(true) };
}

fn switch(_: &App) -> bool {
    VIM.with(Cell::get)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn selected(picker: &Entity<Picker<Words>>, cx: &mut VisualTestContext) -> usize {
    picker.read_with(cx, |picker, _| picker.selected())
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

/// Runs `command` as a key would, on the focused element.
fn run(command: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let context = keys::context_stack(&window.context_stack(), cx);
        let run = RunCommand(Invocation::of(command).unwrap().context(context));
        window.dispatch_action(Box::new(run), cx);
    });
    draw(cx);
}

#[gpui::test]
fn interrupted_g_prefix_is_dropped(cx: &mut TestAppContext) {
    cx.update(kiln_shell::register);
    VIM.with(|vim| vim.set(true));
    cx.update(|cx| picker::set_vim_keys(cx, switch));
    let words = Words {
        words: WORDS.to_vec(),
    };
    let (picker, cx) =
        cx.add_window_view(|_, cx| Picker::new(words, "Find", UiTheme::default_dark(), cx));
    cx.update(|window, cx| picker.update(cx, |picker, cx| picker.show(window, cx)));
    draw(cx);

    run("picker.dismiss", cx);
    let last = WORDS.len() - 1;
    let to_last = |cx: &mut VisualTestContext| {
        keys("shift-g", cx);
        assert_eq!(selected(&picker, cx), last);
    };
    to_last(cx);

    keys("g left g", cx);
    assert_eq!(selected(&picker, cx), last, "g left g");
    to_last(cx);

    keys("g ctrl-x g", cx);
    assert_eq!(selected(&picker, cx), last, "g ctrl-x g");
    to_last(cx);

    keys("g", cx);
    run("editor.move_left", cx);
    keys("g", cx);
    assert_eq!(selected(&picker, cx), last, "g, a host command, g");
    to_last(cx);

    keys("g g", cx);
    assert_eq!(selected(&picker, cx), 0, "g g");
}
