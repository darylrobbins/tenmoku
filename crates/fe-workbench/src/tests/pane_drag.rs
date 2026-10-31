use std::fs;

use fe_project::fixture;
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels,
    Point, TestAppContext, VisualTestContext, point, px, size,
};

use super::{click_row, open_root, panel};
use crate::{
    Axis, Member, Pane, Workbench,
    pane_drag::{DropTarget, DropZone},
    pane_group::{MIN_PANE_HEIGHT, MIN_PANE_WIDTH},
};

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fixture::generate(dir.path(), 1, 1).unwrap();
    for (name, text) in [
        ("a.txt", "alpha\n"),
        ("b.rs", "fn b() {}\n"),
        ("c.md", "# c\n"),
        ("d.json", "{}\n"),
    ] {
        fs::write(dir.path().join(name), text).unwrap();
    }
    dir
}

fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    workbench.read_with(cx, |workbench, cx| {
        let group = workbench.pane_group(cx);
        group.panes().into_iter().cloned().collect()
    })
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

fn tabs(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> (Vec<String>, Option<usize>) {
    pane.read_with(cx, |pane, _| {
        let names = pane.tabs().iter();
        let names = names.map(|tab| crate::pane::file_name(tab.path.as_deref()));
        (names.collect(), pane.active())
    })
}

/// Each pane's tabs, in layout order.
fn layout(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
    panes(workbench, cx)
        .iter()
        .map(|pane| tabs(pane, cx).0)
        .collect()
}

/// The tree's shape: `P` for a pane, `H[…]` and `V[…]` for axes.
fn shape(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    fn member(of: &Member) -> String {
        match of {
            Member::Pane(_) => "P".into(),
            Member::Axis { axis, members } => {
                let inner: Vec<_> = members.iter().map(member).collect();
                let axis = if *axis == Axis::Horizontal { "H" } else { "V" };
                format!("{axis}[{}]", inner.join(", "))
            }
        }
    }
    workbench.read_with(cx, |workbench, cx| member(workbench.pane_group(cx).root()))
}

fn ratios(workbench: &Entity<Workbench>, path: &[usize], cx: &mut VisualTestContext) -> Vec<f32> {
    workbench.update(cx, |workbench, cx| {
        let ratios = workbench.pane_group(cx).ratios(path);
        ratios.expect("an axis there")
    })
}

fn moves(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, _| workbench.moves)
}

fn bounds(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    pane.read_with(cx, |pane, _| pane.bounds())
}

fn content(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    pane.read_with(cx, |pane, _| pane.content_bounds())
}

