//! Toasts in B3 (`docs/design/design-system.md`, Notifications and
//! agents): 340 px, lower right inside the editor area and above the
//! bottom panel, on the raised surface with the float shadow, an icon in the
//! level's colour, no coloured border, at most two actions, and a dismiss
//! with an accessible name.

use std::fs;

use fe_buffer::Buffer;
use fe_command::CommandId;
use fe_ui::ActiveTheme as _;
use fe_ui::{SHADOW_FLOAT, radius};
use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext, point, px};
use kiln_shell::DockView as _;

use super::open;
use crate::{
    Launch, Workbench,
    workbench::toast::{Level, MAX_ACTIONS, Toast, card_look, level_icon, show},
};

fn opened(cx: &mut TestAppContext) -> (tempfile::TempDir, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.fake_sessions = true);
    cx.update(|window, _| window.activate_window());
    draw(cx);
    (dir, cx)
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx)
        });
    }
}

fn card(cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.debug_bounds("toast-0").expect("the toast is drawn")
}

fn area(cx: &mut VisualTestContext) -> Bounds<Pixels> {
    let workbench = cx.windows()[0].downcast::<Workbench>().unwrap();
    let workbench = workbench.entity(cx).unwrap();
    workbench.read_with(cx, |workbench, cx| {
        workbench.editor_area(cx).expect("the panes are laid out")
    })
}

#[gpui::test]
fn a_toast_is_340_px_wide_12_px_inside_the_editor_areas_bottom_right(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    cx.update(|_, cx| show(cx, Toast::info("Saved")));
    draw(cx);
    let (card, area) = (card(cx), area(cx));
    assert_eq!(card.size.width, px(340.));
    assert_eq!(card.right(), area.right() - px(12.));
    assert_eq!(card.bottom(), area.bottom() - px(12.));
}

#[gpui::test]
fn it_stays_above_the_bottom_panel_and_off_the_rail_and_docks(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    cx.update(|_, cx| show(cx, Toast::info("Saved")));
    draw(cx);
    let (card, area) = (card(cx), area(cx));
    let workbench = cx.windows()[0].downcast::<Workbench>().unwrap();
    let workbench = workbench.entity(cx).unwrap();
    let (rail, side, bottom) = workbench.read_with(cx, |workbench, cx| {
        let sidebar = workbench.dock().read(cx);
        let drawn = sidebar.state().drawn();
        let bottom = workbench
            .terminal_dock()
            .read(cx)
            .dock()
            .drawn()
            .column_bounds();
        (drawn.rail_bounds(), drawn.column_bounds(), bottom)
    });
    for other in [rail, side, bottom].into_iter().flatten() {
        assert!(!card.intersects(&other), "{card:?} over {other:?}");
    }
    assert!(area.contains(&card.origin) && area.contains(&point(card.right(), card.bottom())));
}

#[gpui::test]
fn the_card_is_the_raised_surface_with_the_float_shadow_and_radius_10(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    let (colors, look) = cx.update(|_, cx| {
        let colors = cx.theme().colors;
        (colors, card_look(&colors))
    });
    assert_eq!(look.fill, colors.surface.raised);
    assert_eq!(look.radius, radius::FLOAT);
    assert_eq!(look.radius, px(10.));
    assert_eq!(look.shadow.offset, point(px(0.), SHADOW_FLOAT.y));
    assert_eq!(look.shadow.blur_radius, SHADOW_FLOAT.blur);
}

#[gpui::test]
fn the_icons_colour_follows_the_level(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    let colors = cx.update(|_, cx| cx.theme().colors);
    let of = |level| level_icon(level, &colors).1;
    assert_eq!(of(Level::Error), colors.status.error);
    assert_eq!(of(Level::Warning), colors.status.warning);
    assert_eq!(of(Level::Info), colors.status.info);
    assert_eq!(of(Level::Success), colors.status.success);
    let icons: Vec<_> = [Level::Error, Level::Warning, Level::Info, Level::Success]
        .map(|level| level_icon(level, &colors).0.name())
        .into();
    let mut unique = icons.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), icons.len(), "{icons:?}");
    assert_eq!(Toast::success("Saved").level, Level::Success);
    cx.update(|_, cx| {
        show(
            cx,
            Toast::warning("Couldn't save").detail("Permission denied"),
        )
    });
    draw(cx);
    assert!(cx.debug_bounds("toast-icon-0").is_some());
}

#[gpui::test]
fn the_dismiss_button_has_an_accessible_name(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    cx.update(|_, cx| show(cx, Toast::info("Saved")));
    draw(cx);
    let nodes = cx.update(|window, cx| fe_ui::nodes(window, cx));
    let labels: Vec<String> = (nodes.iter())
        .filter(|node| node.role == fe_ui::Role::Button)
        .filter(|node| node.id.to_string().contains("toast-close"))
        .map(|node| node.label.to_string())
        .collect();
    assert_eq!(labels, ["Dismiss"]);
}

