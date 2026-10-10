// SPDX-License-Identifier: LicenseRef-Proprietary

//! Tabs: a click shows a tab's view, a close button asks the owner, who
//! removes the tab.

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, Modifiers,
    ParentElement as _, Render, Styled as _, Subscription, TestAppContext, VisualTestContext,
    Window, div, point, px,
};

use crate::{Tabs, TabsEvent};

struct Page(&'static str);

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let name = self.0;
        div().size_full().debug_selector(move || name.into())
    }
}

/// Owns tabs one, two and three, removing a tab when asked to close it.
struct Host {
    tabs: Entity<Tabs>,
    events: Vec<TabsEvent>,
    _events: Subscription,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(div().w(px(500.)).h(px(300.)).child(self.tabs.clone()))
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn tab_bounds(
    host: &Entity<Host>,
    ix: usize,
    cx: &mut VisualTestContext,
) -> gpui::Bounds<gpui::Pixels> {
    host.read_with(cx, |host, cx| {
        host.tabs
            .read(cx)
            .scroll_handle()
            .bounds_for_item(ix)
            .expect("tab drawn")
    })
}

#[gpui::test]
fn clicking_tabs_shows_their_views_and_closing_asks_the_owner(cx: &mut TestAppContext) {
    let (host, cx) = cx.add_window_view(|window, cx| {
        let pages = ["one", "two", "three"].map(|name| cx.new(|_| Page(name)));
        let tabs = cx.new(|_| {
            pages
                .into_iter()
                .zip(["One", "Two", "Three"])
                .fold(Tabs::new(), |tabs, (page, label)| {
                    tabs.with_tab(label, page)
                })
        });
        let events = cx.subscribe_in(&tabs, window, |host: &mut Host, tabs, event, _, cx| {
            host.events.push(*event);
            if let TabsEvent::CloseRequested(ix) = *event {
                tabs.update(cx, |tabs, cx| tabs.remove(ix, cx));
            }
        });
        Host {
            tabs,
            events: Vec::new(),
            _events: events,
        }
    });
    draw(cx);
    assert!(cx.debug_bounds("one").is_some(), "the first tab shown");
    assert!(cx.debug_bounds("two").is_none());

    let two = tab_bounds(&host, 1, cx);
    cx.simulate_click(
        point(two.left() + px(10.), two.center().y),
        Modifiers::none(),
    );
    draw(cx);
    assert!(cx.debug_bounds("two").is_some());
    assert!(cx.debug_bounds("one").is_none());

    // The close button is the last thing in the tab, before its padding.
    let two = tab_bounds(&host, 1, cx);
    cx.simulate_click(
        point(two.right() - px(15.), two.center().y),
        Modifiers::none(),
    );
    draw(cx);
    let events = host.update(cx, |host, _| std::mem::take(&mut host.events));
    assert_eq!(
        events,
        [TabsEvent::Activated(1), TabsEvent::CloseRequested(1)]
    );
    let (len, active) = host.read_with(cx, |host, cx| {
        let tabs = host.tabs.read(cx);
        (tabs.len(), tabs.active())
    });
    assert_eq!((len, active), (2, 1), "the tab after the closed one shown");
    assert!(cx.debug_bounds("three").is_some());
}
