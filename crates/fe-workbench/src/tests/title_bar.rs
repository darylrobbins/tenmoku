//! The unified title bar: opt-in, the root's name in the project widget,
//! the window dragged and zoomed from its empty space.

use std::path::Path;

use gpui::{
    Bounds, Entity, Modifiers, MouseButton, MouseDownEvent, Pixels, Point, Size, TestAppContext,
    VisualTestContext, point, px,
};

use super::open_root;
use crate::{
    TITLE_BAR_HEIGHT, TRAFFIC_LIGHT_INSET, Workbench,
    title_bar::{REQUESTS, Request},
};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A temporary directory with a `demo` project in it, and a file in that.
fn demo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("demo")).unwrap();
    std::fs::write(dir.path().join("demo/a.rs"), "fn a() {}\n").unwrap();
    dir
}

fn opened<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    title_bar: bool,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, root);
    let file = root.join("a.rs");
    std::fs::write(&file, "fn a() {}\n").unwrap();
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(file, window, cx)));
    if title_bar {
        workbench.update(cx, |workbench, cx| workbench.set_title_bar(true, cx));
    }
    draw(cx);
    (workbench, cx)
}

fn window(cx: &mut VisualTestContext) -> Size<Pixels> {
    cx.update(|window, _| window.viewport_size())
}

fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.debug_bounds(selector).expect(selector)
}

fn pane_top(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Pixels {
    workbench.read_with(cx, |workbench, cx| {
        workbench.pane().read(cx).bounds().origin.y
    })
}

fn name(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    workbench.read_with(cx, |workbench, _| workbench.title_bar_name().to_string())
}

fn near(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() <= px(1.)
}

fn mouse_down(at: Point<Pixels>, click_count: usize, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count,
        first_mouse: false,
    });
}

fn requests() -> Vec<Request> {
    REQUESTS.with_borrow_mut(std::mem::take)
}

#[gpui::test]
fn off_by_default(cx: &mut TestAppContext) {
    let dir = demo();
    let (workbench, cx) = opened(cx, &dir.path().join("demo"), false);
    assert_eq!(cx.debug_bounds("title-bar"), None);
    assert_eq!(cx.debug_bounds("title-bar-label"), None);
    assert_eq!(cx.debug_bounds("title-project"), None);
    assert_eq!(pane_top(&workbench, cx), px(0.));
}

#[gpui::test]
fn shows_the_root_name(cx: &mut TestAppContext) {
    let dir = demo();
    let (workbench, cx) = opened(cx, &dir.path().join("demo"), true);
    let size = window(cx);
    let bar = bounds("title-bar", cx);
    assert_eq!((bar.origin.x, bar.origin.y), (px(0.), px(0.)));
    assert_eq!(bar.size.width, size.width);
    assert_eq!(bar.size.height, TITLE_BAR_HEIGHT);
    assert_eq!(TITLE_BAR_HEIGHT, px(38.));
    assert_eq!(name(&workbench, cx), "demo");
    // The project widget reads it, after the traffic lights, in place of a
    // centred title.
    assert_eq!(cx.debug_bounds("title-bar-label"), None);
    let project = bounds("title-project", cx);
    assert!(project.left() > TRAFFIC_LIGHT_INSET, "{project:?}");
    assert!(near(project.center().y, bar.center().y), "{project:?}");
    assert_eq!(pane_top(&workbench, cx), TITLE_BAR_HEIGHT);
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    let (rail, column) = dock.read_with(cx, |dock, _| {
        (dock.rail_bounds().unwrap(), dock.column_bounds().unwrap())
    });
    assert_eq!(rail.origin.y, TITLE_BAR_HEIGHT);
    assert_eq!(column.origin.y, TITLE_BAR_HEIGHT);
    assert_eq!(bounds("status-bar", cx).bottom(), size.height);

    // Another root retitles it.
    let other = dir.path().join("other");
    std::fs::create_dir(&other).unwrap();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(other, window, cx))
    });
    draw(cx);
    assert_eq!(name(&workbench, cx), "other");

    // A long name stays out of the traffic lights and clear of search and
    // settings, in a narrow window.
    let long = dir.path().join("x".repeat(100));
    std::fs::create_dir(&long).unwrap();
    cx.simulate_resize(gpui::size(px(300.), px(400.)));
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(long, window, cx))
    });
    draw(cx);
    let left = bounds("title-bar-left", cx);
    assert!(left.left() >= TRAFFIC_LIGHT_INSET - px(1.), "{left:?}");
    let search = bounds("icon-button:title-search", cx);
    assert!(left.right() <= search.left(), "{left:?} {search:?}");
    assert!(
        bounds("icon-button:title-settings", cx).right() <= px(300.),
        "settings still shows"
    );
}

#[gpui::test]
fn no_root_reads_tenmoku(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let (workbench, cx) = cx.add_window_view(|window, cx| {
        let theme = fe_gpui::EditorTheme::default_dark(cx);
        Workbench::new(
            crate::Launch {
                root: None,
                file: None,
            },
            theme,
            fe_gpui::EditorSettings::default(),
            window,
            cx,
        )
    });
    workbench.update(cx, |workbench, cx| workbench.set_title_bar(true, cx));
    draw(cx);
    assert_eq!(name(&workbench, cx), "Tenmoku");
}

#[gpui::test]
fn drag_and_double_click(cx: &mut TestAppContext) {
    let dir = demo();
    let (workbench, cx) = opened(cx, &dir.path().join("demo"), true);
    requests();
    let bar = bounds("title-bar", cx);
    let left = bounds("title-bar-left", cx);
    let right = bounds("title-bar-right", cx);
    // Between the left and right items.
    let empty = point((left.right() + right.left()) / 2., bar.center().y);
    assert!(left.right() + px(20.) < right.left());

    mouse_down(empty, 1, cx);
    assert_eq!(requests(), [Request::Move]);
    mouse_down(empty, 2, cx);
    assert_eq!(requests(), [Request::DoubleClick]);
    // Left of the items, by the traffic lights.
    mouse_down(point(px(10.), bar.center().y), 1, cx);
    assert_eq!(requests(), [Request::Move]);

    // The items are buttons: a press on one doesn't move the window.
    for selector in [
        "icon-button:title-back",
        "title-project",
        "icon-button:title-settings",
    ] {
        mouse_down(bounds(selector, cx).center(), 1, cx);
        mouse_down(bounds(selector, cx).center(), 2, cx);
        assert_eq!(requests(), [], "{selector}");
    }

    let tab = workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).tab_bounds(0));
    mouse_down(tab.expect("a tab").center(), 1, cx);
    let pane = workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).bounds());
    mouse_down(pane.center(), 1, cx);
    mouse_down(pane.center(), 2, cx);
    assert_eq!(requests(), []);
}
