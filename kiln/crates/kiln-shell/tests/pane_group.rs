// SPDX-License-Identifier: Apache-2.0

//! The group panes split into: splits that nest and collapse, neighbours
//! by layout, tabs moved between panes by key and by drag, a pane showing
//! a document already showing that copy, and a divider's drag. Generalised
//! from the editor's split and drag tests, over notes.

use crate::support;

use std::{cell::RefCell, rc::Rc};

use gpui::{
    AppContext as _, Context, Entity, Focusable, IntoElement, Modifiers, MouseButton,
    ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window, div, point,
    px,
};
use kiln_shell::{
    Axis, Destination, Direction, ItemHandle, ItemPane, Member, PaneGroup, PaneView,
    pane::{DropTarget, DropZone, TabDropped},
    pane_group::MIN_PANE_WIDTH,
};
use kiln_ui::UiTheme;
use support::note::{Note, note};

/// A host drawing a group of panes, and the tabs dropped on them.
struct Host {
    group: PaneGroup<ItemPane>,
    dropped: Rc<RefCell<Vec<Destination<ItemPane>>>>,
    subscriptions: Vec<gpui::Subscription>,
}

impl Host {
    /// Joins `pane` to the group's panes, and hears its drops.
    fn watch(&mut self, pane: &Entity<ItemPane>, cx: &mut Context<Self>) {
        let dropped = self.dropped.clone();
        self.subscriptions.push(cx.subscribe(
            pane,
            move |_, _, event: &TabDropped<ItemPane>, _| {
                dropped.borrow_mut().push(event.destination.clone())
            },
        ));
        let peers = Rc::new(RefCell::new(
            self.group
                .panes()
                .into_iter()
                .map(Entity::downgrade)
                .collect(),
        ));
        for pane in self.group.panes() {
            pane.update(cx, |pane, _| pane.pane_mut().set_peers(peers.clone()));
        }
    }
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
    first: Entity<ItemPane>,
    cx: &'a mut VisualTestContext,
}

impl Opened<'_> {
    fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn titles(&mut self, pane: &Entity<ItemPane>) -> Vec<String> {
        pane.read_with(self.cx, |pane, cx| {
            let items = pane.pane().items();
            items
                .iter()
                .map(|item| item.tab_title(cx).to_string())
                .collect()
        })
    }

    fn active(&mut self, pane: &Entity<ItemPane>) -> Option<usize> {
        pane.read_with(self.cx, |pane, _| pane.pane().active())
    }

    fn panes(&mut self) -> Vec<Entity<ItemPane>> {
        self.host.read_with(self.cx, |host, _| {
            host.group.panes().into_iter().cloned().collect()
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
        self.host
            .read_with(self.cx, |host, _| member(host.group.root()))
    }

    fn add(&mut self, pane: &Entity<ItemPane>, note: &Entity<Note>) {
        let item: Box<dyn ItemHandle> = Box::new(note.clone());
        pane.update(self.cx, |pane, cx| pane.add_item(item, cx));
    }

    fn focused(&mut self, note: &Entity<Note>) -> bool {
        self.cx
            .update(|window, cx| note.focus_handle(cx).is_focused(window))
    }

    /// Moves `note` from `source` to `destination`, joining a pane split
    /// off for it to the group.
    fn move_tab(
        &mut self,
        source: &Entity<ItemPane>,
        note: &Entity<Note>,
        destination: Destination<ItemPane>,
    ) -> Option<(Entity<ItemPane>, bool)> {
        let host = self.host.clone();
        let source = source.clone();
        let item = note.id();
        let moved = self.cx.update(|window, cx| {
            host.update(cx, |host, cx| {
                host.group.move_tab(&source, item, destination, window, cx)
            })
        });
        if let Some((pane, true)) = &moved {
            let pane = pane.clone();
            host.update(self.cx, |host, cx| host.watch(&pane, cx));
        }
        self.draw();
        moved
    }
}

/// A host with one pane, empty.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let first = Rc::new(RefCell::new(None));
    let made = first.clone();
    let (host, cx) = cx.add_window_view(move |_, cx| {
        let pane = cx.new(ItemPane::new);
        made.replace(Some(pane.clone()));
        let mut host = Host {
            group: PaneGroup::new(pane.clone()),
            dropped: Rc::default(),
            subscriptions: Vec::new(),
        };
        host.watch(&pane, cx);
        host
    });
    let first = first.take().unwrap();
    let mut opened = Opened { host, first, cx };
    opened.draw();
    opened
}

