//! The window's frame under client-side decorations: the title, the
//! buttons and the resize edges, and nothing of them otherwise.

use std::{cell::RefCell, rc::Rc};

use fe_command::{Invocation, Source};
use gpui::{Modifiers, TestAppContext, Tiling};

use super::open;
use crate::Launch;

fn launch() -> Launch {
    Launch {
        root: Some(std::env::temp_dir()),
        file: None,
    }
}

#[gpui::test]
fn client_decorations_draw_the_title_and_buttons(cx: &mut TestAppContext) {
    let (workbench, cx, _) = open(cx, launch());
    let seen = Rc::new(RefCell::new(Vec::<Invocation>::new()));
    cx.update(|_, cx| {
        let sink = seen.clone();
        fe_gpui::commands::observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone());
        });
    });

    // Server-side decorations, as the test platform has: nothing drawn.
    workbench.update(cx, |workbench, cx| workbench.set_window_frame(true, cx));
    cx.run_until_parked();
    assert_eq!(cx.debug_bounds("window-close"), None);
    assert_eq!(
        cx.debug_bounds("window-drag"),
        None,
        "no row without a menu bar"
    );

    // Client-side ones, untiled: the row, its title and three buttons.
    workbench.update(cx, |workbench, cx| {
        workbench.client_tiling = Some(Tiling::default());
        cx.notify();
    });
    cx.run_until_parked();
    for button in ["window-minimize", "window-maximize", "window-close"] {
        assert!(cx.debug_bounds(button).is_some(), "{button}");
    }
    let drag = cx.debug_bounds("window-drag").unwrap();
    let close = cx.debug_bounds("window-close").unwrap();
    assert!(
        close.left() >= drag.right(),
        "the buttons are right of the title"
    );

    // The buttons run the window commands, from the mouse.
    cx.simulate_click(close.center(), Modifiers::none());
    cx.run_until_parked();
    let runs: Vec<_> = seen
        .borrow()
        .iter()
        .map(|invocation| (invocation.command.as_str().to_owned(), invocation.source))
        .collect();
    assert_eq!(runs, [("window.close".to_owned(), Source::Mouse)]);
}

#[gpui::test]
fn the_frame_is_off_unless_the_host_asks(cx: &mut TestAppContext) {
    let (workbench, cx, _) = open(cx, launch());
    workbench.update(cx, |workbench, cx| {
        workbench.client_tiling = Some(Tiling::default());
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(cx.debug_bounds("window-close"), None);
}
