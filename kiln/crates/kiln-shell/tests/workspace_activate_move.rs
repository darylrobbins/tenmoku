// SPDX-License-Identifier: LicenseRef-Proprietary

//! The workspace's own calls for a host's keys: `activate` makes a pane in
//! the group the active one, focused, and tells the host; `move_tab` moves
//! a tab as a drop does, a pane it splits off watched like any other, and
//! the pane it empties closed.

use crate::support;

use std::{cell::RefCell, rc::Rc, slice};

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, Render, SharedString, Styled as _, TestAppContext, VisualTestContext, Window, div,
};
use kiln_shell::{
    Axis, Destination, Direction, ItemHandle, ItemPane, Member, PaneItem, PaneView as _, Workspace,
    WorkspaceEvent,
    pane::{DraggedTab, TabDropped},
};
use support::{APP, linux};

/// A page whose split is another page of the same title.
struct Page {
    title: SharedString,
    focus: FocusHandle,
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full()
    }
}

impl Focusable for Page {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PaneItem for Page {
    const KIND: &'static str = "test.page";

    fn tab_title(&self, _: &App) -> SharedString {
        self.title.clone()
    }

    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let title = self.title.clone();
        Some(cx.new(|cx| Page {
            title,
            focus: cx.focus_handle(),
        }))
    }
}

/// Everything a workspace emitted, by name, in order.
type Events = Rc<RefCell<Vec<&'static str>>>;

struct Opened<'a> {
    workspace: Entity<Workspace>,
    events: Events,
    cx: &'a mut VisualTestContext,
}

impl Opened<'_> {
    fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn page(&mut self, title: &str) -> Entity<Page> {
        let title = SharedString::from(title.to_owned());
        self.cx.new(|cx| Page {
            title,
            focus: cx.focus_handle(),
        })
    }

    fn add(&mut self, page: &Entity<Page>) {
        let (workspace, item) = (self.workspace.clone(), Box::new(page.clone()));
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

    /// Adds `page` to `pane`, shown and focused, whichever pane is active.
    fn add_to(&mut self, pane: &Entity<ItemPane>, page: &Entity<Page>) {
        let (pane, item) = (pane.clone(), Box::new(page.clone()));
        self.cx.update(|window, cx| {
            pane.update(cx, |pane, cx| pane.insert_item(item, None, window, cx))
        });
        self.draw();
    }

    fn move_tab(
        &mut self,
        source: &Entity<ItemPane>,
        page: &Entity<Page>,
        destination: Destination<ItemPane>,
    ) -> Option<Entity<ItemPane>> {
        let (workspace, source, item) = (self.workspace.clone(), source.clone(), page.id());
        let moved = self.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.move_tab(&source, item, destination, window, cx)
            })
        });
        self.draw();
        moved
    }

    /// Has `on` emit `page`'s tab dropped from `from` on it, as a drag
    /// would.
    fn drop_tab(&mut self, from: &Entity<ItemPane>, page: &Entity<Page>, on: &Entity<ItemPane>) {
        let dropped = TabDropped {
            dragged: DraggedTab {
                pane: from.downgrade(),
                item: page.id(),
            },
            destination: Destination::Pane {
                pane: on.clone(),
                index: None,
            },
        };
        on.update(self.cx, |_, cx| cx.emit(dropped));
        self.draw();
    }

    fn close_active_item(&mut self, pane: &Entity<ItemPane>) {
        let pane = pane.clone();
        self.cx
            .update(|window, cx| pane.update(cx, |pane, cx| pane.close_active_item(window, cx)));
        self.draw();
    }

    fn panes(&mut self) -> Vec<Entity<ItemPane>> {
        (self.workspace).read_with(self.cx, |workspace, _| {
            workspace.group().panes().into_iter().cloned().collect()
        })
    }

    fn active_pane(&mut self) -> Entity<ItemPane> {
        (self.workspace).read_with(self.cx, |workspace, _| workspace.active_pane().clone())
    }

    fn titles(&mut self, pane: &Entity<ItemPane>) -> Vec<String> {
        pane.read_with(self.cx, |pane, cx| {
            (pane.pane().items().iter())
                .map(|item| item.tab_title(cx).to_string())
                .collect()
        })
    }

    fn shape(&mut self) -> String {
        fn member<P>(of: &Member<P>) -> String {
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

    fn focused(&mut self, focus: &FocusHandle) -> bool {
        self.cx.update(|window, _| focus.is_focused(window))
    }

    fn page_focused(&mut self, page: &Entity<Page>) -> bool {
        let focus = page.read_with(self.cx, |page, _| page.focus.clone());
        self.focused(&focus)
    }

    fn pane_focused(&mut self, pane: &Entity<ItemPane>) -> bool {
        let focus = pane.read_with(self.cx, |pane, cx| pane.focus_handle(cx));
        self.focused(&focus)
    }

    fn take_events(&mut self) -> Vec<&'static str> {
        self.events.take()
    }
}