fn tab(pane: &Entity<Pane>, ix: usize, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    pane.read_with(cx, |pane, _| pane.tab_bounds(ix).expect("drawn"))
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn near(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() <= px(1.)
}

/// Presses at `from` and moves past the drag threshold, then to `to`.
fn hover(from: Point<Pixels>, to: Point<Pixels>, cx: &mut VisualTestContext) {
    let none = Modifiers::none();
    cx.simulate_mouse_down(from, MouseButton::Left, none);
    let start = point(from.x + px(3.), from.y + px(3.));
    cx.simulate_mouse_move(start, MouseButton::Left, none);
    cx.simulate_mouse_move(to, MouseButton::Left, none);
    draw(cx);
}

fn drag(from: Point<Pixels>, to: Point<Pixels>, cx: &mut VisualTestContext) {
    hover(from, to, cx);
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    draw(cx);
}

/// The middle of the border after `before`, along `axis`, at `across`.
fn border(before: Bounds<Pixels>, axis: Axis) -> Point<Pixels> {
    match axis {
        Axis::Horizontal => point(before.right() + px(0.5), before.center().y),
        Axis::Vertical => point(before.center().x, before.bottom() + px(0.5)),
    }
}

/// A workbench showing a.txt in two panes side by side.
fn two_panes(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let dir = project();
    let (workbench, cx) = open_root(cx, dir.path());
    click_row(&panel(&workbench, cx), "a.txt", cx);
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    assert_eq!(shape(&workbench, cx), "H[P, P]");
    (workbench, cx, dir)
}

#[gpui::test]
fn dragging_a_divider_resizes(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = two_panes(cx);
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    let (l, r) = (bounds(&left, cx), bounds(&right, cx));
    assert_eq!(r.left() - l.right(), px(1.), "{l:?} {r:?}");

    // Pressed 2 px left of the border's middle, at the handle's edge.
    let from = border(l, Axis::Horizontal);
    let from = point(from.x - px(2.), from.y);
    drag(from, point(from.x + px(100.), from.y), cx);
    let (l2, r2) = (bounds(&left, cx), bounds(&right, cx));
    assert!(near(l2.size.width, l.size.width + px(100.)), "{l:?} {l2:?}");
    assert!(near(r2.size.width, r.size.width - px(100.)), "{r:?} {r2:?}");
    let shares = ratios(&workbench, &[], cx);
    assert!((shares.iter().sum::<f32>() - 1.).abs() < 1e-5, "{shares:?}");
    assert_eq!(active(&workbench, cx), right, "the press activated no pane");

    // A wider window keeps the ratios.
    let window = cx.update(|window, _| window.viewport_size());
    cx.simulate_resize(size(window.width + px(400.), window.height));
    draw(cx);
    assert_eq!(ratios(&workbench, &[], cx), shares);
    let (l3, r3) = (bounds(&left, cx), bounds(&right, cx));
    let space = l3.size.width + r3.size.width;
    assert!(
        space > l2.size.width + r2.size.width + px(399.),
        "{l3:?} {r3:?}"
    );
    assert!(near(l3.size.width, space * shares[0]), "{l3:?} {shares:?}");
    assert!(near(r3.size.width, space * shares[1]), "{r3:?} {shares:?}");
}

#[gpui::test]
fn minimums_and_double_click(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = two_panes(cx);
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    let from = border(bounds(&left, cx), Axis::Horizontal);
    drag(from, point(px(0.), from.y), cx);
    assert!(
        near(bounds(&left, cx).size.width, MIN_PANE_WIDTH),
        "{:?}",
        bounds(&left, cx)
    );

    // The right pane split down: its border stops at the upper pane's
    // minimum height.
    cx.simulate_keystrokes("cmd-k cmd-\\");
    draw(cx);
    assert_eq!(shape(&workbench, cx), "H[P, V[P, P]]");
    assert_eq!(panes(&workbench, cx)[1], right);
    let from = border(bounds(&right, cx), Axis::Vertical);
    drag(from, point(from.x, px(0.)), cx);
    let upper = bounds(&right, cx);
    assert!(near(upper.size.height, MIN_PANE_HEIGHT), "{upper:?}");

    // The lower pane split right: the vertical axis is two panes wide, and
    // the root's border stops there.
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    assert_eq!(shape(&workbench, cx), "H[P, V[P, H[P, P]]]");
    let from = border(bounds(&left, cx), Axis::Horizontal);
    let window = cx.update(|window, _| window.viewport_size());
    drag(from, point(window.width, from.y), cx);
    let column = bounds(&right, cx).size.width;
    assert!(near(column, MIN_PANE_WIDTH * 2. + px(1.)), "{column:?}");
    let lower = &panes(&workbench, cx)[2..];
    for pane in lower {
        assert!(near(bounds(pane, cx).size.width, MIN_PANE_WIDTH));
    }

    // With the lower panes at 80/20, the column stops where the narrower
    // one is at its minimum.
    let from = border(bounds(&left, cx), Axis::Horizontal);
    drag(from, point(px(0.), from.y), cx);
    let lower: Vec<_> = panes(&workbench, cx)[2..].to_vec();
    let from = border(bounds(&lower[0], cx), Axis::Horizontal);
    drag(from, point(window.width, from.y), cx);
    let shares = ratios(&workbench, &[1, 1], cx);
    assert!(shares[1] < 0.3, "{shares:?}");
    let from = border(bounds(&left, cx), Axis::Horizontal);
    drag(from, point(window.width, from.y), cx);
    let narrow = bounds(&lower[1], cx).size.width;
    assert!(near(narrow, MIN_PANE_WIDTH), "{narrow:?}");
    assert!(bounds(&lower[0], cx).size.width > MIN_PANE_WIDTH * 2.);

    // Double-clicking the root's border evens it out.
    let at = border(bounds(&left, cx), Axis::Horizontal);
    for click_count in [1, 2] {
        cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
        });
    }
    draw(cx);
    assert_eq!(ratios(&workbench, &[], cx), [0.5, 0.5]);
    let (l, r) = (bounds(&left, cx), bounds(&right, cx));
    assert!(near(l.size.width, r.size.width), "{l:?} {r:?}");
}

