// SPDX-License-Identifier: Apache-2.0

//! TextInput: a click focuses it, and a bordered one keeps its
//! `border.control` border while it has focus.

use gpui::{
    AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    Modifiers, ParentElement as _, Render, Styled as _, TestAppContext, Window, div, px,
};

use crate::{ActiveTheme as _, TextInput, UiTheme};

struct Host {
    plain: Entity<TextInput>,
    bordered: Entity<TextInput>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(300.))
            .child(
                div()
                    .debug_selector(|| "plain".into())
                    .child(self.plain.clone()),
            )
            .child(
                div()
                    .debug_selector(|| "bordered".into())
                    .child(self.bordered.clone()),
            )
    }
}

#[gpui::test]
fn a_click_focuses_it_and_a_bordered_one_shows_focus(cx: &mut TestAppContext) {
    let (host, cx) = cx.add_window_view(|_, cx| {
        let theme = UiTheme::default();
        Host {
            plain: cx.new(|cx| TextInput::new("Plain", &theme, cx)),
            bordered: cx.new(|cx| TextInput::new("Bordered", &theme, cx).bordered()),
        }
    });
    cx.run_until_parked();
    let (plain, bordered) =
        host.read_with(cx, |host, _| (host.plain.clone(), host.bordered.clone()));

    for (selector, input) in [("plain", &plain), ("bordered", &bordered)] {
        let at = cx.debug_bounds(selector).unwrap().center();
        cx.simulate_click(at, Modifiers::none());
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(input.focus_handle(cx).is_focused(window), "{selector}");
        });
    }

    cx.update(|_, cx| {
        let c = cx.theme().colors;
        assert_eq!(plain.read(cx).border_color(true, &c), None, "opt-in");
        let input = bordered.read(cx);
        // Focus shows as the focus ring outside it (tests::b3_controls).
        assert_eq!(input.border_color(false, &c), Some(c.border.control));
        assert_eq!(input.border_color(true, &c), Some(c.border.control));
    });
}