/// A workspace of kiln's panes with one pane, empty, its events recorded.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let (workspace, cx) = cx.add_window_view(|window, cx| {
        Workspace::item_panes(APP, window, cx)
            .capabilities(linux(), cx)
            .title("Notes")
    });
    let events = Events::default();
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&workspace, move |_, event: &WorkspaceEvent, _| {
            sink.borrow_mut().push(match event {
                WorkspaceEvent::ActiveChanged => "active",
                WorkspaceEvent::PaneAdded(_) => "added",
                WorkspaceEvent::PaneRemoved(_) => "removed",
            })
        })
        .detach()
    });
    let mut opened = Opened {
        workspace,
        events,
        cx,
    };
    opened.draw();
    opened
}

#[gpui::test]
fn activate_makes_a_pane_in_the_group_active_and_focused(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let a = s.page("a");
    s.add(&a);
    let first = s.active_pane();
    let right = s.split(Direction::Right);
    assert_eq!(s.active_pane(), right);
    s.take_events();

    // The first pane: active, its item focused, the host told.
    s.activate(&first);
    assert_eq!(s.active_pane(), first);
    assert!(s.page_focused(&a));
    assert_eq!(s.take_events(), ["active"]);

    // Again: nothing changes, so nothing is told.
    s.activate(&first);
    assert_eq!(s.active_pane(), first);
    assert_eq!(s.take_events(), Vec::<&str>::new());

    // A pane outside the group: nothing at all.
    let workspace = s.workspace.clone();
    let outside = s
        .cx
        .update(|window, cx| workspace.update(cx, |workspace, cx| workspace.new_pane(window, cx)));
    s.activate(&outside);
    assert_eq!(s.active_pane(), first);
    assert!(s.page_focused(&a));
    assert!(!s.panes().contains(&outside));
    assert_eq!(s.take_events(), Vec::<&str>::new());

    // The other pane, its copy of a focused.
    s.activate(&right);
    assert_eq!(s.active_pane(), right);
    assert!(!s.page_focused(&a));
    assert_eq!(s.take_events(), ["active"]);
}

#[gpui::test]
fn activate_focuses_an_empty_pane_itself(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let first = s.active_pane();
    let lower = s.split(Direction::Down);
    assert!(s.pane_focused(&lower));
    s.take_events();

    s.activate(&first);
    assert_eq!(s.active_pane(), first);
    assert!(s.pane_focused(&first));
    assert_eq!(s.take_events(), ["active"]);
}

