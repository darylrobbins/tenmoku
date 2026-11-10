//! A breadcrumb pick drops a go-to-definition still waiting for its answer.

use fe_ui::CrumbKind;
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::{
    MAIN, active, at, caret_at, click_button, click_crumb, nav, opened, project, pushes, settle,
};
use crate::Workbench;

/// Starts F12 on `helper` with its answer held until the sender is dropped;
/// the caret it started from.
fn hold_goto(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (usize, async_channel::Sender<()>) {
    let (release, hold) = async_channel::bounded::<()>(1);
    workbench.update(cx, |workbench, _| {
        workbench.nav_state.hold_answer = Some(hold)
    });
    let origin = caret_at(workbench, "helper", 1, cx);
    cx.simulate_keystrokes("f12");
    settle(cx);
    (origin, release)
}

#[gpui::test]
fn a_symbol_pick_drops_a_running_goto_definition(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (workbench, cx) = opened(cx, &root);
    let pane = active(&workbench, cx);
    let main = root.join("src/main.rs");
    let (origin, release) = hold_goto(&workbench, cx);

    click_crumb(&pane, CrumbKind::Symbol, "b", cx);
    cx.simulate_keystrokes("up enter");
    settle(cx);
    let picked = at(&workbench, cx);
    assert_eq!(picked, (main.clone(), MAIN.find("a()").unwrap()));
    let before = pushes(&workbench, cx);

    drop(release);
    settle(cx);
    assert_eq!(at(&workbench, cx), picked, "the held answer was dropped");
    assert_eq!(pushes(&workbench, cx), before, "and recorded nothing");

    assert_eq!(nav(&workbench, cx), (true, false));
    click_button(false, cx);
    assert_eq!(at(&workbench, cx), (main, origin));
    click_button(true, cx);
    assert_eq!(at(&workbench, cx), picked);
}

#[gpui::test]
fn an_entry_pick_of_the_shown_file_drops_a_running_goto_definition(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (workbench, cx) = opened(cx, &root);
    let pane = active(&workbench, cx);
    let main = root.join("src/main.rs");
    let (origin, release) = hold_goto(&workbench, cx);

    click_crumb(&pane, CrumbKind::File, "main.rs", cx);
    cx.simulate_keystrokes("m a i n enter");
    settle(cx);
    let picked = at(&workbench, cx);
    assert_eq!(picked, (main, origin), "the same editor is kept");
    let before = pushes(&workbench, cx);

    drop(release);
    settle(cx);
    assert_eq!(at(&workbench, cx), picked, "the held answer was dropped");
    assert_eq!(pushes(&workbench, cx), before, "and recorded nothing");
}
