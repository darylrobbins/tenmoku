// SPDX-License-Identifier: LicenseRef-Proprietary

//! Resizing a pane by a size, evening out every axis, and stepping through
//! the panes in order.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, div, px, size,
};
use kiln_shell::{
    Axis, ItemPane, Member, PaneGroup,
    pane_group::{MIN_PANE_HEIGHT, MIN_PANE_WIDTH},
};
use kiln_ui::UiTheme;

struct Host {
    group: PaneGroup<ItemPane>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = UiTheme::default_dark();
        div()
            .size_full()
            .flex()
            .child(self.group.render(&ui, |host| &mut host.group, cx))
    }
}

struct Opened<'a> {
    host: Entity<Host>,
    panes: Vec<Entity<ItemPane>>,
    cx: &'a mut VisualTestContext,
}

impl Opened<'_> {
    fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn resize(&mut self, pane: usize, axis: Axis, delta: f32) -> bool {
        let pane = self.panes[pane].clone();
        let resized = self
            .host
            .update(self.cx, |host, _| host.group.resize(&pane, axis, px(delta)));
        self.draw();
        resized
    }

    fn ratios(&mut self, path: &[usize]) -> Vec<f32> {
        self.host
            .read_with(self.cx, |host, _| host.group.ratios(path).unwrap())
    }

    /// The pane `n` steps from `from` in the order of [`PaneGroup::panes`].
    fn next(&mut self, from: usize, forward: bool) -> Option<usize> {
        let from = self.panes[from].clone();
        let found = self
            .host
            .read_with(self.cx, |host, _| host.group.next(&from, forward));
        found.map(|found| self.panes.iter().position(|pane| *pane == found).unwrap())
    }
}

/// A host of `count` panes in a row, in a window 801 px wide, drawn. With
/// `nested`, the last two stack, as left | (top / bottom).
fn open(cx: &mut TestAppContext, count: usize, nested: bool) -> Opened<'_> {
    let (host, cx) = cx.add_window_view(|_, cx| {
        let pane = cx.new(ItemPane::new);
        Host {
            group: PaneGroup::new(pane),
        }
    });
    let panes: Vec<_> = (0..count)
        .map(|_| host.update(cx, |_, cx| cx.new(ItemPane::new)))
        .collect();
    let even = |n: usize| vec![1. / n as f32; n];
    let root = if nested {
        let stack = Member::axis(
            Axis::Vertical,
            vec![
                Member::Pane(panes[1].clone()),
                Member::Pane(panes[2].clone()),
            ],
            even(2),
        );
        Member::axis(
            Axis::Horizontal,
            vec![Member::Pane(panes[0].clone()), stack],
            even(2),
        )
    } else {
        let members = panes.iter().cloned().map(Member::Pane).collect();
        Member::axis(Axis::Horizontal, members, even(count))
    };
    host.update(cx, |host, _| host.group.set_root(root));
    cx.simulate_resize(size(px(801.), px(601.)));
    let mut opened = Opened { host, panes, cx };
    opened.draw();
    opened
}

#[gpui::test]
fn resize_moves_the_border_with_the_next_sibling(cx: &mut TestAppContext) {
    let mut s = open(cx, 2, false);
    assert!(s.resize(0, Axis::Horizontal, 32.));
    let ratios = s.ratios(&[]);
    assert!((ratios[0] * 800. - 432.).abs() < 1., "{ratios:?}");
    assert!((ratios[1] * 800. - 368.).abs() < 1., "{ratios:?}");
    // The last pane takes from, and gives to, its previous sibling.
    assert!(s.resize(1, Axis::Horizontal, 32.));
    let ratios = s.ratios(&[]);
    assert!((ratios[0] * 800. - 400.).abs() < 1., "{ratios:?}");
    assert!(s.resize(1, Axis::Horizontal, -32.));
    let ratios = s.ratios(&[]);
    assert!((ratios[1] * 800. - 368.).abs() < 1., "{ratios:?}");
}