#[gpui::test]
fn splits_and_closes_share_space(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = two_panes(cx);
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    let (l, r) = (bounds(&left, cx), bounds(&right, cx));
    let space = l.size.width + r.size.width;
    let from = border(l, Axis::Horizontal);
    drag(from, point(from.x + space * 0.7 - l.size.width, from.y), cx);
    let close = |got: Vec<f32>, want: &[f32]| {
        got.len() == want.len() && got.iter().zip(want).all(|(a, b)| (a - b).abs() < 0.005)
    };
    assert!(close(ratios(&workbench, &[], cx), &[0.7, 0.3]));

    cx.simulate_keystrokes("cmd-k cmd-left cmd-\\");
    draw(cx);
    assert_eq!(shape(&workbench, cx), "H[P, P, P]");
    let got = ratios(&workbench, &[], cx);
    assert!(close(got.clone(), &[0.35, 0.35, 0.3]), "{got:?}");
    let widths: Vec<_> = panes(&workbench, cx)
        .iter()
        .map(|pane| bounds(pane, cx).size.width)
        .collect();
    assert!(near(widths[0], widths[1]), "{widths:?}");

    // The middle pane, active, closes; its share goes back to the left one.
    cx.simulate_keystrokes("cmd-k cmd-w");
    draw(cx);
    assert_eq!(panes(&workbench, cx), [left.clone(), right.clone()]);
    let got = ratios(&workbench, &[], cx);
    assert!(close(got.clone(), &[0.7, 0.3]), "{got:?}");
    assert!(near(bounds(&left, cx).size.width, space * 0.7));
}

/// Left: a.txt and b.rs, b.rs active; right: c.md and d.json.
fn tab_panes(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let (workbench, cx, dir) = two_panes(cx);
    let panel = panel(&workbench, cx);
    click_row(&panel, "c.md", cx);
    click_row(&panel, "d.json", cx);
    cx.simulate_keystrokes("cmd-shift-] cmd-w cmd-k cmd-left");
    click_row(&panel, "b.rs", cx);
    draw(cx);
    assert_eq!(
        layout(&workbench, cx),
        [vec!["a.txt", "b.rs"], vec!["c.md", "d.json"]]
    );
    (workbench, cx, dir)
}

#[gpui::test]
fn dropping_on_a_tab_bar(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = tab_panes(cx);
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    let editor = left.read_with(cx, |pane, _| pane.tabs()[1].editor().unwrap().clone());

    // Over the left half of the right pane's second tab: the marker shows
    // at its left edge.
    let second = tab(&right, 1, cx);
    let to = point(second.left() + second.size.width / 4., second.center().y);
    hover(tab(&left, 1, cx).center(), to, cx);
    let hovered = right.read_with(cx, |pane, _| pane.hovered_drop());
    assert_eq!(hovered, Some(DropTarget::TabBar(1)));
    let marker = right.read_with(cx, |pane, _| pane.drop_hint_bounds());
    let marker = marker.expect("a marker shows");
    assert_eq!(marker.size.width, px(2.));
    assert!(
        near(marker.center().x, second.left()),
        "{marker:?} {second:?}"
    );
    assert_eq!(left.read_with(cx, |pane, _| pane.hovered_drop()), None);

    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    draw(cx);
    assert_eq!(
        tabs(&right, cx),
        (vec!["c.md".into(), "b.rs".into(), "d.json".into()], Some(1))
    );
    assert_eq!(tabs(&left, cx).0, ["a.txt"]);
    let moved = right.read_with(cx, |pane, _| pane.tabs()[1].editor().unwrap().clone());
    assert_eq!(moved, editor);
    assert_eq!(active(&workbench, cx), right);
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    assert_eq!(right.read_with(cx, |pane, _| pane.drop_hint_bounds()), None);

    // The left pane's last tab, dropped past the right's last tab, goes at
    // the end, and the emptied pane closes.
    let past = tab(&right, 2, cx);
    let to = point(past.right() + px(20.), past.center().y);
    drag(tab(&left, 0, cx).center(), to, cx);
    assert_eq!(panes(&workbench, cx), std::slice::from_ref(&right));
    assert_eq!(
        tabs(&right, cx),
        (
            vec![
                "c.md".into(),
                "b.rs".into(),
                "d.json".into(),
                "a.txt".into()
            ],
            Some(3)
        )
    );

    // A file the target shows already: its tab there activates, and the
    // dragged one closes.
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    let [right, other] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    assert_eq!(tabs(&other, cx).0, ["a.txt"]);
    drag(tab(&right, 3, cx).center(), tab(&other, 0, cx).center(), cx);
    assert_eq!(
        layout(&workbench, cx),
        [vec!["c.md", "b.rs", "d.json"], vec!["a.txt"]]
    );
    assert_eq!(active(&workbench, cx), other);

    // Dropped past the last tab of its own bar, a tab moves to the end.
    let past = tab(&right, 2, cx);
    let to = point(past.right() + px(20.), past.center().y);
    drag(tab(&right, 0, cx).center(), to, cx);
    assert_eq!(
        tabs(&right, cx),
        (vec!["b.rs".into(), "d.json".into(), "c.md".into()], Some(2))
    );
    assert_eq!(active(&workbench, cx), right);
}

