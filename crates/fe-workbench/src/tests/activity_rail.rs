//! The activity rail on the window's right edge: its layout beside the
//! dock, clicks that switch and collapse panels, the highlight, tooltips
//! and icons.

use std::{fs, path::Path, time::Duration};

use fe_buffer::Buffer;
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, Keymap, KeymapLayer};
use fe_ui::{Icon, RAIL_WIDTH, ROW_HEIGHT, TAB_HEIGHT};
use gpui::{
    AssetSource as _, Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext, point,
};

use super::open;
use crate::{Dock, DockPanel, Launch, PANEL_WIDTH, Workbench, dock::DockOp};

/// A workbench showing `a.rs` from a temporary project, focused as the app
/// focuses it.
fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx, dir)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn dock(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Dock> {
    workbench.read_with(cx, |workbench, _| workbench.dock().clone())
}

/// Whether the dock shows, and the panel it shows then.
fn state(dock: &Entity<Dock>, cx: &mut VisualTestContext) -> (bool, DockPanel) {
    dock.read_with(cx, |dock, _| (dock.is_visible(), dock.active()))
}

fn click(dock: &Entity<Dock>, panel: DockPanel, cx: &mut VisualTestContext) {
    let button = dock.read_with(cx, |dock, _| dock.rail_button_bounds(panel).expect("drawn"));
    cx.simulate_click(button.center(), Modifiers::none());
    draw(cx);
}

fn editor_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor shows");
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

fn input_focused(dock: &Entity<Dock>, cx: &mut VisualTestContext) -> bool {
    let search = dock.read_with(cx, |dock, _| dock.search().clone());
    cx.update(|window, cx| {
        let input = search.read(cx).input().focus_handle(cx);
        input.is_focused(window)
    })
}

fn ops(dock: &Entity<Dock>, cx: &mut VisualTestContext) -> Vec<DockOp> {
    dock.read_with(cx, |dock, _| dock.ops().to_vec())
}

#[gpui::test]
fn the_rail_stays_when_the_dock_hides(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let dock = dock(&workbench, cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let window = cx.update(|window, _| window.viewport_size());
    let (rail, column) = dock.read_with(cx, |dock, _| {
        (dock.rail_bounds().unwrap(), dock.column_bounds().unwrap())
    });
    assert_eq!(rail.right(), window.width, "{rail:?}");
    assert_eq!(rail.size.width, RAIL_WIDTH);
    // Above the status bar.
    assert_eq!(rail.size.height, window.height - ROW_HEIGHT);
    assert_eq!(column.right(), rail.left(), "{column:?}");
    assert_eq!(column.size.width, PANEL_WIDTH);
    let right = pane.read_with(cx, |pane, _| pane.bounds().right());
    assert_eq!(right, column.left());

    cx.simulate_keystrokes("cmd-b");
    draw(cx);
    assert!(!state(&dock, cx).0);
    let (hidden, column) = dock.read_with(cx, |dock, _| (dock.rail_bounds(), dock.column_bounds()));
    assert_eq!(hidden, Some(rail));
    assert_eq!(column, None);
    let moved = pane.read_with(cx, |pane, _| pane.bounds().right());
    assert_eq!(moved, right + PANEL_WIDTH);
    assert_eq!(moved, rail.left());
}

#[gpui::test]
fn clicks_switch_and_collapse(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let dock = dock(&workbench, cx);
    assert_eq!(state(&dock, cx), (true, DockPanel::Explorer));

    click(&dock, DockPanel::Search, cx);
    assert_eq!(state(&dock, cx), (true, DockPanel::Search));
    assert!(input_focused(&dock, cx));

    // The highlighted panel's button hides the dock.
    click(&dock, DockPanel::Search, cx);
    assert!(!state(&dock, cx).0);
    assert!(editor_focused(&workbench, cx));

    // Any button shows it again.
    click(&dock, DockPanel::Explorer, cx);
    assert_eq!(state(&dock, cx), (true, DockPanel::Explorer));
    use DockOp::*;
    assert_eq!(ops(&dock, cx), [ShowSearch, Toggle, ShowProject]);

    // The panels' keys run the same ops; the outline's shows the Explorer.
    cx.simulate_keystrokes("cmd-shift-f cmd-shift-o cmd-shift-e");
    assert_eq!(ops(&dock, cx)[3..], [ShowSearch, ShowOutline, ShowProject]);
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    assert!(cx.update(|window, cx| panel.focus_handle(cx).is_focused(window)));
    cx.simulate_keystrokes("cmd-shift-f");
    click(&dock, DockPanel::Explorer, cx);
    assert!(
        cx.update(|window, cx| panel.focus_handle(cx).is_focused(window)),
        "as cmd-shift-e does"
    );
}

#[gpui::test]
fn only_the_shown_panel_is_highlighted(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let dock = dock(&workbench, cx);
    let highlighted = |cx: &mut VisualTestContext| {
        dock.read_with(cx, |dock, _| (dock.rail_highlighted(), dock.title()))
    };
    assert_eq!(
        highlighted(cx),
        (Some(DockPanel::Explorer), "Explorer".into())
    );
    cx.simulate_keystrokes("cmd-shift-f");
    assert_eq!(highlighted(cx), (Some(DockPanel::Search), "Search".into()));
    cx.simulate_keystrokes("cmd-b");
    assert_eq!(highlighted(cx).0, None);
    cx.simulate_keystrokes("cmd-shift-o");
    assert_eq!(
        highlighted(cx),
        (Some(DockPanel::Explorer), "Explorer".into())
    );

    // The title row is TAB_HEIGHT high, and a click on it does nothing.
    draw(cx);
    let column = dock.read_with(cx, |dock, _| dock.column_bounds().unwrap());
    let before = ops(&dock, cx);
    let title = point(column.center().x, column.top() + TAB_HEIGHT / 2.);
    cx.simulate_click(title, Modifiers::none());
    draw(cx);
    assert_eq!(state(&dock, cx), (true, DockPanel::Explorer));
    assert_eq!(ops(&dock, cx), before);
    assert!(editor_focused(&workbench, cx));

    // The header strip of tabs is gone.
    let source = include_str!("../dock.rs");
    assert!(!source.contains("TabBar"));
    assert!(!source.contains("tab_bounds"));
}

#[gpui::test]
fn tooltips_show_name_and_shortcut(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let dock = dock(&workbench, cx);
    let away = workbench.read_with(cx, |workbench, cx| {
        workbench.pane().read(cx).bounds().center()
    });
    let hover = |panel: DockPanel, cx: &mut VisualTestContext| {
        cx.simulate_mouse_move(away, None, Modifiers::none());
        draw(cx);
        let button = dock.read_with(cx, |dock, _| dock.rail_button_bounds(panel).expect("drawn"));
        let before = dock.read_with(cx, |dock, _| dock.last_tooltip());
        cx.simulate_mouse_move(button.center(), None, Modifiers::none());
        cx.executor().advance_clock(Duration::from_millis(400));
        draw(cx);
        let early = dock.read_with(cx, |dock, _| dock.last_tooltip());
        assert_eq!(early, before, "not shown before the delay");
        cx.executor().advance_clock(Duration::from_millis(100));
        draw(cx);
        dock.read_with(cx, |dock, _| dock.last_tooltip())
    };
    // The keymap is macOS's; its keys print as this platform prints them:
    // in symbols on macOS, spelled out elsewhere.
    let shown = |mac: &str, other: &str| {
        Some(
            if cfg!(target_os = "macos") {
                mac
            } else {
                other
            }
            .to_owned(),
        )
    };
    assert_eq!(
        hover(DockPanel::Explorer, cx),
        shown("Explorer ⇧⌘E", "Explorer Shift+Super+E")
    );
    assert_eq!(
        hover(DockPanel::Search, cx),
        shown("Search ⇧⌘F", "Search Shift+Super+F")
    );

    let keymap = |user: &str| {
        let (default, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
        let (user, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), user, None);
        assert_eq!(diagnostics, []);
        Keymap::from_layers([default.unwrap(), user.unwrap()])
    };
    let moved = keymap(
        r#"
        [[bindings]]
        keys = "alt-cmd-j"
        command = "search.show"
        context = "workbench"
        "#,
    );
    cx.update(|_, cx| fe_gpui::set_user_keymap(cx, moved));
    assert_eq!(
        hover(DockPanel::Search, cx),
        shown("Search ⌥⌘J", "Search Alt+Super+J")
    );

    let unbound = keymap(
        r#"
        [[bindings]]
        keys = "cmd-shift-f"
        command = ""
        context = "workbench"
        "#,
    );
    cx.update(|_, cx| fe_gpui::set_user_keymap(cx, unbound));
    assert_eq!(hover(DockPanel::Search, cx), Some("Search".to_owned()));
}

#[gpui::test]
fn buttons_draw_lucide_icons(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let dock = dock(&workbench, cx);
    let drawn =
        |cx: &mut VisualTestContext| dock.read_with(cx, |dock, _| dock.rail_icons().to_vec());
    // The Terminal button is pinned below the panels'.
    let icons = [
        Icon::Files,
        Icon::Search,
        Icon::GitBranch,
        crate::dock::panel_icon(DockPanel::GitLog),
        Icon::MessageSquare,
        Icon::FileTerminal,
    ];
    assert_eq!(drawn(cx), icons);
    // Drawn whether or not the dock shows.
    cx.simulate_keystrokes("cmd-b");
    draw(cx);
    assert_eq!(drawn(cx), icons);
    for icon in icons {
        let svg = fe_ui::Assets.load(icon.path()).unwrap();
        assert!(svg.is_some_and(|svg| svg.starts_with(b"<svg")), "{icon:?}");
    }
}
