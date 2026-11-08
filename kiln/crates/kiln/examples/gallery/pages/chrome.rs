// SPDX-License-Identifier: Apache-2.0

//! The app chrome kiln-ui grew from: TabBar, Breadcrumbs, StatusBarFrame,
//! ActivityRail, Tooltip, IconButton and ToggleButton. These take the flat
//! `UiTheme` when built, rather than reading `cx.theme()`.

use gpui::{
    AnyView, App, AppContext as _, ClickEvent, Context, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div, px,
};
use kiln::{
    prelude::*,
    ui::{
        ActivityRail, Breadcrumbs, Crumb, CrumbKind, Icon, IconButton, RailButton, StatusBarFrame,
        StatusEntry, Tab, TabBar, ToggleButton, Tooltip, file_icon, h_flex, ui_theme,
    },
};

use super::{labelled, page_frame, page_root, section};

const TABS: [(&str, bool); 4] = [
    ("main.rs", false),
    ("store.rs", true),
    ("view.rs", false),
    ("Cargo.toml", false),
];

const RAIL: [(Icon, &str); 3] = [
    (Icon::Files, "Files"),
    (Icon::Search, "Search"),
    (Icon::ListTree, "Outline"),
];

pub(crate) struct ChromePage {
    active_tab: usize,
    rail: usize,
    match_case: bool,
    whole_word: bool,
    last: Option<SharedString>,
}

impl ChromePage {
    pub(crate) fn new() -> Self {
        Self {
            active_tab: 0,
            rail: 0,
            match_case: true,
            whole_word: false,
            last: None,
        }
    }

    fn did(&mut self, what: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.last = Some(what.into());
        cx.notify();
    }
}

/// A tooltip's view builder, as `.tooltip(..)` takes.
fn tooltip(
    label: &'static str,
    shortcut: Option<&'static str>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_, cx| {
        let theme = ui_theme(cx);
        cx.new(|_| Tooltip::new(label, shortcut, &theme)).into()
    }
}

impl Render for ChromePage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ui_theme(cx);
        let c = cx.theme().colors;
        let tabs = TABS.iter().enumerate().map(|(ix, (label, dirty))| {
            Tab::new(("chrome-tab", ix), *label, &theme)
                .active(ix == self.active_tab)
                .dirty(*dirty)
                .on_click(cx.listener(move |page, _: &ClickEvent, _, cx| {
                    page.active_tab = ix;
                    cx.notify();
                }))
                .on_close(cx.listener(move |page, _: &ClickEvent, _, cx| {
                    page.did(format!("Close {} asked", TABS[ix].0), cx)
                }))
        });
        let tab_bar = div()
            .w(px(520.))
            .border_1()
            .border_color(c.border.default)
            .child(TabBar::new("chrome-tabs", &theme).children(tabs));

        let crumb = |label: &'static str, kind| {
            Crumb::new(label, kind).on_click(cx.listener(move |page, _: &ClickEvent, _, cx| {
                page.did(format!("Crumb {label}"), cx)
            }))
        };
        let crumbs = Breadcrumbs::new(
            vec![
                crumb("notes", CrumbKind::Path),
                crumb("src", CrumbKind::Path),
                crumb("store.rs", CrumbKind::File).icon(file_icon("store.rs")),
                crumb("Store::save", CrumbKind::Symbol),
            ],
            &theme,
        );
        let crumbs = div()
            .w(px(520.))
            .border_1()
            .border_color(c.border.default)
            .child(crumbs);

        let status = StatusBarFrame::new(&theme)
            .left([StatusEntry::new("branch", "main", &theme).on_click(
                cx.listener(|page, _: &ClickEvent, _, cx| page.did("Status: branch", cx)),
            )])
            .right([
                StatusEntry::new("position", "Ln 12, Col 4", &theme),
                StatusEntry::new("encoding", "UTF-8", &theme),
            ]);
        let status = div().w(px(520.)).child(status);

        let rail_buttons = RAIL.iter().enumerate().map(|(ix, (icon, label))| {
            RailButton::new(("rail", ix), *icon, &theme)
                .highlighted(ix == self.rail)
                .tooltip(tooltip(label, None))
                .on_click(cx.listener(move |page, _: &ClickEvent, _, cx| {
                    page.rail = ix;
                    cx.notify();
                }))
        });
        let rail = h_flex()
            .h(px(140.))
            .border_1()
            .border_color(c.border.default)
            .child(ActivityRail::new("rail", &theme).children(rail_buttons))
            .child(
                div()
                    .p_3()
                    .text_color(c.text.muted)
                    .child(format!("{} panel", RAIL[self.rail].1)),
            );

        let icon_buttons = h_flex()
            .gap_2()
            .items_center()
            .child(
                IconButton::new("back", Icon::ArrowLeft, &theme)
                    .name("Back")
                    .tooltip(tooltip("Back", Some("Alt+Left")))
                    .on_click(cx.listener(|page, _: &ClickEvent, _, cx| page.did("Back", cx))),
            )
            .child(
                IconButton::new("forward", Icon::ArrowRight, &theme)
                    .name("Forward")
                    .disabled(true)
                    .tooltip(tooltip("Forward (disabled)", None)),
            )
            .child(
                IconButton::new("split", Icon::Columns2, &theme)
                    .name("Split Right")
                    .tooltip(tooltip("Split Right", None))
                    .on_click(cx.listener(|page, _: &ClickEvent, _, cx| page.did("Split", cx))),
            )
            .child(
                div()
                    .id("hover-me")
                    .ml_3()
                    .px_2()
                    .border_1()
                    .border_color(c.border.default)
                    .rounded_sm()
                    .tooltip(tooltip("A Tooltip on any element", Some("with a shortcut")))
                    .child("Hover me"),
            );
        let toggles = h_flex()
            .gap_2()
            .child(
                ToggleButton::new("match-case", "Aa", &theme)
                    .on(self.match_case)
                    .on_click(cx.listener(|page, _: &ClickEvent, _, cx| {
                        page.match_case = !page.match_case;
                        cx.notify();
                    })),
            )
            .child(
                ToggleButton::new("whole-word", "ab", &theme)
                    .on(self.whole_word)
                    .on_click(cx.listener(|page, _: &ClickEvent, _, cx| {
                        page.whole_word = !page.whole_word;
                        cx.notify();
                    })),
            );
        let last = self
            .last
            .clone()
            .unwrap_or_else(|| "Click a crumb, a tab's close button or a status item".into());

        let page = page_frame("Chrome", "The widgets an app's frame is built from.", cx)
            .child(section("TABBAR", tab_bar, cx))
            .child(section("BREADCRUMBS", crumbs, cx))
            .child(section("STATUSBARFRAME", status, cx))
            .child(section("ACTIVITYRAIL", rail, cx))
            .child(section(
                "ICONBUTTON, TOOLTIP, TOGGLEBUTTON",
                h_flex()
                    .gap_5()
                    .child(labelled("Icons", icon_buttons, cx))
                    .child(labelled("Toggles", toggles, cx)),
                cx,
            ))
            .child(div().text_color(c.text.muted).child(last));
        page_root("chrome-page", cx).child(page)
    }
}