#[gpui::test]
fn dropping_on_pane_edges(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = tab_panes(cx);
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");

    // Over the left pane's right quarter: the overlay covers its right
    // half, and a drop splits it right.
    let area = content(&left, cx);
    let to = point(area.right() - area.size.width / 10., area.center().y);
    hover(tab(&right, 1, cx).center(), to, cx);
    let hovered = left.read_with(cx, |pane, _| pane.hovered_drop());
    assert_eq!(hovered, Some(DropTarget::Content(DropZone::Right)));
    let overlay = left
        .read_with(cx, |pane, _| pane.drop_hint_bounds())
        .expect("shows");
    let pane = bounds(&left, cx);
    assert!(
        near(overlay.left(), pane.center().x),
        "{overlay:?} {pane:?}"
    );
    assert!(near(overlay.right(), pane.right()) && overlay.top() == pane.top());
    assert_eq!(overlay.size.height, pane.size.height);
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    draw(cx);
    assert_eq!(shape(&workbench, cx), "H[P, P, P]");
    assert_eq!(
        layout(&workbench, cx),
        [vec!["a.txt", "b.rs"], vec!["d.json"], vec!["c.md"]]
    );
    let split = panes(&workbench, cx)[1].clone();
    assert_eq!(active(&workbench, cx), split);

    // The top quarter: a new pane above.
    let area = content(&left, cx);
    let to = point(area.center().x, area.top() + area.size.height / 10.);
    hover(tab(&left, 0, cx).center(), to, cx);
    let hovered = left.read_with(cx, |pane, _| pane.hovered_drop());
    assert_eq!(hovered, Some(DropTarget::Content(DropZone::Up)));
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    draw(cx);
    assert_eq!(shape(&workbench, cx), "H[V[P, P], P, P]");
    assert_eq!(
        layout(&workbench, cx),
        [vec!["a.txt"], vec!["b.rs"], vec!["d.json"], vec!["c.md"]]
    );

    // The centre takes the tab in, at the end; the emptied pane closes.
    let above = panes(&workbench, cx)[0].clone();
    drag(
        tab(&above, 0, cx).center(),
        content(&right, cx).center(),
        cx,
    );
    assert_eq!(shape(&workbench, cx), "H[P, P, P]");
    assert_eq!(
        layout(&workbench, cx),
        [vec!["b.rs"], vec!["d.json"], vec!["c.md", "a.txt"]]
    );
    assert_eq!(tabs(&right, cx).1, Some(1));

    // On its own pane's centre, a tab moves to the end.
    drag(
        tab(&right, 0, cx).center(),
        content(&right, cx).center(),
        cx,
    );
    assert_eq!(
        tabs(&right, cx),
        (vec!["a.txt".into(), "c.md".into()], Some(1))
    );

    // A pane's only tab dropped on that pane changes nothing.
    let before = layout(&workbench, cx);
    let area = content(&split, cx);
    let targets = [
        area.center(),
        point(area.right() - px(5.), area.center().y),
        point(area.center().x, area.bottom() - px(5.)),
        tab(&split, 0, cx).center(),
        point(
            tab(&split, 0, cx).right() + px(20.),
            tab(&split, 0, cx).center().y,
        ),
    ];
    for to in targets {
        drag(tab(&split, 0, cx).center(), to, cx);
        assert_eq!(shape(&workbench, cx), "H[P, P, P]", "{to:?}");
        assert_eq!(layout(&workbench, cx), before, "{to:?}");
        assert_eq!(panes(&workbench, cx)[1], split);
    }
}