#[gpui::test]
fn splits_nest_collapse_and_find_neighbours(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let a = note("a", s.cx);
    let first = s.first.clone();
    s.add(&first, &a);
    // A tab moved to a split makes the pane first.
    let (right, new) = s
        .move_tab(
            &first,
            &a,
            Destination::Split {
                pane: first.clone(),
                direction: Direction::Right,
            },
        )
        .expect("moved");
    assert!(new);
    assert_eq!(s.shape(), "H(P,P)");
    assert_eq!(s.titles(&right), ["a"]);
    let (lower, _) = s
        .move_tab(
            &right,
            &a,
            Destination::Split {
                pane: right.clone(),
                direction: Direction::Down,
            },
        )
        .unwrap();
    assert_eq!(s.shape(), "H(P,V(P,P))");
    let ratios = s.host.read_with(s.cx, |host, _| host.group.ratios(&[1]));
    assert_eq!(ratios, Some(vec![0.5, 0.5]));

    // Neighbours by where the last frame laid the panes out.
    let neighbour = |s: &mut Opened, from: &Entity<ItemPane>, direction| {
        s.host
            .read_with(s.cx, |host, cx| host.group.neighbour(from, direction, cx))
    };
    assert_eq!(
        neighbour(&mut s, &first, Direction::Right),
        Some(right.clone())
    );
    assert_eq!(
        neighbour(&mut s, &lower, Direction::Up),
        Some(right.clone())
    );
    assert_eq!(
        neighbour(&mut s, &lower, Direction::Left),
        Some(first.clone())
    );
    assert_eq!(neighbour(&mut s, &first, Direction::Left), None);
    let widths = s
        .panes()
        .iter()
        .map(|pane| pane.read_with(s.cx, |pane, _| pane.pane().bounds().size.width))
        .collect::<Vec<_>>();
    assert!(
        widths.iter().all(|width| *width >= MIN_PANE_WIDTH),
        "{widths:?}"
    );

    // Removing a pane collapses its axis into its parent.
    s.host.update(s.cx, |host, _| host.group.remove(&right));
    s.draw();
    assert_eq!(s.shape(), "H(P,P)");
    let nearest = s
        .host
        .read_with(s.cx, |host, cx| host.group.nearest(&first, cx));
    assert_eq!(nearest, Some(lower));
}

#[gpui::test]
fn moves_go_in_at_an_index_or_show_a_copy(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let [a, b] = ["a", "b"].map(|title| note(title, s.cx));
    let first = s.first.clone();
    s.add(&first, &a);
    s.add(&first, &b);
    let (right, _) = s
        .move_tab(
            &first,
            &b,
            Destination::Split {
                pane: first.clone(),
                direction: Direction::Right,
            },
        )
        .unwrap();
    // Into another pane, at an index, shown and focused.
    s.move_tab(
        &first,
        &a,
        Destination::Pane {
            pane: right.clone(),
            index: Some(0),
        },
    );
    assert_eq!(s.titles(&right), ["a", "b"]);
    assert!(s.focused(&a));
    assert!(first.read_with(s.cx, |pane, _| pane.pane().is_emptied()));

    // A pane showing a note's document already shows that one instead.
    let copy = s.cx.update(|window, cx| {
        let item: &dyn ItemHandle = &a;
        item.split(window, cx).unwrap()
    });
    let copy = copy.downcast::<Note>().unwrap();
    s.add(&first, &copy);
    s.move_tab(
        &first,
        &copy,
        Destination::Pane {
            pane: right.clone(),
            index: None,
        },
    );
    assert_eq!(s.titles(&right), ["a", "b"]);
    assert_eq!(s.active(&right), Some(0));
    assert_eq!(s.titles(&first), Vec::<String>::new());

    // Unsaved changes too: the target's copy shows them.
    let other = note("c", s.cx);
    s.add(&first, &other);
    let twin = s.cx.new(|cx| Note {
        document: other.read(cx).document.clone(),
        title: "c".into(),
        dirty: false,
        tint: None,
        focus: cx.focus_handle(),
    });
    s.add(&right, &twin);
    other.update(s.cx, |note, _| note.dirty = true);
    assert!(
        s.move_tab(
            &first,
            &other,
            Destination::Pane {
                pane: right.clone(),
                index: None,
            }
        )
        .is_some()
    );
    assert_eq!(s.titles(&first), Vec::<String>::new());
}

