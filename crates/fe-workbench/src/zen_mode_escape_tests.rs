//! `escape escape` leaves Zen Mode, while a lone escape still does its own
//! work at once: closing the find bar, or nothing visible without one.

use gpui::{TestAppContext, VisualTestContext};

use crate::zen_mode_tests::{draw, keys, opened, pane, zen};

#[gpui::test]
fn esc_esc_leaves(cx: &mut TestAppContext) {
    let (workbench, cx) = opened(cx);
    let pane = pane(&workbench, cx);
    let find_open = |cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, _| pane.find_bar().unwrap().is_open())
    };

    keys("cmd-k z", cx);
    assert!(zen(&workbench, cx));

    // With no find bar and no selection, a lone escape does nothing visible,
    // and a second leaves.
    keys("escape", cx);
    assert!(zen(&workbench, cx));
    keys("escape", cx);
    assert!(!zen(&workbench, cx));

    // Two escapes leave it.
    keys("cmd-k z", cx);
    keys("escape escape", cx);
    assert!(!zen(&workbench, cx));

    // A lone escape closes an open find bar at once and doesn't leave.
    keys("cmd-k z", cx);
    keys("cmd-f", cx);
    assert!(find_open(cx));
    keys("escape", cx);
    assert!(!find_open(cx));
    assert!(zen(&workbench, cx));
    draw(cx);
    assert!(zen(&workbench, cx));

    // With the find bar open, two escapes close it and leave: the first
    // moves focus, and the chord carries on from where it lands.
    keys("cmd-f", cx);
    assert!(find_open(cx));
    keys("escape escape", cx);
    assert!(!find_open(cx));
    assert!(!zen(&workbench, cx));
}
