// SPDX-License-Identifier: LicenseRef-Proprietary

//! The side docks run the main area's full height, to the status bar; the
//! bottom dock lies between them, under the centre only.

use crate::support;

use std::{cell::Cell, rc::Rc};

use gpui::{
    AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, SharedString, Styled as _, TestAppContext,
    VisualTestContext, Window, canvas, div,
};
use kiln_shell::{Dock, DockItem, DockPosition, DockView as _, PanelId, Shell};
use kiln_ui::Icon;
use support::{APP, Pad};

/// Records where it was laid out.
struct Probe {
    id: &'static str,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    focus: FocusHandle,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let bounds = self.bounds.clone();
        div()
            .track_focus(&self.focus)
            .size_full()
            .child(canvas(move |laid_out, _, _| bounds.set(laid_out), |_, _, _, _| {}).size_full())
    }
}

impl Focusable for Probe {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DockItem for Probe {
    fn id(&self) -> PanelId {
        PanelId(self.id)
    }

    fn title(&self, _: &gpui::App) -> SharedString {
        self.id.into()
    }

    fn icon(&self, _: &gpui::App) -> Icon {
        Icon::Files
    }
}

fn probe(id: &'static str, cx: &mut VisualTestContext) -> Entity<Probe> {
    cx.new(|cx| Probe {
        id,
        bounds: Rc::default(),
        focus: cx.focus_handle(),
    })
}

#[gpui::test]
fn side_docks_run_full_height_with_the_bottom_dock_between(cx: &mut TestAppContext) {
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
    let bar = probe("bar", cx);
    let docks = [
        (DockPosition::Left, "left"),
        (DockPosition::Right, "right"),
        (DockPosition::Bottom, "bottom"),
    ]
    .map(|(at, id)| {
        let panel = probe(id, cx);
        let dock = cx.new(|cx| {
            let mut dock = Dock::new(at);
            dock.add_panel(panel, cx);
            dock
        });
        (at, dock)
    });
    shell.update(cx, |shell, cx| {
        for (at, dock) in &docks {
            shell.set_dock(*at, Some(dock.clone().into()), cx);
        }
        shell.set_status_bar(Some(bar.clone().into()), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let pad = cx.debug_bounds("pad").expect("the pad is drawn");
    let status = bar.read_with(cx, |bar, _| bar.bounds.get());
    let column = |at: DockPosition, cx: &mut VisualTestContext| {
        let (_, dock) = docks.iter().find(|(position, _)| *position == at).unwrap();
        dock.read_with(cx, |dock, _| dock.dock().drawn().column_bounds())
            .expect("shown")
    };
    let (left, right, bottom) = (
        column(DockPosition::Left, cx),
        column(DockPosition::Right, cx),
        column(DockPosition::Bottom, cx),
    );

    // The side docks run from the main area's top to the status bar.
    for side in [left, right] {
        assert_eq!(side.top(), pad.top(), "{side:?} {pad:?}");
        assert_eq!(side.bottom(), status.top(), "{side:?} {status:?}");
    }
    // The bottom dock lies between them, under the centre.
    assert_eq!(bottom.left(), left.right(), "{bottom:?} {left:?}");
    assert_eq!(bottom.right(), right.left(), "{bottom:?} {right:?}");
    assert_eq!(bottom.top(), pad.bottom(), "{bottom:?} {pad:?}");
    assert_eq!(bottom.bottom(), status.top(), "{bottom:?} {status:?}");
}
