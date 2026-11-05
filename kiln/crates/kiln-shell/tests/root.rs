// SPDX-License-Identifier: Apache-2.0

//! What a window gets from [`Root`] as it's made: focus that moves off an
//! element that stops being drawn, and the theme following the system's
//! appearance.

use crate::support;

use gpui::{
    AppContext as _, Context, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, TestAppContext, Window, WindowAppearance, div,
};
use kiln_shell::Root;
use kiln_ui::{
    ActiveTheme as _, Appearance, Theme, ThemeMode, set_themes, system_appearance_changed,
};

/// A view with a focusable child, drawn while `shown`.
struct Panel {
    outer: FocusHandle,
    inner: FocusHandle,
    shown: bool,
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.outer).size_full().children(
            self.shown
                .then(|| div().track_focus(&self.inner).size_full()),
        )
    }
}

#[gpui::test]
fn focus_leaves_an_element_no_longer_drawn(cx: &mut TestAppContext) {
    let (root, cx) = cx.add_window_view(|window, cx| {
        let panel = cx.new(|cx| Panel {
            outer: cx.focus_handle(),
            inner: cx.focus_handle(),
            shown: true,
        });
        Root::new(panel, window, cx)
    });
    let panel = root.read_with(cx, |root, _| {
        root.content().clone().downcast::<Panel>().unwrap()
    });
    let (outer, inner) = panel.read_with(cx, |panel, _| (panel.outer.clone(), panel.inner.clone()));
    cx.update(|window, cx| window.focus(&inner, cx));
    cx.run_until_parked();
    panel.update(cx, |panel, cx| {
        panel.shown = false;
        cx.notify();
    });
    cx.run_until_parked();
    cx.update(|window, _| {
        assert!(!inner.is_focused(window));
        assert!(outer.is_focused(window), "its nearest drawn ancestor");
    });
}

#[gpui::test]
fn the_theme_follows_the_system_appearance(cx: &mut TestAppContext) {
    // The test platform is light, so as if the system had been dark until
    // now. A window tells its observers its appearance once it opens.
    cx.update(|cx| {
        set_themes(cx, ThemeMode::System, Theme::dark(), Theme::light());
        system_appearance_changed(cx, WindowAppearance::Dark);
    });
    assert_eq!(cx.read(|cx| cx.theme().appearance), Appearance::Dark);
    cx.add_window(|window, cx| {
        let content = cx.new(|_| support::Blank);
        Root::new(content, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| cx.theme().appearance), Appearance::Light);
}
