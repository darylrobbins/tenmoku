// SPDX-License-Identifier: Apache-2.0

//! The window's frame under client-side decorations: the title, the
//! buttons, the drag area and the resize edges, and nothing of them under
//! server-side ones.

use crate::support;

use gpui::{
    Modifiers, MouseButton, MouseDownEvent, Pixels, Point, ResizeEdge, TestAppContext, Tiling,
    VisualTestContext,
};
use kiln_command::{ContextFrame, Source};
use kiln_platform::Capabilities;
use kiln_shell::window_frame::{WindowRequest, simulate_client_decorations};

use support::{APP, linux, open, record, requests};

fn client(tiling: Tiling, cx: &mut VisualTestContext) {
    cx.update(|_, cx| simulate_client_decorations(cx, Some(tiling)));
    cx.run_until_parked();
}

fn press(at: Point<Pixels>, button: MouseButton, click_count: usize, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button,
        click_count,
        first_mouse: false,
    });
}

#[gpui::test]
fn client_decorations_draw_the_title_and_buttons(cx: &mut TestAppContext) {
    let (_, _, cx) = open(cx, linux());
    let seen = record(cx);

    // Server-side decorations, as the test platform has: the menu bar's
    // row, but no frame.
    assert!(cx.debug_bounds("window-drag").is_some());
    assert_eq!(cx.debug_bounds("window-close"), None);

    // Client-side ones, untiled: three buttons right of the drag area.
    client(Tiling::default(), cx);
    for button in ["window-minimize", "window-maximize", "window-close"] {
        assert!(cx.debug_bounds(button).is_some(), "{button}");
    }
    let drag = cx.debug_bounds("window-drag").unwrap();
    let menu = cx.debug_bounds("menu:Window").unwrap();
    let close = cx.debug_bounds("window-close").unwrap();
    assert!(drag.left() >= menu.right(), "the menus come first");
    assert!(
        close.left() >= drag.right(),
        "the buttons are right of the title"
    );

    // The buttons run the window commands, from the mouse, in the app's
    // context.
    cx.simulate_click(close.center(), Modifiers::none());
    cx.run_until_parked();
    let runs: Vec<_> = seen
        .borrow()
        .iter()
        .map(|invocation| {
            let frames: Vec<_> = invocation.context.frames().to_vec();
            (
                invocation.command.as_str().to_owned(),
                invocation.source,
                frames,
            )
        })
        .collect();
    assert_eq!(
        runs,
        [(
            "window.close".to_owned(),
            Source::Mouse,
            vec![ContextFrame::new(APP)]
        )]
    );
}

#[gpui::test]
fn the_drag_area_moves_zooms_and_shows_the_window_menu(cx: &mut TestAppContext) {
    let (_, _, cx) = open(cx, linux());
    let asked = requests(cx);
    let drag = cx.debug_bounds("window-drag").unwrap();

    // Under server-side decorations the window manager moves the window.
    press(drag.center(), MouseButton::Left, 1, cx);
    assert_eq!(asked.take(), []);

    client(Tiling::default(), cx);
    let drag = cx.debug_bounds("window-drag").unwrap();
    press(drag.center(), MouseButton::Left, 1, cx);
    press(drag.center(), MouseButton::Left, 2, cx);
    press(drag.center(), MouseButton::Right, 1, cx);
    assert_eq!(
        asked.take(),
        [
            WindowRequest::Move,
            WindowRequest::Zoom,
            WindowRequest::ShowMenu(drag.center())
        ]
    );

    // Not from the buttons.
    let close = cx.debug_bounds("window-minimize").unwrap();
    press(close.center(), MouseButton::Left, 1, cx);
    assert_eq!(asked.take(), []);
}

#[gpui::test]
fn untiled_edges_resize(cx: &mut TestAppContext) {
    let (_, _, cx) = open(cx, linux());
    let asked = requests(cx);
    assert_eq!(cx.debug_bounds("window-resize-Top"), None, "server-side");

    client(Tiling::default(), cx);
    let size = cx.update(|window, _| window.viewport_size());
    // Along the edges, inside the frame's 1 px border.
    let border = gpui::px(1.);
    let top = cx.debug_bounds("window-resize-Top").unwrap();
    assert_eq!(top.top(), border);
    assert_eq!(top.size.width, size.width - border * 2.);
    let corner = cx.debug_bounds("window-resize-BottomRight").unwrap();
    assert_eq!(
        corner.bottom_right(),
        gpui::point(size.width - border, size.height - border)
    );
    press(top.center(), MouseButton::Left, 1, cx);
    press(corner.center(), MouseButton::Left, 1, cx);
    assert_eq!(
        asked.take(),
        [
            WindowRequest::Resize(ResizeEdge::Top),
            WindowRequest::Resize(ResizeEdge::BottomRight)
        ]
    );

    // A window tiled on its left and top resizes from the other edges.
    client(
        Tiling {
            top: true,
            left: true,
            right: false,
            bottom: false,
        },
        cx,
    );
    for edge in ["Top", "Left", "TopLeft", "TopRight", "BottomLeft"] {
        let selector: &'static str = format!("window-resize-{edge}").leak();
        assert_eq!(cx.debug_bounds(selector), None, "{edge}");
    }
    for edge in ["Bottom", "Right", "BottomRight"] {
        let selector: &'static str = format!("window-resize-{edge}").leak();
        assert!(cx.debug_bounds(selector).is_some(), "{edge}");
    }
}

#[gpui::test]
fn no_row_without_a_menu_bar_or_a_frame(cx: &mut TestAppContext) {
    // Native menus, and a window manager that draws the title bar.
    let capabilities = Capabilities {
        unified_title_bar: false,
        ..support::mac()
    };
    let (_, _, cx) = open(cx, capabilities);
    assert_eq!(cx.debug_bounds("window-drag"), None);
    assert_eq!(
        cx.debug_bounds("pad").unwrap().origin.y,
        Pixels::ZERO,
        "the content fills the window"
    );

    // Client-side decorations: the row is the title bar.
    client(Tiling::default(), cx);
    assert!(cx.debug_bounds("window-drag").is_some());
    assert!(cx.debug_bounds("window-close").is_some());
}

#[gpui::test]
fn the_unified_title_bar_draws_no_frame(cx: &mut TestAppContext) {
    let (_, _, cx) = open(cx, support::mac());
    client(Tiling::default(), cx);
    assert_eq!(cx.debug_bounds("window-close"), None);
    assert_eq!(cx.debug_bounds("window-resize-Top"), None);
}

#[gpui::test]
fn the_unified_window_has_the_lights_at_14_12(cx: &mut TestAppContext) {
    let spec = kiln_platform::window_options::WindowSpec::main("Notes");
    let options = cx.update(|cx| {
        kiln_platform::window_options::window_options(&spec, &support::mac(), cx)
    });
    let titlebar = options.titlebar.expect("a title bar");
    assert_eq!(
        titlebar.traffic_light_position,
        Some(gpui::point(gpui::px(14.), gpui::px(12.)))
    );
    assert!(titlebar.appears_transparent);
    assert!(options.app_owns_titlebar_drag);

    // Elsewhere the window manager places its own buttons.
    let options = cx.update(|cx| {
        kiln_platform::window_options::window_options(&spec, &support::linux(), cx)
    });
    assert_eq!(options.titlebar.unwrap().traffic_light_position, None);
}