#[gpui::test]
fn drags_and_keys_share_move_tab(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = tab_panes(cx);
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");

    let before = moves(&workbench, cx);
    cx.simulate_keystrokes("ctrl-cmd-right");
    assert_eq!(moves(&workbench, cx), before + 1);
    assert_eq!(
        layout(&workbench, cx),
        [vec!["a.txt"], vec!["c.md", "d.json", "b.rs"]]
    );

    // a.txt, edited, dragged onto the right pane's bar.
    cx.simulate_keystrokes("cmd-k cmd-left");
    cx.simulate_input("x");
    draw(cx);
    let first = tab(&right, 0, cx);
    let to = point(first.left() + px(5.), first.center().y);
    drag(tab(&left, 0, cx).center(), to, cx);
    assert_eq!(moves(&workbench, cx), before + 2);
    assert_eq!(panes(&workbench, cx), std::slice::from_ref(&right));
    assert_eq!(
        tabs(&right, cx),
        (
            vec![
                "a.txt".into(),
                "c.md".into(),
                "d.json".into(),
                "b.rs".into()
            ],
            Some(0)
        )
    );
    let editor = right.read_with(cx, |pane, _| pane.tabs()[0].editor().unwrap().clone());
    let dirty =
        |cx: &mut VisualTestContext| editor.read_with(cx, |view, _| view.model().is_dirty());
    assert!(dirty(cx));
    assert_eq!(cx.window_title().as_deref(), Some("a.txt ● — Tenmoku"));

    cx.simulate_keystrokes("cmd-z");
    draw(cx);
    let text = editor.read_with(cx, |view, _| view.model().text().to_string());
    assert_eq!(text, "alpha\n");
}

#[test]
fn drop_zones_and_overlays() {
    let pane = Bounds::new(point(px(100.), px(0.)), size(px(400.), px(330.)));
    let area = Bounds::new(point(px(100.), px(30.)), size(px(400.), px(300.)));
    let at = |x: f32, y: f32| DropZone::at(area, point(px(100. + x), px(30. + y)));
    assert_eq!(at(10., 150.), DropZone::Left);
    assert_eq!(at(390., 150.), DropZone::Right);
    assert_eq!(at(200., 10.), DropZone::Up);
    assert_eq!(at(200., 290.), DropZone::Down);
    assert_eq!(at(200., 150.), DropZone::Centre);
    assert_eq!(at(101., 76.), DropZone::Centre);
    // Corners: the nearer edge, relative to the size, wins.
    assert_eq!(at(20., 30.), DropZone::Left);
    assert_eq!(at(40., 15.), DropZone::Up);
    assert_eq!(at(395., 280.), DropZone::Right);
    assert_eq!(at(380., 295.), DropZone::Down);

    let half =
        |x: f32, y: f32, w: f32, h: f32| Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
    assert_eq!(
        DropZone::Left.overlay(pane, area),
        half(100., 0., 200., 330.)
    );
    assert_eq!(
        DropZone::Right.overlay(pane, area),
        half(300., 0., 200., 330.)
    );
    assert_eq!(DropZone::Up.overlay(pane, area), half(100., 0., 400., 165.));
    assert_eq!(
        DropZone::Down.overlay(pane, area),
        half(100., 165., 400., 165.)
    );
    assert_eq!(DropZone::Centre.overlay(pane, area), area);
}
