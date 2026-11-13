// SPDX-License-Identifier: Apache-2.0

//! `window.prompt(..)`, plain GPUI: native alerts on macOS, kiln-ui's
//! in-window prompt elsewhere (`kiln::shell::init` installs it).

use gpui::{
    ClickEvent, Context, IntoElement, ParentElement as _, PromptLevel, Render, Styled as _, Window,
    div,
};
use kiln::{
    prelude::*,
    ui::{Button, h_flex},
};
use kiln_l10n::Text;

use super::{page_frame, page_root, section};

const ANSWERS: [&str; 3] = ["Save", "Don't Save", "Cancel"];

#[derive(Default)]
pub(crate) struct PromptsPage {
    answer: Option<String>,
}

impl PromptsPage {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Asks a question at `level`, and shows the answer once it comes.
    pub(crate) fn ask(&mut self, level: PromptLevel, window: &mut Window, cx: &mut Context<Self>) {
        let answer = window.prompt(
            level,
            "Save changes to “notes.md” before closing?",
            Some("Your changes will be lost if you don't save them."),
            &ANSWERS,
            cx,
        );
        cx.spawn_in(window, async move |page, cx| {
            let answer = answer.await.ok();
            page.update(cx, |page, cx| {
                page.answer = Some(match answer {
                    Some(ix) => format!("Answered “{}”", ANSWERS[ix]),
                    None => "Dismissed".to_owned(),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}

impl Render for PromptsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.theme().colors;
        let levels = [
            (PromptLevel::Info, "Info"),
            (PromptLevel::Warning, "Warning"),
            (PromptLevel::Critical, "Critical"),
        ];
        let buttons =
            h_flex().gap_2().children(levels.map(|(level, label)| {
                Button::new(label, Text::verbatim(format!("Ask ({label})")))
                    .primary()
                    .on_click(cx.listener(move |page, _: &ClickEvent, window, cx| {
                        page.ask(level, window, cx)
                    }))
            }));
        let answer = self
            .answer
            .clone()
            .unwrap_or_else(|| "No answer yet".to_owned());
        page_root("prompts-page", cx).child(
            page_frame(
                "Prompts",
                "window.prompt is GPUI's own; the Show Prompt command asks the Info one.",
                cx,
            )
            .child(section("WINDOW.PROMPT", buttons, cx))
            .child(div().text_color(c.text.muted).child(answer)),
        )
    }
}
