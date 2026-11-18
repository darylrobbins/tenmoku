//! The New menu's `+` and the empty panel with no folder open (T680): both
//! offer Open Folder…, and a hover tooltip doesn't read as a menu entry.

use std::time::Duration;

use gpui::{Modifiers, TestAppContext, VisualTestContext};

use super::{NEW_BUTTON, bounds, button, click_plus, draw, items, panel};
use crate::{Launch, Workbench, project_panel::new_menu::NewItem, workbench::toast::Level};

fn no_folder(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<Workbench>,
    gpui::Entity<crate::ProjectPanel>,
    &mut VisualTestContext,
) {
    let (workbench, cx, _) = crate::tests::open(
        cx,
        Launch {
            root: None,
            file: None,
        },
    );
    cx.update(|window, _| window.activate_window());
    draw(cx);
    let panel = panel(&workbench, cx);
    (workbench, panel, cx)
}

/// How many error toasts show: an Open Folder… that ran, with the open
/// panel made to fail.
fn errors(workbench: &gpui::Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .filter(|entry| entry.toast.level == Level::Error)
            .count()
    })
}

#[gpui::test]
fn plus_with_no_folder_offers_open_folder(cx: &mut TestAppContext) {
    let (workbench, panel, cx) = no_folder(cx);
    click_plus(cx);
    let drawn = panel.read_with(cx, |panel, cx| panel.new_menu_drawn(cx));
    let titles: Option<Vec<String>> =
        drawn.map(|drawn| drawn.into_iter().map(|(title, _)| title).collect());
    assert_eq!(titles, Some(vec!["Open Folder…".to_owned()]));
    // Choosing it runs workbench.open_folder.
    crate::workbench::OPEN_PANEL_FAILS.set(Some("no portal"));
    super::choose("Open Folder…", cx);
    assert_eq!(items(&panel, cx), None);
    assert_eq!(errors(&workbench, cx), 1);
}

#[gpui::test]
fn the_empty_panel_button_runs_open_folder(cx: &mut TestAppContext) {
    let (workbench, _panel, cx) = no_folder(cx);
    let open = bounds("project-open-folder", cx);
    crate::workbench::OPEN_PANEL_FAILS.set(Some("no portal"));
    cx.simulate_click(open.center(), Modifiers::none());
    draw(cx);
    assert_eq!(errors(&workbench, cx), 1);
}

#[gpui::test]
fn the_plus_tooltip_is_not_a_menu_item(cx: &mut TestAppContext) {
    let (workbench, panel, cx) = no_folder(cx);
    let plus = button(NEW_BUTTON, cx);
    cx.simulate_mouse_move(plus.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(1000));
    draw(cx);
    let tooltip = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).state().drawn().last_tooltip()
    });
    let tooltip = tooltip.expect("a tooltip shows");
    assert_ne!(tooltip, "New…");
    click_plus(cx);
    let drawn = panel
        .read_with(cx, |panel, cx| panel.new_menu_drawn(cx))
        .unwrap();
    assert!(
        drawn
            .iter()
            .all(|(title, _)| !tooltip.starts_with(title.as_str())),
        "{tooltip} {drawn:?}"
    );
}

#[gpui::test]
fn the_menu_stays_open_as_the_pointer_moves_onto_it(cx: &mut TestAppContext) {
    let dir = super::project(&[("a.txt", "")]);
    let (_workbench, panel, cx) = super::workbench(cx, &dir);
    click_plus(cx);
    let plus = button(NEW_BUTTON, cx);
    let item = bounds("menu-item:File", cx);
    // Down the way from the + to the first item, in steps.
    for step in 0..=4 {
        let t = step as f32 / 4.;
        let at = plus.center() + (item.center() - plus.center()) * t;
        cx.simulate_mouse_move(at, None, Modifiers::none());
        cx.executor().advance_clock(Duration::from_millis(100));
        draw(cx);
    }
    assert!(items(&panel, cx).is_some(), "the menu stays open");
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.new_menu_highlighted()),
        Some(NewItem::File)
    );
}
