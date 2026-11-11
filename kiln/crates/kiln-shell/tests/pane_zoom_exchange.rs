// SPDX-License-Identifier: Apache-2.0

//! A zoomed pane, drawn alone over the group's area until the tree changes
//! or another pane is active, its neighbours where they were before the
//! zoom; and vim's `<C-w>x`, the active pane trading places and sizes with
//! its next sibling, or its previous when it's last.

use crate::support;

use gpui::{Bounds, Entity, Pixels, TestAppContext, VisualTestContext, px, size};
use kiln_shell::{
    Axis, Destination, Direction, ItemPane, Member, PaneEvent, PaneView as _, Workspace,
};
use support::{
    APP, linux,
    note::{Note, note},
};

struct Opened<'a> {
    workspace: Entity<Workspace>,
    cx: &'a mut VisualTestContext,
}

impl Opened<'_> {
    fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn add(&mut self, note: &Entity<Note>) {
        let (workspace, item) = (self.workspace.clone(), Box::new(note.clone()));
        self.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.add_item(item, window, cx))
        });
        self.draw();
    }

    fn split(&mut self, direction: Direction) -> Entity<ItemPane> {
        let workspace = self.workspace.clone();
        let pane = self.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.split(direction, window, cx))
        });
        self.draw();
        pane
    }

    fn activate(&mut self, pane: &Entity<ItemPane>) {
        let (workspace, pane) = (self.workspace.clone(), pane.clone());
        self.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.activate(pane, window, cx))
        });
        self.draw();
    }

    fn close_pane(&mut self, pane: &Entity<ItemPane>) {
        let (workspace, pane) = (self.workspace.clone(), pane.clone());
        self.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.close_pane(&pane, window, cx))
        });
        self.draw();
    }

    fn toggle_zoom(&mut self) -> bool {
        let workspace = self.workspace.clone();
        let zoomed = workspace.update(self.cx, |workspace, cx| workspace.toggle_zoom(cx));
        self.draw();
        zoomed
    }

    fn exchange(&mut self) -> Option<Entity<ItemPane>> {
        let workspace = self.workspace.clone();
        let other = self.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.exchange(window, cx))
        });
        self.draw();
        other
    }

    fn zoomed(&mut self) -> Option<Entity<ItemPane>> {
        (self.workspace).read_with(self.cx, |workspace, _| workspace.group().zoomed().cloned())
    }

    fn active_pane(&mut self) -> Entity<ItemPane> {
        (self.workspace).read_with(self.cx, |workspace, _| workspace.active_pane().clone())
    }

    fn panes(&mut self) -> Vec<Entity<ItemPane>> {
        (self.workspace).read_with(self.cx, |workspace, _| {
            workspace.group().panes().into_iter().cloned().collect()
        })
    }

    fn bounds(&mut self, pane: &Entity<ItemPane>) -> Bounds<Pixels> {
        pane.read_with(self.cx, |pane, _| pane.pane().bounds())
    }

    fn ratios(&mut self, path: &[usize]) -> Option<Vec<f32>> {
        (self.workspace).read_with(self.cx, |workspace, _| workspace.group().ratios(path))
    }

    fn neighbour(
        &mut self,
        from: &Entity<ItemPane>,
        direction: Direction,
    ) -> Option<Entity<ItemPane>> {
        (self.workspace).read_with(self.cx, |workspace, cx| {
            workspace.group().neighbour(from, direction, cx)
        })
    }

    fn titles(&mut self, pane: &Entity<ItemPane>) -> Vec<String> {
        pane.read_with(self.cx, |pane, cx| {
            (pane.pane().items().iter())
                .map(|item| item.tab_title(cx).to_string())
                .collect()
        })
    }

    /// Whether the active pane's active item has focus.
    fn active_item_focused(&mut self) -> bool {
        let pane = self.active_pane();
        self.cx.update(|window, cx| {
            let item = pane.read(cx).pane().active_item().expect("an item");
            item.item_focus_handle(cx).is_focused(window)
        })
    }

    fn shape(&mut self) -> String {
        fn member(of: &Member<ItemPane>) -> String {
            match of {
                Member::Pane(_) => "P".into(),
                Member::Axis { axis, members } => {
                    let axis = if *axis == Axis::Horizontal { "H" } else { "V" };
                    let inner: Vec<_> = members.iter().map(member).collect();
                    format!("{axis}({})", inner.join(","))
                }
            }
        }
        (self.workspace).read_with(self.cx, |workspace, _| member(workspace.group().root()))
    }
}

/// A workspace of kiln's panes with one pane, showing note "a".
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let (workspace, cx) = cx.add_window_view(|window, cx| {
        Workspace::item_panes(APP, window, cx).capabilities(linux(), cx)
    });
    let mut opened = Opened { workspace, cx };
    opened.draw();
    let a = note("a", opened.cx);
    opened.add(&a);
    opened
}

/// `H(first, V(right, lower))`, `lower` active.
fn three(s: &mut Opened) -> (Entity<ItemPane>, Entity<ItemPane>, Entity<ItemPane>) {
    let first = s.active_pane();
    let right = s.split(Direction::Right);
    let lower = s.split(Direction::Down);
    assert_eq!(s.shape(), "H(P,V(P,P))");
    (first, right, lower)
}

