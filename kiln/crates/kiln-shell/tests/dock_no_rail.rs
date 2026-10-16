// SPDX-License-Identifier: LicenseRef-Proprietary

//! A dock built without a rail draws no rail buttons or actions, at the
//! bottom or a side: shown, only its panel's title and body; hidden,
//! nothing.

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, TestAppContext, VisualTestContext,
    Window, div, px,
};
use kiln_shell::{Dock, DockItem, DockPosition, DockState, DockView as _, PanelId, RailAction};
use kiln_ui::Icon;

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
        PanelId("log")
    }

    fn title(&self, _: &gpui::App) -> SharedString {
        "Log".into()
    }

    fn icon(&self, _: &gpui::App) -> Icon {
        Icon::Files
    }
}

struct Host(Entity<Dock>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().child(self.0.clone())
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn open(position: DockPosition, cx: &mut TestAppContext) -> (Entity<Dock>, &mut VisualTestContext) {
    let (host, cx) = cx.add_window_view(move |_, cx| {
        let panel = cx.new(|cx| Panel(cx.focus_handle()));
        let dock = cx.new(|cx| {
            let state = DockState::new(position).without_rail().size(px(200.));
            let mut dock = Dock::with_state(state);
            dock.add_panel(panel, cx);
            dock.add_action(
                RailAction {
                    id: "act",
                    icon: Icon::FileTerminal,
                    title: "Act".into(),
                    command: "act.run",
                },
                cx,
            );
            dock
        });
        Host(dock)
    });
    let dock = host.read_with(cx, |host, _| host.0.clone());
    draw(cx);
    (dock, cx)
}

#[gpui::test]
fn a_dock_without_a_rail_draws_only_its_panel(cx: &mut TestAppContext) {
    for position in [
        DockPosition::Bottom,
        DockPosition::Right,
        DockPosition::Left,
    ] {
        let (dock, cx) = open(position, cx);
        let drawn = dock.read_with(cx, |dock, _| dock.dock().drawn().clone());

        // Shown: no rail, a title and a body.
        assert!(dock.read_with(cx, |dock, _| dock.is_visible()));
        assert_eq!(drawn.rail_icons(), [], "{position:?}");
        assert!(drawn.rail_bounds().is_none(), "{position:?}");
        assert!(drawn.rail_button_bounds(PanelId("log")).is_none());
        assert!(drawn.rail_button_bounds(PanelId("act")).is_none());
        let column = drawn.column_bounds().expect("the column is drawn");
        let title = drawn.title_bounds().expect("the title is drawn");
        assert!(title.size.height > px(0.) && column.size.height > title.size.height);

        // Hidden, nothing.
        dock.update(cx, |dock, cx| dock.toggle(cx));
        draw(cx);
        assert_eq!(drawn.rail_icons(), [], "{position:?}");
        assert!(drawn.rail_bounds().is_none());
        assert!(
            drawn.column_bounds().is_none(),
            "{position:?} {:?}",
            drawn.column_bounds()
        );
        assert!(drawn.title_bounds().is_none());
    }
}
