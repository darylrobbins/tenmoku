// SPDX-License-Identifier: LicenseRef-Proprietary

//! A tab's title colour: an item's `tab_title_color` replaces the theme's
//! tab text colour, active or not, with the dirty dot unchanged, and the
//! default keeps the theme's. Read from the tabs the pane draws.

mod support;

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, TestAppContext,
    VisualTestContext, Window, div, hsla,
};
use kiln_shell::{ItemHandle, ItemPane, PaneItem, PaneView as _};
use kiln_ui::{ActiveTheme as _, UiTheme};
use support::note::{Note, note};

struct Host(Entity<ItemPane>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(self.0.clone())
    }
}

/// An item that keeps `PaneItem`'s default `tab_title_color`.
struct Plain(FocusHandle);

impl Render for Plain {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.0).size_full()
    }
}

impl Focusable for Plain {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.0.clone()
    }
}

impl PaneItem for Plain {
    const KIND: &'static str = "test.plain";

    fn tab_title(&self, _: &gpui::App) -> SharedString {
        "plain".into()
    }
}

/// Each tab's title colour and dirty flag, as the pane draws them.
fn drawn(pane: &Entity<ItemPane>, cx: &mut VisualTestContext) -> Vec<(Hsla, bool)> {
    cx.update(|_, cx| {
        let tabs = pane.read(cx).pane().tabs(cx);
        tabs.iter()
            .map(|tab| (tab.title_color(cx), tab.is_dirty()))
            .collect()
    })
}

/// How many dirty-dot quads were painted in the last frame inside each
/// tab's bounds, and in `colour`.
fn dots(pane: &Entity<ItemPane>, colour: Hsla, cx: &mut VisualTestContext) -> Vec<usize> {
    cx.update(|window, cx| {
        let scale = window.scale_factor();
        let quads = window.painted_quads();
        let n = pane.read(cx).pane().items().len();
        (0..n)
            .map(|ix| {
                let tab = pane.read(cx).pane().tab_bounds(ix).expect("drawn");
                let tab = tab.scale(scale);
                quads
                    .iter()
                    .filter(|quad| {
                        quad.background == colour.into() && tab.contains(&quad.bounds.center())
                    })
                    .count()
            })
            .collect()
    })
}

// Checks the tabs `PaneState` builds for drawing, and the painted dirty dots.
// Asserting the drawn glyph colour through the render path is follow-up T715:
// GPUI's test harness doesn't expose painted glyph colours.
#[gpui::test]
fn a_tinted_title_is_drawn_in_its_tint_active_or_not(cx: &mut TestAppContext) {
    let (host, cx) =
        cx.add_window_view(|_, cx| Host(cx.new(|cx| ItemPane::new(UiTheme::default_dark(), cx))));
    let pane = host.read_with(cx, |host, _| host.0.clone());
    let tint = hsla(0.3, 0.6, 0.5, 1.);
    let [a, b] = ["a", "b"].map(|title| note(title, cx));
    let plain = cx.new(|cx| Plain(cx.focus_handle()));
    b.update(cx, |note: &mut Note, _| {
        note.tint = Some(tint);
        note.dirty = true;
    });
    let items: [Box<dyn ItemHandle>; 3] = [Box::new(a), Box::new(b), Box::new(plain)];
    for item in items {
        pane.update(cx, |pane, cx| pane.add_item(item, cx));
    }
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (active_text, text, dirty) = cx.update(|_, cx| {
        let colors = &cx.theme().colors;
        (
            colors.tab.active_text,
            colors.text.primary,
            colors.state.dirty,
        )
    });

    // `plain`, with the default, is active; `b` is tinted, dirty, inactive.
    assert_eq!(
        drawn(&pane, cx),
        [(text, false), (tint, true), (active_text, false)]
    );

    // The dirty dot is painted, in the theme's colour and not the tint.
    assert_eq!(dots(&pane, dirty, cx), [0, 1, 0]);
    assert_eq!(dots(&pane, tint, cx), [0, 0, 0]);

    pane.update(cx, |pane, _| pane.pane_mut().set_active(1));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        drawn(&pane, cx),
        [(text, false), (tint, true), (text, false)],
        "the tint is the same active, and the dirty dot stays"
    );
    assert_eq!(dots(&pane, dirty, cx), [0, 1, 0]);
    assert_eq!(dots(&pane, tint, cx), [0, 0, 0]);
}