/// The smallest bounds holding all of `bounds`.
fn union(bounds: &[Bounds<Pixels>]) -> Bounds<Pixels> {
    let left = bounds
        .iter()
        .map(|b| b.left())
        .fold(px(f32::MAX), Pixels::min);
    let top = bounds
        .iter()
        .map(|b| b.top())
        .fold(px(f32::MAX), Pixels::min);
    let right = bounds
        .iter()
        .map(|b| b.right())
        .fold(px(f32::MIN), Pixels::max);
    let bottom = bounds
        .iter()
        .map(|b| b.bottom())
        .fold(px(f32::MIN), Pixels::max);
    Bounds::from_corners(gpui::point(left, top), gpui::point(right, bottom))
}

#[gpui::test]
fn a_zoomed_pane_is_drawn_alone(cx: &mut TestAppContext) {
    let mut s = open(cx);
    // A lone pane doesn't zoom.
    assert!(!s.toggle_zoom());
    assert_eq!(s.zoomed(), None);

    let (first, right, lower) = three(&mut s);
    let panes = [first.clone(), right.clone(), lower.clone()];
    let before: Vec<_> = panes.iter().map(|pane| s.bounds(pane)).collect();
    let area = union(&before);

    // The active pane, over the whole of the group's area.
    assert!(s.toggle_zoom());
    assert_eq!(s.zoomed(), Some(lower.clone()));
    assert_eq!(s.active_pane(), lower);
    assert_eq!(s.bounds(&lower), area);
    assert!(s.active_item_focused());

    // Only it is laid out: as the window shrinks, it follows, while the
    // others stay where they were, their items and ratios kept.
    let viewport = s.cx.update(|window, _| window.viewport_size());
    s.cx.simulate_resize(size(viewport.width - px(100.), viewport.height - px(60.)));
    s.draw();
    let shrunk = s.bounds(&lower);
    assert_eq!(shrunk.size.width, area.size.width - px(100.));
    assert_eq!(shrunk.size.height, area.size.height - px(60.));
    assert_eq!(s.bounds(&first), before[0]);
    assert_eq!(s.bounds(&right), before[1]);
    assert_eq!(s.titles(&first), ["a"]);
    assert_eq!(s.titles(&right), ["a"]);
    assert_eq!(s.shape(), "H(P,V(P,P))");

    // Unzoomed, every pane is laid out again at its ratios, in the
    // smaller area.
    assert!(!s.toggle_zoom());
    assert_eq!(s.zoomed(), None);
    assert_eq!(s.ratios(&[]), Some(vec![0.5, 0.5]));
    assert_eq!(s.ratios(&[1]), Some(vec![0.5, 0.5]));
    let after: Vec<_> = panes.iter().map(|pane| s.bounds(pane)).collect();
    assert_eq!(union(&after), shrunk);
    let half = (shrunk.size.width - px(1.)) / 2.;
    assert!((after[0].size.width - half).abs() <= px(1.), "{after:?}");
    assert!((after[1].size.width - half).abs() <= px(1.), "{after:?}");
    assert_eq!(after[1].left(), after[2].left());
    assert!(after[2].top() > after[1].bottom(), "{after:?}");
    assert_eq!(s.active_pane(), lower);
}

#[gpui::test]
fn a_split_a_close_or_another_active_pane_ends_the_zoom(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let (first, right, lower) = three(&mut s);

    // A split.
    assert!(s.toggle_zoom());
    let split = s.split(Direction::Right);
    assert_eq!(s.zoomed(), None);
    assert_eq!(s.active_pane(), split);

    // A pane closed, even one that isn't active.
    assert!(s.toggle_zoom());
    s.close_pane(&split);
    assert_eq!(s.zoomed(), None);
    s.activate(&lower);
    assert!(s.toggle_zoom());
    s.close_pane(&first);
    assert_eq!(s.zoomed(), None);
    assert_eq!(s.shape(), "V(P,P)");

    // Another pane made active, by the host or by a click on it; the
    // active one again keeps it.
    assert!(s.toggle_zoom());
    s.activate(&lower);
    assert_eq!(s.zoomed(), Some(lower.clone()));
    s.activate(&right);
    assert_eq!(s.zoomed(), None);
    assert!(s.toggle_zoom());
    assert_eq!(s.zoomed(), Some(right.clone()));
    lower.update(s.cx, |_, cx| cx.emit(PaneEvent::Activated));
    s.draw();
    assert_eq!(s.active_pane(), lower);
    assert_eq!(s.zoomed(), None);

    // A tab moved to another pane.
    assert!(s.toggle_zoom());
    let tab = lower.read_with(s.cx, |pane, _| pane.pane().active_item().unwrap().id());
    let (workspace, from) = (s.workspace.clone(), lower.clone());
    let destination = Destination::Pane {
        pane: right.clone(),
        index: None,
    };
    s.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.move_tab(&from, tab, destination, window, cx)
        })
    });
    s.draw();
    assert_eq!(s.active_pane(), right);
    assert_eq!(s.zoomed(), None);

    // Resizing does nothing while zoomed; evening the ratios keeps it.
    let pane = s.split(Direction::Down);
    assert!(s.toggle_zoom());
    let ratios = s.ratios(&[]);
    let resized = s.workspace.update(s.cx, |workspace, _| {
        workspace.group_mut().resize(&pane, Axis::Vertical, px(40.))
    });
    assert!(!resized);
    assert_eq!(s.ratios(&[]), ratios);
    s.workspace
        .update(s.cx, |workspace, _| workspace.group_mut().equalize());
    s.draw();
    assert_eq!(s.zoomed(), Some(pane.clone()));

    // A restored tree, through the group.
    s.workspace.update(s.cx, |workspace, _| {
        let only = Member::Pane(workspace.active_pane().clone());
        workspace.group_mut().set_root(only);
    });
    s.draw();
    assert_eq!(s.zoomed(), None);
}