#[gpui::test]
fn move_tab_to_an_edge_splits_off_a_watched_pane(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let [a, b, c] = ["a", "b", "c"].map(|title| s.page(title));
    s.add(&a);
    s.add(&b);
    let first = s.active_pane();
    let right = s.split(Direction::Right);
    s.activate(&first);
    s.take_events();

    // To the right pane's lower edge: a pane splits off, active, a focused.
    let moved = s.move_tab(
        &first,
        &a,
        Destination::Split {
            pane: right.clone(),
            direction: Direction::Down,
        },
    );
    let lower = moved.expect("it moved");
    assert_eq!(s.shape(), "H(P,V(P,P))");
    assert!(lower != first && lower != right);
    assert_eq!(s.active_pane(), lower);
    assert_eq!(s.titles(&lower), ["a"]);
    assert_eq!(s.titles(&first), ["b"], "the source keeps its other tab");
    assert!(s.page_focused(&a));
    assert_eq!(s.take_events(), ["added", "active"]);

    // It's watched: a tab dropped on it moves there.
    s.add_to(&first, &c);
    s.drop_tab(&first, &c, &lower);
    assert_eq!(s.titles(&lower), ["a", "c"]);
    assert_eq!(s.active_pane(), lower);

    // And it closes once its tabs close.
    s.close_active_item(&lower);
    s.close_active_item(&lower);
    assert_eq!(s.panes(), [first.clone(), right.clone()]);
    assert!(s.take_events().contains(&"removed"));
}

#[gpui::test]
fn move_tab_closes_the_source_it_empties(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let [a, b] = ["a", "b"].map(|title| s.page(title));
    s.add(&a);
    let first = s.active_pane();
    let right = s.split(Direction::Right);
    s.add(&b);
    s.activate(&first);
    s.take_events();

    // Closed by the move itself, not later once the pane's change is
    // heard.
    let (workspace, source, item) = (s.workspace.clone(), first.clone(), a.id());
    let (moved, left) = s.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let destination = Destination::Pane {
                pane: right.clone(),
                index: Some(0),
            };
            let moved = workspace.move_tab(&source, item, destination, window, cx);
            (moved, workspace.group().panes().len())
        })
    });
    s.draw();
    assert_eq!(moved, Some(right.clone()));
    assert_eq!(left, 1);
    assert_eq!(s.panes(), slice::from_ref(&right));
    assert_eq!(s.titles(&right), ["a", "a", "b"]);
    assert_eq!(s.active_pane(), right);
    assert!(s.page_focused(&a));
    assert_eq!(s.take_events(), ["active", "removed"]);

    // Split off from its own pane with tabs left, the pane stays.
    let moved = s.move_tab(
        &right,
        &b,
        Destination::Split {
            pane: right.clone(),
            direction: Direction::Left,
        },
    );
    let left = moved.expect("it moved");
    assert_eq!(s.panes(), [left.clone(), right.clone()]);
    assert_eq!(s.titles(&right), ["a", "a"]);
    assert_eq!(s.titles(&left), ["b"]);
    assert_eq!(s.take_events(), ["added", "active"]);
}

#[gpui::test]
fn move_tab_moves_nothing_it_cannot(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let [a, b] = ["a", "b"].map(|title| s.page(title));
    s.add(&a);
    let first = s.active_pane();
    let right = s.split(Direction::Right);
    s.take_events();

    // An item the source doesn't have.
    let none = s.move_tab(
        &first,
        &b,
        Destination::Pane {
            pane: right.clone(),
            index: None,
        },
    );
    assert_eq!(none, None);

    // A destination outside the group.
    let workspace = s.workspace.clone();
    let outside = s
        .cx
        .update(|window, cx| workspace.update(cx, |workspace, cx| workspace.new_pane(window, cx)));
    let none = s.move_tab(
        &first,
        &a,
        Destination::Pane {
            pane: outside,
            index: None,
        },
    );
    assert_eq!(none, None);
    assert_eq!(s.panes(), [first.clone(), right.clone()]);
    assert_eq!(s.titles(&first), ["a"]);
    assert_eq!(s.active_pane(), right);
    assert_eq!(s.take_events(), Vec::<&str>::new());
}
