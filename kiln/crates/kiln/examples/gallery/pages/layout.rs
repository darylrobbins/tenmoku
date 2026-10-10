// SPDX-License-Identifier: LicenseRef-Proprietary

//! ScrollArea, a SplitPane nested in another, and Tabs.

use gpui::{
    AppContext as _, Axis, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, Window, div, px,
};
use kiln::{
    prelude::*,
    ui::{ScrollArea, SplitPane, Tabs, TabsEvent, h_flex, v_flex},
};

use super::{Placeholder, page_frame, page_root, section};

pub(crate) struct LayoutPage {
    split: Entity<SplitPane>,
    tabs: Entity<Tabs>,
    _subscriptions: [Subscription; 1],
}

impl LayoutPage {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = |label: &str, cx: &mut Context<Self>| {
            let label = label.to_owned();
            cx.new(|_| Placeholder::new(label))
        };
        let (left, top_right, bottom_right) = (
            placeholder("Left", cx),
            placeholder("Top right", cx),
            placeholder("Bottom right", cx),
        );
        let right = cx.new(|_| {
            SplitPane::new(Axis::Vertical, top_right, bottom_right).with_min_sizes(px(30.), px(30.))
        });
        let split = cx.new(|_| {
            SplitPane::new(Axis::Horizontal, left, right)
                .with_ratio(0.35)
                .with_min_sizes(px(60.), px(60.))
        });
        let (one, two, three) = (
            placeholder("The first tab's view", cx),
            placeholder("The second tab's view", cx),
            placeholder("The third tab's view", cx),
        );
        let tabs = cx.new(|_| {
            Tabs::new()
                .with_tab("Overview", one)
                .with_tab("Details", two)
                .with_tab("History", three)
        });
        // A tab's close button asks; the owner removes it.
        let tab_events = cx.subscribe_in(&tabs, window, |_, tabs, event, _, cx| {
            if let TabsEvent::CloseRequested(ix) = *event {
                tabs.update(cx, |tabs, cx| tabs.remove(ix, cx));
            }
        });
        Self {
            split,
            tabs,
            _subscriptions: [tab_events],
        }
    }
}

impl Render for LayoutPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.theme().colors;
        let lines = (1..=60).map(|n| {
            div()
                .px_3()
                .py_1()
                .child(format!("Line {n} of the scroll area's content"))
        });
        let frame = || {
            div()
                .border_1()
                .border_color(c.border.default)
                .bg(c.surface.panel)
        };
        let scroll = frame()
            .w(px(360.))
            .h(px(220.))
            .child(ScrollArea::new("layout-scroll").size_full().children(lines));
        let split = frame().w(px(420.)).h(px(220.)).child(self.split.clone());
        let tabs = frame().w(px(520.)).h(px(180.)).child(self.tabs.clone());
        page_root("layout-page", cx).child(
            page_frame(
                "Layout",
                "ScrollArea draws a draggable thumb; SplitPane's border drags; Tabs shows one view.",
                cx,
            )
            .child(
                h_flex()
                    .gap_5()
                    .items_start()
                    .child(section("SCROLLAREA", scroll, cx))
                    .child(section("SPLITPANE, NESTED", split, cx)),
            )
            .child(v_flex().child(section("TABS", tabs, cx))),
        )
    }
}
