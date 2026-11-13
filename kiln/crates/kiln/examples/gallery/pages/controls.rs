// SPDX-License-Identifier: Apache-2.0

//! Buttons in every variant and size, disabled ones and a focused one,
//! checkboxes, switches, toggle buttons, segmented controls and badges,
//! each in its states. The values are the page's: controls show what
//! they're given and report clicks.

use gpui::{
    App, ClickEvent, Context, Div, ElementId, FocusHandle, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window, div, prelude::FluentBuilder as _, px, svg,
};
use kiln::{
    prelude::*,
    ui::{
        Badge, BadgeTone, Button, ButtonSize, ButtonVariant, Checkbox, Icon, SegmentedControl,
        Switch, ToggleButton, h_flex, ui_theme, v_flex,
    },
};
use kiln_l10n::Text;

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

pub(crate) struct ControlsPage {
    /// The "Focused" button's focus, which [`ControlsPage::focus_demo`]
    /// moves to it.
    focus_demo: FocusHandle,
    clicks: usize,
    last_clicked: Option<SharedString>,
    word_wrap: bool,
    line_numbers: bool,
    autosave: bool,
    telemetry: bool,
    match_case: bool,
    whole_word: bool,
    /// The chosen segment of the scope switch.
    scope: usize,
}

impl ControlsPage {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_demo: cx.focus_handle(),
            clicks: 0,
            last_clicked: None,
            word_wrap: false,
            line_numbers: true,
            autosave: true,
            telemetry: false,
            match_case: true,
            whole_word: false,
            scope: 0,
        }
    }

    /// The "Focused" button's focus handle: focused, as tabbing to it would
    /// leave it, it shows the focus ring while the keyboard was the last
    /// input.
    #[allow(
        dead_code,
        reason = "the gallery's screenshots use it, the gallery doesn't"
    )]
    pub(crate) fn focus_demo(&self) -> FocusHandle {
        self.focus_demo.clone()
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
        Button::new(id, Text::verbatim(size.1))
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
                    Button::new(
                        SharedString::from(format!("{name} disabled")),
                        Text::verbatim("Disabled"),
                    )
                    .variant(variant.0)
                    .disabled(true),
                )
                .child(
                    Button::icon_only(SharedString::from(format!("{name} icon")), Icon::Search)
                        .name("Search")
                        .shortcut("⇧⌘F")
                        .variant(variant.0)
                        .on_click(cx.listener(move |page, _: &ClickEvent, _, cx| {
                            page.clicked(format!("{name} icon").into(), cx)
                        })),
                )
                .child(
                    Button::new(
                        SharedString::from(format!("{name} labelled icon")),
                        Text::verbatim("Apply"),
                    )
                    .icon(Icon::Check)
                    .variant(variant.0)
                    .on_click(cx.listener(
                        move |page, _: &ClickEvent, _, cx| {
                            page.clicked(format!("{name} Apply").into(), cx)
                        },
                    )),
                )
                .when(variant.0 == ButtonVariant::Secondary, |row| {
                    // Focused from the keyboard, it shows the focus ring:
                    // the gallery's screenshots tab to it.
                    row.child(
                        Button::new("focused", Text::verbatim("Focused"))
                            .focus_handle(&self.focus_demo)
                            .on_click(cx.listener(|page, _: &ClickEvent, _, cx| {
                                page.clicked("Focused".into(), cx)
                            })),
                    )
                });
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
            .child(
                v_flex()
                    .gap_4()
                    .child(
                        h_flex()
                            .gap_5()
                            .items_start()
                            .child(section("TOGGLE BUTTON", self.toggle_buttons(cx), cx))
                            .child(section("BADGE", badges(cx), cx)),
                    )
                    .child(section("SEGMENTED CONTROL", self.segmented(cx), cx)),
            )
    }

    fn toggle_buttons(&self, cx: &mut Context<Self>) -> Div {
        let ui = ui_theme(cx);
        h_flex()
            .gap_1()
            .items_center()
            .child(
                ToggleButton::new("match-case", "Aa", "Match Case", &ui)
                    .shortcut("⌥⌘C")
                    .on(self.match_case)
                    .on_click(cx.listener(|page, _: &ClickEvent, _, cx| {
                        page.match_case = !page.match_case;
                        cx.notify();
                    })),
            )
            .child(
                ToggleButton::new("whole-word", "ab", "Whole Word", &ui)
                    .on(self.whole_word)
                    .on_click(cx.listener(|page, _: &ClickEvent, _, cx| {
                        page.whole_word = !page.whole_word;
                        cx.notify();
                    })),
            )
            .child(
                ToggleButton::new("regex-disabled-on", ".*", "Regex", &ui)
                    .on(true)
                    .disabled(true),
            )
            .child(ToggleButton::new("regex-disabled-off", ".*", "Regex", &ui).disabled(true))
    }

    fn segmented(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap_2()
            .items_start()
            .child(
                SegmentedControl::new("scope", "Settings scope")
                    .segments(["You", "This project"])
                    .selected(self.scope)
                    .on_change(cx.listener(|page, ix: &usize, _, cx| {
                        page.scope = *ix;
                        cx.notify();
                    })),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        SegmentedControl::new("scope-small", "Settings scope")
                            .segments(["You", "This project"])
                            .selected(1)
                            .small(),
                    )
                    .child(
                        SegmentedControl::new("scope-disabled", "Settings scope")
                            .segments(["You", "Project"])
                            .small()
                            .disabled(true),
                    ),
            )
    }
}

/// An icon with a badge of each kind: counts by meaning and a dot.
fn badges(cx: &App) -> Div {
    let c = &cx.theme().colors;
    let icon = |icon: Icon, badge: Badge| {
        div()
            .relative()
            .size(px(16.))
            .child(
                svg()
                    .path(icon.path())
                    .size(px(16.))
                    .text_color(c.list.icon),
            )
            .child(badge.surface(c.surface.background))
    };
    h_flex()
        .gap_6()
        .h(px(22.))
        .items_center()
        .pr_1()
        .child(icon(Icon::File, Badge::count(3).tone(BadgeTone::Error)))
        .child(icon(Icon::File, Badge::count(12).tone(BadgeTone::Warning)))
        .child(icon(
            Icon::GitBranch,
            Badge::count(120).tone(BadgeTone::Modified),
        ))
        .child(icon(Icon::Search, Badge::dot()))
        .child(icon(Icon::Folder, Badge::dot().tone(BadgeTone::Success)))
}

impl Render for ControlsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let buttons = self.buttons(cx);
        let toggles = self.toggles(cx);
        page_root("controls-page", cx).child(
            page_frame(
                "Controls",
                "Each control in its states: enabled, disabled, focused, on and off.",
                cx,
            )
            .child(section("BUTTON", buttons, cx))
            .child(toggles),
        )
    }
}
