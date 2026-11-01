// SPDX-License-Identifier: LicenseRef-Proprietary

//! The tab strip scrolls to show the active tab whenever it changes, and
//! otherwise stays where a hand left it.

use crate::support;

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, div, point, px,
};
use kiln_shell::{ItemHandle, ItemPane, PaneView as _, pane::DropTarget};
use support::note::note;

struct Host {
    pane: Entity<ItemPane>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(self.pane.clone())
    }
}

const TABS: usize = 30;

fn open(cx: &mut TestAppContext) -> (Entity<ItemPane>, &mut VisualTestContext) {
    let (host, cx) = cx.add_window_view(|_, cx| Host {
        pane: cx.new(ItemPane::new),
    });
    let pane = host.read_with(cx, |host, _| host.pane.clone());
    for ix in 0..TABS {
        let item: Box<dyn ItemHandle> = Box::new(note(&format!("note {ix}"), cx));
        pane.update(cx, |pane, cx| pane.add_item(item, cx));
    }
    (pane, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn activate(pane: &Entity<ItemPane>, ix: usize, cx: &mut VisualTestContext) {
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate_item(ix, window, cx)));
    draw(cx);
}

/// What the strip's drop target is 1 px inside its left or right end.
fn probe(pane: &Entity<ItemPane>, right: bool, cx: &mut VisualTestContext) -> Option<DropTarget> {
    pane.read_with(cx, |pane, _| {
        let bar = pane.pane().tab_bar_bounds();
        let x = if right {
            bar.right() - px(1.)
        } else {
            bar.left() + px(1.)
        };
        pane.pane()
            .drop_target(point(x, bar.top() + bar.size.height / 2.))
    })
}

#[gpui::test]
fn the_active_tab_scrolls_into_view(cx: &mut TestAppContext) {
    let (pane, cx) = open(cx);
    draw(cx);
    activate(&pane, TABS - 1, cx);
    assert_eq!(probe(&pane, true, cx), Some(DropTarget::TabBar(TABS)));
}

#[gpui::test]
fn a_hand_scroll_stays_until_the_active_tab_changes(cx: &mut TestAppContext) {
    let (pane, cx) = open(cx);
    draw(cx);
    activate(&pane, TABS - 1, cx);
    pane.read_with(cx, |pane, _| pane.pane().scroll_tabs(px(0.)));
    draw(cx);
    draw(cx);
    assert_eq!(probe(&pane, false, cx), Some(DropTarget::TabBar(0)));
    activate(&pane, 0, cx);
    activate(&pane, TABS - 1, cx);
    assert_eq!(probe(&pane, true, cx), Some(DropTarget::TabBar(TABS)));
}
