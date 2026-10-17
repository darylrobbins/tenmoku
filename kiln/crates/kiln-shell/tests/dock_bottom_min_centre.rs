// SPDX-License-Identifier: LicenseRef-Proprietary

//! A window shorter than the bottom dock's size shrinks the dock before the
//! centre, which keeps `MIN_PANE_HEIGHT`.

mod support;

use gpui::{
    AppContext as _, Context, FocusHandle, Focusable, InteractiveElement as _, IntoElement, Render,
    SharedString, Styled as _, TestAppContext, Window, div, px, size,
};
use kiln_shell::{
    Dock, DockItem, DockPosition, DockState, DockView as _, PanelId, Shell,
    pane_group::MIN_PANE_HEIGHT,
};
use kiln_ui::Icon;
use support::{APP, Pad};

struct Panel(FocusHandle);

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.0).size_full()
    }
}

impl Focusable for Panel {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.0.clone()
    }
}

impl DockItem for Panel {
    fn id(&self) -> PanelId {
        PanelId("panel")
    }

    fn title(&self, _: &gpui::App) -> SharedString {
        "panel".into()
    }

    fn icon(&self, _: &gpui::App) -> Icon {
        Icon::Files
    }
}

#[gpui::test]
fn a_shortened_window_shrinks_the_bottom_dock_before_the_centre(cx: &mut TestAppContext) {
    cx.update(|cx| kiln_platform::commands::init(cx, "Pad"));
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let pad = cx.new(|cx| Pad {
            focus: cx.focus_handle(),
            ran: Vec::new(),
        });
        let mut shell = Shell::new(APP, window, cx);
        shell.set_center(pad, cx);
        shell
    });
    let dock = cx.new(|cx| {
        let mut dock = Dock::with_state(DockState::new(DockPosition::Bottom).size(px(500.)));
        let panel = cx.new(|cx| Panel(cx.focus_handle()));
        dock.add_panel(panel, cx);
        dock
    });
    shell.update(cx, |shell, cx| {
        shell.set_dock(DockPosition::Bottom, Some(dock.clone().into()), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.simulate_resize(size(px(1920.), px(560.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let pad = cx.debug_bounds("pad").expect("the pad is drawn");
    assert!(pad.size.height >= MIN_PANE_HEIGHT, "{pad:?}");
    let column = dock
        .read_with(cx, |dock, _| dock.dock().drawn().column_bounds())
        .expect("shown");
    assert!(column.size.height < px(500.), "{column:?}");
    let saved = dock.read_with(cx, |dock, _| dock.dock().current_size());
    assert_eq!(saved, px(500.));
}