#[gpui::test]
fn a_dragged_tab_drops_on_another_pane(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let [a, b] = ["a", "b"].map(|title| note(title, s.cx));
    let first = s.first.clone();
    s.add(&first, &a);
    s.add(&first, &b);
    let (right, _) = s
        .move_tab(
            &first,
            &b,
            Destination::Split {
                pane: first.clone(),
                direction: Direction::Right,
            },
        )
        .unwrap();
    s.draw();
    let from = first.read_with(s.cx, |pane, _| pane.pane().tab_bounds(0).unwrap().center());
    let content = right.read_with(s.cx, |pane, _| pane.pane().content_bounds());
    // The right edge of the right pane's content splits it.
    let to = point(content.right() - px(10.), content.center().y);
    let none = Modifiers::none();
    s.cx.simulate_mouse_down(from, MouseButton::Left, none);
    s.cx.simulate_mouse_move(
        point(from.x + px(3.), from.y + px(3.)),
        MouseButton::Left,
        none,
    );
    s.cx.simulate_mouse_move(to, MouseButton::Left, none);
    s.draw();
    let hovered = right.read_with(s.cx, |pane, _| pane.pane().hovered_drop());
    assert_eq!(hovered, Some(DropTarget::Content(DropZone::Right)));
    assert!(
        right
            .read_with(s.cx, |pane, _| pane.pane().drop_hint_bounds())
            .is_some()
    );
    s.cx.simulate_mouse_up(to, MouseButton::Left, none);
    s.draw();
    let dropped = s
        .host
        .read_with(s.cx, |host, _| host.dropped.borrow().clone());
    assert_eq!(
        dropped,
        [Destination::Split {
            pane: right.clone(),
            direction: Direction::Right,
        }]
    );
}

#[gpui::test]
fn a_divider_drag_moves_the_border(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let a = note("a", s.cx);
    let first = s.first.clone();
    s.add(&first, &a);
    let (right, _) = s
        .move_tab(
            &first,
            &a,
            Destination::Split {
                pane: first.clone(),
                direction: Direction::Right,
            },
        )
        .unwrap();
    s.draw();
    let border = right.read_with(s.cx, |pane, _| pane.pane().bounds().left());
    let window = s.cx.update(|window, _| window.viewport_size());
    let at = point(border - px(1.), window.height / 2.);
    let to = point(at.x + px(100.), at.y);
    let none = Modifiers::none();
    s.cx.simulate_mouse_down(at, MouseButton::Left, none);
    s.cx.simulate_mouse_move(point(at.x + px(3.), at.y), MouseButton::Left, none);
    s.cx.simulate_mouse_move(to, MouseButton::Left, none);
    s.draw();
    s.cx.simulate_mouse_up(to, MouseButton::Left, none);
    s.draw();
    let ratios = s
        .host
        .read_with(s.cx, |host, _| host.group.ratios(&[]))
        .unwrap();
    assert!(ratios[0] > 0.5, "{ratios:?}");
    assert!((ratios.iter().sum::<f32>() - 1.).abs() < 1e-4);
}