#[test]
fn two_actions_fit_and_a_third_is_a_debug_assertion() {
    assert_eq!(MAX_ACTIONS, 2);
    let command = CommandId::parse("view.toggle_breadcrumbs").unwrap();
    let toast = Toast::info("x").action("A", command);
    assert_eq!(toast.action_count(), 1);
    let two = Toast::info("x").choices(vec!["A".into(), "B".into()], |_| {});
    assert_eq!(two.action_count(), 2);
    let three = Toast::info("x")
        .action("A", CommandId::parse("view.toggle_breadcrumbs").unwrap())
        .choices(vec!["B".into(), "C".into()], |_| {});
    assert_eq!(three.action_count(), 3);
}

#[gpui::test]
#[should_panic(expected = "room for 2 actions")]
fn drawing_a_toast_with_three_actions_asserts(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    let command = CommandId::parse("view.toggle_breadcrumbs").unwrap();
    let three =
        (Toast::info("x").action("A", command)).choices(vec!["B".into(), "C".into()], |_| {});
    cx.update(|_, cx| show(cx, three));
    draw(cx);
}

/// Asserts the toast is 12 px inside the editor area's bottom right as the
/// last frame laid it out, never over a dock or the rail.
fn assert_placed(cx: &mut VisualTestContext, what: &str) {
    let (card, area) = (card(cx), area(cx));
    let width = (area.size.width - px(24.)).min(px(340.)).max(px(0.));
    assert_eq!(card.size.width, width, "{what}");
    assert_eq!(card.right(), area.right() - px(12.), "{what}");
    assert_eq!(card.bottom(), area.bottom() - px(12.), "{what}");
}

/// One frame, as the first to follow a change.
fn one_frame(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn workbench(cx: &mut VisualTestContext) -> gpui::Entity<Workbench> {
    let window = cx.windows()[0].downcast::<Workbench>().unwrap();
    window.entity(cx).unwrap()
}

/// Shows or hides the sidebar, without drawing.
fn toggle_sidebar(cx: &mut VisualTestContext) {
    let dock = workbench(cx).read_with(cx, |workbench, _| workbench.dock().clone());
    dock.update(cx, |dock, cx| {
        dock.dock_mut().toggle();
        cx.notify();
    });
}

/// Shows or hides the bottom panel, without drawing.
fn toggle_bottom(cx: &mut VisualTestContext) {
    let dock = workbench(cx).read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    dock.update(cx, |dock, cx| dock.toggle(cx));
}

#[gpui::test]
fn it_follows_a_dock_opening_and_closing_in_the_same_frame(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    cx.update(|_, cx| show(cx, Toast::info("Saved")));
    draw(cx);
    for what in ["the sidebar hides", "the sidebar shows"] {
        let before = area(cx);
        toggle_sidebar(cx);
        one_frame(cx);
        assert_ne!(area(cx), before, "{what}");
        assert_placed(cx, what);
    }
}

#[gpui::test]
fn it_follows_a_dock_resizing_in_the_same_frame(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    cx.update(|_, cx| show(cx, Toast::info("Saved")));
    draw(cx);
    let workbench = cx.windows()[0].downcast::<Workbench>().unwrap();
    let workbench = workbench.entity(cx).unwrap();
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    let before = area(cx);
    for width in [px(400.), px(300.)] {
        dock.update(cx, |dock, cx| {
            dock.dock_mut().set_size(width);
            cx.notify();
        });
        one_frame(cx);
        assert_placed(cx, "the sidebar resizes");
    }
    assert_ne!(area(cx), before);
}

#[gpui::test]
fn it_follows_the_bottom_panel_toggling_in_the_same_frame(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    cx.update(|_, cx| show(cx, Toast::info("Saved")));
    draw(cx);
    for what in ["the panel opens", "the panel closes"] {
        let before = area(cx);
        toggle_bottom(cx);
        one_frame(cx);
        assert_ne!(area(cx), before, "{what}");
        assert_placed(cx, what);
    }
}

#[gpui::test]
fn a_narrow_editor_area_narrows_the_card_to_its_width_less_both_margins(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    cx.simulate_resize(gpui::size(px(420.), px(400.)));
    cx.update(|_, cx| show(cx, Toast::info("Saved")));
    draw(cx);
    let area = area(cx);
    assert!(area.size.width < px(340.), "{area:?}");
    assert_placed(cx, "a narrow area");
    assert!(card(cx).size.width <= area.size.width - px(24.));
}

#[gpui::test]
fn the_card_narrows_in_the_same_frame_a_dock_narrows_the_area(cx: &mut TestAppContext) {
    let (_dir, cx) = opened(cx);
    cx.simulate_resize(gpui::size(px(560.), px(400.)));
    cx.update(|_, cx| show(cx, Toast::info("Saved")));
    draw(cx);
    // With the sidebar hidden, whichever way it started.
    if card(cx).size.width < px(340.) {
        toggle_sidebar(cx);
        draw(cx);
    }
    let wide = card(cx);
    assert_eq!(wide.size.width, px(340.), "room for the whole card");
    toggle_sidebar(cx);
    one_frame(cx);
    let (card, area) = (card(cx), area(cx));
    assert!(area.size.width < px(340.) + px(24.), "{area:?}");
    assert!(
        area.contains(&card.origin) && area.contains(&card.bottom_right()),
        "{card:?} outside {area:?}"
    );
    assert_placed(cx, "the dock opening narrows the area");
}