#[gpui::test]
fn neighbours_while_zoomed_follow_the_layout_before_it(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let (first, right, lower) = three(&mut s);
    let panes = [first, right, lower.clone()];
    let directions = [
        Direction::Left,
        Direction::Right,
        Direction::Up,
        Direction::Down,
    ];
    let neighbours = |s: &mut Opened| {
        let mut found = Vec::new();
        for pane in &panes {
            for direction in directions {
                found.push(s.neighbour(pane, direction));
            }
        }
        found
    };
    let before = neighbours(&mut s);
    assert!(s.toggle_zoom());
    // The zoomed pane's frame is the whole area: what it lies next to is
    // where it was.
    assert_eq!(neighbours(&mut s), before);
    let nearest = s
        .workspace
        .read_with(s.cx, |workspace, cx| workspace.group().nearest(&lower, cx));
    assert_eq!(nearest, Some(panes[1].clone()));

    // A focus move goes there, ending the zoom.
    let left = s.neighbour(&lower, Direction::Left).unwrap();
    assert_eq!(left, panes[0]);
    s.activate(&left);
    assert_eq!(s.zoomed(), None);
    assert_eq!(s.active_pane(), panes[0]);
}

#[gpui::test]
fn exchange_swaps_with_the_next_sibling_or_the_previous_when_last(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let first = s.active_pane();
    let middle = s.split(Direction::Right);
    let last = s.split(Direction::Right);
    assert_eq!(s.panes(), [first.clone(), middle.clone(), last.clone()]);
    assert_eq!(s.ratios(&[]), Some(vec![0.5, 0.25, 0.25]));

    // The first trades places with the next, their sizes with them; the
    // other, now where the first was, is active and focused.
    s.activate(&first);
    assert_eq!(s.exchange(), Some(middle.clone()));
    assert_eq!(s.panes(), [middle.clone(), first.clone(), last.clone()]);
    assert_eq!(s.ratios(&[]), Some(vec![0.25, 0.5, 0.25]));
    assert_eq!(s.active_pane(), middle);
    assert!(s.active_item_focused());
    let (moved, stayed) = (s.bounds(&first), s.bounds(&middle));
    assert!(moved.left() > stayed.right(), "{moved:?} {stayed:?}");
    assert!(moved.size.width > stayed.size.width * 1.5);

    // The last trades with the one before it.
    s.activate(&last);
    assert_eq!(s.exchange(), Some(first.clone()));
    assert_eq!(s.panes(), [middle.clone(), last.clone(), first.clone()]);
    assert_eq!(s.ratios(&[]), Some(vec![0.25, 0.25, 0.5]));
    assert_eq!(s.active_pane(), first);
    assert!(s.active_item_focused());

    // In a nested axis, among its own members; and a zoom ends.
    let lower = s.split(Direction::Down);
    assert_eq!(s.shape(), "H(P,P,V(P,P))");
    assert!(s.toggle_zoom());
    assert_eq!(s.exchange(), Some(first.clone()));
    assert_eq!(s.zoomed(), None);
    assert_eq!(
        s.panes(),
        [middle.clone(), last.clone(), lower.clone(), first.clone()]
    );
    assert_eq!(s.active_pane(), first);
    assert!(s.bounds(&first).top() > s.bounds(&lower).bottom());
}

#[gpui::test]
fn exchange_with_a_split_sibling_or_alone_does_nothing(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let only = s.active_pane();
    assert_eq!(s.exchange(), None);
    assert_eq!(s.panes(), [only]);

    let (first, right, lower) = three(&mut s);
    s.activate(&first);
    assert!(s.toggle_zoom());
    // Its sibling is a split.
    assert_eq!(s.exchange(), None);
    assert_eq!(s.shape(), "H(P,V(P,P))");
    assert_eq!(s.panes(), [first.clone(), right, lower]);
    assert_eq!(s.ratios(&[]), Some(vec![0.5, 0.5]));
    assert_eq!(s.active_pane(), first);
    assert_eq!(s.zoomed(), Some(first));
    assert!(s.active_item_focused());
}
