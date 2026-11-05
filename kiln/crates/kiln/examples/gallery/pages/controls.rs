// SPDX-License-Identifier: Apache-2.0

//! Buttons in every variant and size, disabled ones, checkboxes and
//! switches. The values are the page's: controls show what they're given
//! and report clicks.

use gpui::{
    ClickEvent, Context, Div, ElementId, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window, div,
};
use kiln::{
    prelude::*,
    ui::{Button, ButtonSize, ButtonVariant, Checkbox, Icon, Switch, h_flex, v_flex},
};

use super::{labelled, page_frame, page_root, section};

const VARIANTS: [(ButtonVariant, &str); 4] = [
    (ButtonVariant::Primary, "Primary"),
    (ButtonVariant::Secondary, "Secondary"),
    (ButtonVariant::Ghost, "Ghost"),
    (ButtonVariant::Danger, "Danger"),
];

const SIZES: [(ButtonSize, &str); 3] = [
    (ButtonSize::Small, "Small"),
    (ButtonSize::Medium, "Medium"),
    (ButtonSize::Large, "Large"),
];

#[derive(Default)]
pub(crate) struct ControlsPage {
    clicks: usize,
    last_clicked: Option<SharedString>,
    word_wrap: bool,
    line_numbers: bool,
    autosave: bool,
    telemetry: bool,
}

impl ControlsPage {
    pub(crate) fn new() -> Self {
        Self {
            line_numbers: true,
            autosave: true,
            ..Self::default()
        }
    }

    fn clicked(&mut self, what: SharedString, cx: &mut Context<Self>) {
        self.clicks += 1;
        self.last_clicked = Some(what);
        cx.notify();
    }

    fn button(
        &self,
        variant: (ButtonVariant, &str),
        size: (ButtonSize, &str),
        cx: &mut Context<Self>,
    ) -> Button {
        let what: SharedString = format!("{} {}", variant.1, size.1).into();
        let id = ElementId::from(what.clone());
        Button::new(id, size.1)
            .variant(variant.0)
            .size(size.0)
            .on_click(
                cx.listener(move |page, _: &ClickEvent, _, cx| page.clicked(what.clone(), cx)),
            )
    }

    fn buttons(&self, cx: &mut Context<Self>) -> Div {
        let rows = VARIANTS.map(|variant| {
            let sizes = SIZES.map(|size| self.button(variant, size, cx));
            let name = variant.1;
            let row = h_flex()
                .gap_2()
                .items_center()
                .children(sizes)
                .child(
                    Button::new(SharedString::from(format!("{name} disabled")), "Disabled")
                        .variant(variant.0)
                        .disabled(true),
                )
                .child(
                    Button::icon_only(SharedString::from(format!("{name} icon")), Icon::Search)
                        .variant(variant.0)
                        .on_click(cx.listener(move |page, _: &ClickEvent, _, cx| {
                            page.clicked(format!("{name} icon").into(), cx)
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("{name} labelled icon")), "Apply")
                        .icon(Icon::Check)
                        .variant(variant.0)
                        .on_click(cx.listener(move |page, _: &ClickEvent, _, cx| {
                            page.clicked(format!("{name} Apply").into(), cx)
                        })),
                );
            labelled(name, row, cx)
        });
        let c = cx.theme().colors;
        let status = match &self.last_clicked {
            Some(what) => format!("Clicked {} times; last: {what}", self.clicks),
            None => "Click a button".to_owned(),
        };
        v_flex()
            .gap_2()
            .children(rows)
            .child(div().text_color(c.text.muted).child(status))
    }

    fn toggles(&self, cx: &mut Context<Self>) -> Div {
        let checkboxes = v_flex()
            .gap_2()
            .child(
                Checkbox::new("word-wrap")
                    .label("Word wrap")
                    .checked(self.word_wrap)
                    .on_change(cx.listener(|page, on: &bool, _, cx| {
                        page.word_wrap = *on;
                        cx.notify();
                    })),
            )
            .child(
                Checkbox::new("line-numbers")
                    .label("Line numbers")
                    .checked(self.line_numbers)
                    .on_change(cx.listener(|page, on: &bool, _, cx| {
                        page.line_numbers = *on;
                        cx.notify();
                    })),
            )
            .child(
                Checkbox::new("disabled-on")
                    .label("Disabled, checked")
                    .checked(true)
                    .disabled(true),
            )
            .child(
                Checkbox::new("disabled-off")
                    .label("Disabled, unchecked")
                    .disabled(true),
            );
        let switches = v_flex()
            .gap_2()
            .child(
                Switch::new("autosave")
                    .label("Autosave")
                    .on(self.autosave)
                    .on_change(cx.listener(|page, on: &bool, _, cx| {
                        page.autosave = *on;
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("telemetry")
                    .label("Telemetry")
                    .on(self.telemetry)
                    .on_change(cx.listener(|page, on: &bool, _, cx| {
                        page.telemetry = *on;
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("switch-disabled-on")
                    .label("Disabled, on")
                    .on(true)
                    .disabled(true),
            )
            .child(
                Switch::new("switch-disabled-off")
                    .label("Disabled, off")
                    .disabled(true),
            );
        h_flex()
            .gap_5()
            .items_start()
            .child(section("CHECKBOX", checkboxes, cx))
            .child(section("SWITCH", switches, cx))
    }
}

impl Render for ControlsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let buttons = self.buttons(cx);
        let toggles = self.toggles(cx);
        page_root("controls-page", cx).child(
            page_frame(
                "Controls",
                "Button in each variant and size, with icons and disabled; Checkbox and Switch.",
                cx,
            )
            .child(section("BUTTON", buttons, cx))
            .child(toggles),
        )
    }
}
