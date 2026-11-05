// SPDX-License-Identifier: Apache-2.0

//! The shell's setup: prompts follow the platform.

use gpui::{Context, IntoElement, PromptButton, PromptLevel, Render, TestAppContext, Window, div};
use kiln_platform::Capabilities;

struct Empty;

impl Render for Empty {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Where there are no native alerts, `window.prompt` shows kiln-ui's
/// in-window prompt, which Escape answers with its cancel button.
#[gpui::test]
fn prompts_follow_the_platform(cx: &mut TestAppContext) {
    if Capabilities::current().native_prompts {
        return;
    }
    cx.update(kiln_shell::init);
    let (_, cx) = cx.add_window_view(|_, _| Empty);
    let buttons = [PromptButton::ok("OK"), PromptButton::cancel("Cancel")];
    let mut answer =
        cx.update(|window, cx| window.prompt(PromptLevel::Info, "Save?", None, &buttons, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(answer.try_recv().ok().flatten(), Some(1));
}