#[gpui::test]
fn resize_without_an_axis_or_a_frame_changes_nothing(cx: &mut TestAppContext) {
    let mut s = open(cx, 2, false);
    let before = s.ratios(&[]);
    assert!(!s.resize(0, Axis::Vertical, 32.));
    assert_eq!(s.ratios(&[]), before);

    // Before any frame, there are no sizes.
    let (host, cx) = cx.add_window_view(|_, cx| {
        let pane = cx.new(ItemPane::new);
        Host {
            group: PaneGroup::new(pane),
        }
    });
    let (a, b) = host.update(cx, |host, cx| {
        let a = host.group.panes()[0].clone();
        let b = cx.new(ItemPane::new);
        host.group
            .split(&a, b.clone(), kiln_shell::Direction::Right);
        (a, b)
    });
    let _ = b;
    let resized = host.update(cx, |host, _| {
        host.group.resize(&a, Axis::Horizontal, px(32.))
    });
    assert!(!resized);
    assert_eq!(
        host.read_with(cx, |host, _| host.group.ratios(&[])),
        Some(vec![0.5, 0.5])
    );
}

#[gpui::test]
fn resize_stops_at_the_minimums(cx: &mut TestAppContext) {
    let mut s = open(cx, 2, false);
    assert!(s.resize(0, Axis::Horizontal, 10_000.));
    let ratios = s.ratios(&[]);
    assert!(
        (ratios[1] * 800. - f32::from(MIN_PANE_WIDTH)).abs() < 1.,
        "{ratios:?}"
    );
    assert!(!s.resize(0, Axis::Horizontal, 32.));
    assert!(s.resize(0, Axis::Horizontal, -10_000.));
    let ratios = s.ratios(&[]);
    assert!(
        (ratios[0] * 800. - f32::from(MIN_PANE_WIDTH)).abs() < 1.,
        "{ratios:?}"
    );

    let mut s = open(cx, 3, true);
    assert!(s.resize(1, Axis::Vertical, 10_000.));
    let ratios = s.ratios(&[1]);
    assert!(
        (ratios[1] * 600. - f32::from(MIN_PANE_HEIGHT)).abs() < 2.,
        "{ratios:?}"
    );
}

#[gpui::test]
fn resize_uses_the_nearest_axis_of_its_kind(cx: &mut TestAppContext) {
    let mut s = open(cx, 3, true);
    // The top pane is in a vertical axis, so its width is the outer one's.
    assert!(s.resize(1, Axis::Horizontal, 32.));
    let outer = s.ratios(&[]);
    assert!(outer[0] < 0.5, "{outer:?}");
    assert_eq!(s.ratios(&[1]), vec![0.5, 0.5]);
    assert!(s.resize(1, Axis::Vertical, 32.));
    assert!(s.ratios(&[1])[0] > 0.5);

    s.host.update(s.cx, |host, _| host.group.equalize());
    assert_eq!(s.ratios(&[]), vec![0.5, 0.5]);
    assert_eq!(s.ratios(&[1]), vec![0.5, 0.5]);
}

#[gpui::test]
fn next_steps_through_the_panes_and_wraps(cx: &mut TestAppContext) {
    let mut s = open(cx, 3, true);
    assert_eq!(s.next(0, true), Some(1));
    assert_eq!(s.next(1, true), Some(2));
    assert_eq!(s.next(2, true), Some(0));
    assert_eq!(s.next(0, false), Some(2));
    assert_eq!(s.next(2, false), Some(1));
    let mut s = open(cx, 1, false);
    assert_eq!(s.next(0, true), None);
    assert_eq!(s.next(0, false), None);
}

#[gpui::test]
fn resize_and_equalize_end_a_border_drag(cx: &mut TestAppContext) {
    use gpui::{Modifiers, MouseButton, point};
    let mut s = open(cx, 2, false);
    let none = Modifiers::none();
    let at = point(px(400.), px(300.));
    for equalize in [false, true] {
        s.cx.simulate_mouse_down(at, MouseButton::Left, none);
        s.cx.simulate_mouse_move(point(at.x + px(3.), at.y), MouseButton::Left, none);
        if equalize {
            s.host.update(s.cx, |host, _| host.group.equalize());
        } else {
            assert!(s.resize(0, Axis::Horizontal, 100.));
        }
        let changed = s.ratios(&[]);
        s.cx.simulate_mouse_move(point(at.x + px(200.), at.y), MouseButton::Left, none);
        s.draw();
        s.cx.simulate_mouse_up(point(at.x + px(200.), at.y), MouseButton::Left, none);
        s.draw();
        assert_eq!(s.ratios(&[]), changed);
        s.host.update(s.cx, |host, _| host.group.equalize());
    }
}
