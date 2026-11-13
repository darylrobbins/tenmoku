// SPDX-License-Identifier: Apache-2.0

//! `Button::command`: clicking dispatches the button's action from the
//! focused element, a typed command as an invocation from the mouse.

use kiln_l10n::Text;
use std::{cell::RefCell, rc::Rc};

use gpui::{
    Context, FocusHandle, InteractiveElement as _, IntoElement, Modifiers, ParentElement as _,
    Render, TestAppContext, VisualTestContext, Window, div,
};
use kiln_command::{Invocation, Source};
use kiln_platform::{
    command_actions::current_invocation,
    commands::{self, observe_invocations},
};

use crate::Button;

mod pad {
    kiln_platform::commands!(
        pad,
        [
            /// Bold
            Bold,
        ]
    );
}

gpui::actions!(pad_plain, [Plain]);

/// What the view's handlers saw: each action, with the invocation it ran
/// under.
type Log = Rc<RefCell<Vec<(&'static str, Option<Invocation>)>>>;

struct Toolbar {
    focus: FocusHandle,
    log: Log,
}

impl Render for Toolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &pad::Bold, _, cx| {
                this.log.borrow_mut().push(("bold", current_invocation(cx)));
            }))
            .on_action(cx.listener(|this, _: &Plain, _, cx| {
                this.log
                    .borrow_mut()
                    .push(("plain", current_invocation(cx)));
            }))
            .child(
                Button::new("bold", Text::verbatim("Bold"))
                    .debug_selector(|| "bold".into())
                    .command(pad::Bold),
            )
            .child(
                Button::new("plain", Text::verbatim("Plain"))
                    .debug_selector(|| "plain".into())
                    .command(Plain),
            )
            .child(
                Button::new("off", Text::verbatim("Off"))
                    .disabled(true)
                    .debug_selector(|| "off".into())
                    .command(Plain),
            )
    }
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn a_command_button_dispatches_its_action(cx: &mut TestAppContext) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        commands::init(cx, "Pad");
        pad::register(cx);
        let sink = seen.clone();
        observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        });
    });
    let log = Log::default();
    let (view, cx) = cx.add_window_view({
        let log = log.clone();
        move |_, cx| Toolbar {
            focus: cx.focus_handle(),
            log,
        }
    });
    cx.update(|window, cx| {
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    cx.run_until_parked();

    click("bold", cx);
    click("plain", cx);
    click("off", cx);
    let log = log.borrow();
    let handled: Vec<_> = log.iter().map(|(what, _)| *what).collect();
    assert_eq!(handled, ["bold", "plain"], "the disabled one runs nothing");
    let bold = log[0]
        .1
        .as_ref()
        .expect("a typed command runs as an invocation");
    assert_eq!(bold.command.as_str(), "pad.bold");
    assert_eq!(bold.source, Source::Mouse);
    assert_eq!(log[1].1, None, "a plain action runs as itself");
    let seen: Vec<_> = seen
        .borrow()
        .iter()
        .map(|i| i.command.to_string())
        .collect();
    assert_eq!(seen, ["pad.bold"], "observers hear of the command");
}
