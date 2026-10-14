// SPDX-License-Identifier: LicenseRef-Proprietary

//! The workspace's panes: splits showing a copy of the active item, panes
//! closing once emptied, tabs dropped on a pane or its edge, the titles
//! and events following the active item, focus going back to it, the
//! window's close asking every pane, and a pane view of a host's own.

mod support;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    slice,
};

use gpui::{
    AnyView, App, AppContext as _, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Task, TestAppContext, VisualTestContext, Window, div,
};
use kiln_shell::{
    Axis, CloseAnswer, Destination, Direction, ItemHandle, ItemPane, Member, ModalId, ModalView,
    PaneEvent, PaneItem, PaneState, PaneView, Workspace, WorkspaceEvent, close,
    pane::{DraggedTab, TabDropped},
};
use kiln_ui::ui_theme;
use support::{APP, linux};

/// A page: a pane item over a document that its splits share, whose
/// window title isn't its tab's.
struct Page {
    document: Rc<()>,
    title: SharedString,
    focus: FocusHandle,
    /// What it answers when asked to close.
    answer: Answer,
    /// How often it was asked.
    asked: usize,
}

#[derive(Clone, Copy)]
enum Answer {
    Close,
    Keep,
    /// Closes, once a task resolves.
    Later,
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

    fn window_title(&self, _: &App) -> SharedString {
        format!("{} — Pad", self.title).into()
    }

    fn shares_document(&self, other: &AnyView, cx: &App) -> bool {
        (other.clone().downcast::<Page>())
            .is_ok_and(|other| Rc::ptr_eq(&other.read(cx).document, &self.document))
    }

    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let (document, title) = (self.document.clone(), self.title.clone());
        Some(cx.new(|cx| Page {
            document,
            title,
            focus: cx.focus_handle(),
            answer: Answer::Close,
            asked: 0,
        }))
    }

    fn may_close(&mut self, _: &mut Window, _: &mut Context<Self>) -> CloseAnswer {
        self.asked += 1;
        match self.answer {
            Answer::Close => CloseAnswer::Close,
            Answer::Keep => CloseAnswer::Keep,
            Answer::Later => CloseAnswer::Ask(Task::ready(true)),
        }
    }
}

/// A modal that does nothing until dismissed.
struct Ask(FocusHandle);

impl Render for Ask {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.0).size_full()
    }
}

impl Focusable for Ask {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.0.clone()
    }
}

impl EventEmitter<DismissEvent> for Ask {}

impl ModalView for Ask {}

/// A pane view of the test's own: kiln's pane state, and how it was made.
struct Sheet {
    state: PaneState<Sheet>,
    made: &'static str,
}

impl EventEmitter<PaneEvent> for Sheet {}

impl EventEmitter<TabDropped<Sheet>> for Sheet {}

impl Focusable for Sheet {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.state.focus().clone()
    }
}

impl PaneView for Sheet {
    fn pane(&self) -> &PaneState<Self> {
        &self.state
    }

    fn pane_mut(&mut self) -> &mut PaneState<Self> {
        &mut self.state
    }

    fn new_peer(&self, cx: &mut Context<Self>) -> Self {
        Self {
            state: PaneState::new(ui_theme(cx), cx),
            made: "peer",
        }
    }
}

impl Render for Sheet {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = (self.state.active_item()).map(|item| item.to_any_view());
        div()
            .track_focus(self.state.focus())
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .child(self.state.record_bounds())
            .child(self.state.tab_bar(cx))
            .children(body)
    }
}

/// Everything a workspace emitted, in order.
type Events = Rc<RefCell<Vec<String>>>;

struct Opened<'a, P: PaneView> {
    workspace: Entity<Workspace<P>>,
    events: Events,
    cx: &'a mut VisualTestContext,
}

impl<P: PaneView> Opened<'_, P> {
    fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn page(&mut self, title: &str) -> Entity<Page> {
        let title = SharedString::from(title.to_owned());
        self.cx.new(|cx| Page {
            document: Rc::default(),
            title,
            focus: cx.focus_handle(),
            answer: Answer::Close,
            asked: 0,
        })
    }

    fn add(&mut self, page: &Entity<Page>) {
        let (workspace, item) = (self.workspace.clone(), Box::new(page.clone()));
        self.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.add_item(item, window, cx))
        });
        self.draw();
    }

    fn split(&mut self, direction: Direction) -> Entity<P> {
        let workspace = self.workspace.clone();
        let pane = self.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.split(direction, window, cx))
        });
        self.draw();
        pane
    }

    fn close_pane(&mut self, pane: &Entity<P>) {
        let (workspace, pane) = (self.workspace.clone(), pane.clone());
        self.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.close_pane(&pane, window, cx))
        });
        self.draw();
    }

    fn close_active_item(&mut self, pane: &Entity<P>) {
        let pane = pane.clone();
        self.cx
            .update(|window, cx| pane.update(cx, |pane, cx| pane.close_active_item(window, cx)));
        self.draw();
    }

    /// Has `on` emit `item`'s tab dropped from `from` at `destination`, as
    /// a drag would.
    fn drop_tab(&mut self, from: &Entity<P>, item: &Entity<Page>, destination: Destination<P>) {
        let dropped = TabDropped {
            dragged: DraggedTab {
                pane: from.downgrade(),
                item: item.id(),
            },
            destination: destination.clone(),
        };
        let on = match destination {
            Destination::Pane { pane, .. } | Destination::Split { pane, .. } => pane,
        };
        on.update(self.cx, |_, cx| cx.emit(dropped));
        self.draw();
    }

    /// A click in `pane`, as far as the workspace hears it.
    fn click(&mut self, pane: &Entity<P>) {
        pane.update(self.cx, |_, cx| cx.emit(PaneEvent::Activated));
        self.draw();
    }

    fn panes(&mut self) -> Vec<Entity<P>> {
        (self.workspace).read_with(self.cx, |workspace, _| {
            workspace.group().panes().into_iter().cloned().collect()
        })
    }

    fn active_pane(&mut self) -> Entity<P> {
        (self.workspace).read_with(self.cx, |workspace, _| workspace.active_pane().clone())
    }

    /// The active item's tab title.
    fn active_title(&mut self) -> Option<String> {
        (self.workspace).read_with(self.cx, |workspace, cx| {
            Some(workspace.active_item(cx)?.tab_title(cx).to_string())
        })
    }

    fn active_item(&mut self) -> Option<Entity<Page>> {
        (self.workspace).read_with(self.cx, |workspace, cx| {
            workspace.active_item(cx)?.downcast()
        })
    }

    fn titles(&mut self, pane: &Entity<P>) -> Vec<String> {
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

    /// The window's title and the shell's.
    fn titles_shown(&mut self) -> (String, String) {
        let window = self.cx.window_title().unwrap_or_default();
        let shell = self.workspace.read_with(self.cx, |workspace, cx| {
            workspace.shell().read(cx).title().to_string()
        });
        (window, shell)
    }

    fn take_events(&mut self) -> Vec<String> {
        self.events.take()
    }
}

/// Hears `workspace`'s events, by name.
fn record<P: PaneView>(workspace: &Entity<Workspace<P>>, cx: &mut VisualTestContext) -> Events {
    let events = Events::default();
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(workspace, move |_, event: &WorkspaceEvent<P>, _| {
            sink.borrow_mut().push(match event {
                WorkspaceEvent::ActiveChanged => "active".into(),
                WorkspaceEvent::PaneAdded(_) => "added".into(),
                WorkspaceEvent::PaneRemoved(_) => "removed".into(),
            })
        })
        .detach()
    });
    events
}

/// A workspace of kiln's panes, titled "Notes", with one pane, empty.
fn open(cx: &mut TestAppContext) -> Opened<'_, ItemPane> {
    let (workspace, cx) = cx.add_window_view(|window, cx| {
        Workspace::item_panes(APP, window, cx)
            .capabilities(linux(), cx)
            .title("Notes")
    });
    let events = record(&workspace, cx);
    let mut opened = Opened {
        workspace,
        events,
        cx,
    };
    opened.draw();
    opened
}

#[gpui::test]
fn splits_show_a_copy_beside_the_active_pane_active_and_focused(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let a = s.page("a");
    s.add(&a);
    let first = s.active_pane();
    s.take_events();

    let right = s.split(Direction::Right);
    assert_eq!(s.shape(), "H(P,P)");
    assert_eq!(s.active_pane(), right);
    assert_eq!(s.titles(&first), ["a"]);
    assert_eq!(s.titles(&right), ["a"]);
    let copy = s.active_item().unwrap();
    assert_ne!(copy, a);
    assert!(copy.read_with(s.cx, |copy, cx| copy.shares_document(&a.clone().into(), cx)));
    assert!(s.page_focused(&copy));

    let lower = s.split(Direction::Down);
    assert_eq!(s.shape(), "H(P,V(P,P))");
    assert_eq!(s.active_pane(), lower);
    let again = s.active_item().unwrap();
    assert!(again != copy && again != a);
    assert!(s.page_focused(&again));
    assert_eq!(s.take_events(), ["added", "active", "added", "active"]);
}

#[gpui::test]
fn an_emptied_pane_closes_unless_it_is_the_last(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let a = s.page("a");
    s.add(&a);
    let first = s.active_pane();
    let right = s.split(Direction::Right);
    s.take_events();

    // The split's copy closes: its pane goes, and the first is active.
    s.close_active_item(&right);
    assert_eq!(s.panes(), slice::from_ref(&first));
    assert_eq!(s.active_pane(), first);
    assert!(s.page_focused(&a));
    assert_eq!(s.take_events(), ["removed", "active"]);

    // The last pane stays, empty and focused.
    s.close_active_item(&first);
    assert_eq!(s.panes(), slice::from_ref(&first));
    assert_eq!(s.active_item(), None);
    let pane_focus = first.read_with(s.cx, |pane, cx| pane.focus_handle(cx));
    assert!(s.focused(&pane_focus));

    // So does a split of it, which shows no copy.
    let empty = s.split(Direction::Down);
    assert_eq!(s.titles(&empty), Vec::<String>::new());
    let empty_focus = empty.read_with(s.cx, |pane, cx| pane.focus_handle(cx));
    assert!(s.focused(&empty_focus));
    assert_eq!(s.panes().len(), 2);
}

#[gpui::test]
fn closing_the_active_pane_activates_the_nearest(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let a = s.page("a");
    s.add(&a);
    let first = s.active_pane();
    let middle = s.split(Direction::Right);
    let middle_copy = s.active_item().unwrap();
    let last = s.split(Direction::Right);
    assert_eq!(s.shape(), "H(P,P,P)");
    s.take_events();

    // The last's nearest is the middle, not the first in the group.
    s.close_pane(&last);
    assert_eq!(s.panes(), [first.clone(), middle.clone()]);
    assert_eq!(s.active_pane(), middle);
    assert!(s.page_focused(&middle_copy));
    assert_eq!(s.take_events(), ["removed", "active"]);

    // A pane that isn't active closes without moving the active one.
    let lower = s.split(Direction::Down);
    let lower_copy = s.active_item().unwrap();
    s.close_pane(&first);
    assert_eq!(s.shape(), "V(P,P)");
    assert_eq!(s.active_pane(), lower);
    assert!(s.page_focused(&lower_copy));

    // The last pane doesn't close.
    s.close_pane(&middle);
    s.close_pane(&lower);
    assert_eq!(s.panes(), slice::from_ref(&lower));
}

#[gpui::test]
fn a_close_finishing_after_another_pane_is_active_leaves_focus_there(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let [a, b] = ["a", "b"].map(|title| s.page(title));
    s.add(&a);
    let first = s.active_pane();
    let right = s.split(Direction::Right);
    let copy = s.active_item().unwrap();
    // The first pane's last tab is b, whose document shows nowhere else, so
    // it's asked.
    s.click(&first);
    s.add(&b);
    let workspace = s.workspace.clone();
    s.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.activate_item_where(|item, cx| item.tab_title(cx) == "a", window, cx)
        })
    });
    s.close_active_item(&first);
    assert_eq!(s.titles(&first), ["b"]);
    assert!(s.page_focused(&b));
    b.update(s.cx, |page, _| page.answer = Answer::Later);

    // The close waits on its answer while the other pane becomes active.
    let (closing, other) = (first.clone(), right.clone());
    s.cx.update(|window, cx| {
        closing.update(cx, |pane, cx| pane.close_active_item(window, cx));
        other.update(cx, |_, cx| cx.emit(PaneEvent::Activated));
    });
    s.draw();
    assert_eq!(b.read_with(s.cx, |page, _| page.asked), 1);
    assert_eq!(s.panes(), slice::from_ref(&right));
    assert_eq!(s.active_pane(), right);
    assert!(s.page_focused(&copy));
}

#[gpui::test]
fn a_dropped_tab_moves_and_its_destination_becomes_active(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let [a, b] = ["a", "b"].map(|title| s.page(title));
    s.add(&a);
    s.add(&b);
    let first = s.active_pane();
    let right = s.split(Direction::Right);
    s.click(&first);
    assert_eq!(s.active_pane(), first);
    s.take_events();

    // On another pane: it moves there, which becomes active.
    s.drop_tab(
        &first,
        &a,
        Destination::Pane {
            pane: right.clone(),
            index: None,
        },
    );
    assert_eq!(s.titles(&first), ["b"]);
    assert_eq!(s.titles(&right), ["b", "a"]);
    assert_eq!(s.active_pane(), right);
    assert!(s.page_focused(&a));
    assert_eq!(s.take_events(), ["active"]);

    // On a pane's edge: it splits off a pane, which becomes active.
    s.drop_tab(
        &first,
        &b,
        Destination::Split {
            pane: right.clone(),
            direction: Direction::Down,
        },
    );
    let panes = s.panes();
    assert_eq!(panes.len(), 2, "the emptied first pane closed");
    assert_eq!(s.shape(), "V(P,P)");
    let lower = s.active_pane();
    assert!(lower != right && lower != first);
    assert_eq!(s.titles(&lower), ["b"]);
    assert!(s.page_focused(&b));
    assert_eq!(s.take_events(), ["added", "active", "removed"]);

    // The split-off pane is watched: a click activates it, and it closes
    // once emptied.
    s.click(&right);
    assert_eq!(s.active_pane(), right);
    s.click(&lower);
    assert_eq!(s.active_pane(), lower);
    s.drop_tab(
        &lower,
        &b,
        Destination::Pane {
            pane: right.clone(),
            index: Some(0),
        },
    );
    assert_eq!(s.panes(), slice::from_ref(&right));
    assert_eq!(s.titles(&right), ["b", "a"]);
    assert_eq!(s.active_pane(), right);
}

#[gpui::test]
fn the_titles_follow_the_active_item_as_events_tell_the_host(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let notes = ("Notes".to_owned(), "Notes".to_owned());
    assert_eq!(s.titles_shown(), notes);
    assert_eq!(s.take_events(), Vec::<String>::new());

    let a = s.page("a");
    s.add(&a);
    let a_shown = ("a — Pad".to_owned(), "a — Pad".to_owned());
    assert_eq!(s.titles_shown(), a_shown);
    assert_eq!(s.take_events(), ["active"]);

    let b = s.page("b");
    s.add(&b);
    assert_eq!(s.titles_shown().0, "b — Pad");
    assert_eq!(s.take_events(), ["active"]);

    // Activating an item by what it is.
    let workspace = s.workspace.clone();
    let found = s.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.activate_item_where(|item, cx| item.tab_title(cx) == "a", window, cx)
        })
    });
    assert!(found);
    assert_eq!(s.active_title().as_deref(), Some("a"));
    assert!(s.page_focused(&a));
    assert_eq!(s.titles_shown(), a_shown);
    assert_eq!(s.take_events(), ["active"]);
    let missing = s.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.activate_item_where(|_, _| false, window, cx)
        })
    });
    assert!(!missing);

    // A split, another pane with a copy of the same item.
    let first = s.active_pane();
    let right = s.split(Direction::Right);
    assert_eq!(s.titles_shown(), a_shown);
    assert_eq!(s.take_events(), ["added", "active"]);
    s.close_active_item(&right);
    assert_eq!(s.take_events(), ["removed", "active"]);

    // Their window titles changing as they notify.
    a.update(s.cx, |page, cx| {
        page.title = "a2".into();
        cx.notify();
    });
    s.draw();
    assert_eq!(s.titles_shown().1, "a2 — Pad");
    assert_eq!(s.take_events(), Vec::<String>::new());

    // With no item, the workspace's title, which can change.
    s.close_active_item(&first);
    s.close_active_item(&first);
    assert_eq!(s.titles_shown(), notes);
    s.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.set_title("Pad", window, cx))
    });
    assert_eq!(s.titles_shown(), ("Pad".to_owned(), "Pad".to_owned()));
}

#[gpui::test]
fn focus_goes_back_to_the_active_item_or_its_empty_pane(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let first = s.active_pane();
    let workspace = s.workspace.clone();
    let shell = workspace.read_with(s.cx, |workspace, _| workspace.shell().clone());
    let handles = |s: &mut Opened<ItemPane>| {
        s.cx.update(|_, cx| {
            let centre = shell.read(cx).center_focus(cx).expect("a centre");
            (workspace.focus_handle(cx), centre)
        })
    };
    let pane_focus = first.read_with(s.cx, |pane, cx| pane.focus_handle(cx));
    assert_eq!(handles(&mut s), (pane_focus.clone(), pane_focus));

    let a = s.page("a");
    s.add(&a);
    let a_focus = a.read_with(s.cx, |page, _| page.focus.clone());
    assert_eq!(handles(&mut s), (a_focus.clone(), a_focus.clone()));

    // A dismissed modal gives focus back to it.
    let ask = s.cx.new(|cx| Ask(cx.focus_handle()));
    s.cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.show_modal(ModalId("ask"), ask.clone(), window, cx)
        })
    });
    s.draw();
    assert!(!s.focused(&a_focus));
    s.cx.update(|window, cx| shell.update(cx, |shell, cx| shell.dismiss_modal(window, cx)));
    s.draw();
    assert!(s.focused(&a_focus));

    // Focus elsewhere comes back through focus_active.
    let ask_focus = ask.read_with(s.cx, |ask, _| ask.0.clone());
    s.cx.update(|window, cx| window.focus(&ask_focus, cx));
    s.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.focus_active(window, cx))
    });
    assert!(s.focused(&a_focus));
}

#[gpui::test]
fn closing_the_window_asks_every_pane_items(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let [a, b] = ["a", "b"].map(|title| s.page(title));
    s.add(&a);
    s.split(Direction::Right);
    let copy = s.active_item().unwrap();
    s.add(&b);
    let handle = s.cx.update(|window, _| window.window_handle());
    let items =
        s.cx.update(|_, cx| close::window_items(handle, cx).expect("items given"));
    let ids: Vec<_> = items.iter().map(|item| item.id()).collect();
    assert_eq!(ids.len(), 3);
    for id in [a.id(), copy.id(), b.id()] {
        assert!(ids.contains(&id), "{ids:?}");
    }

    // The platform's close button asks them, the second pane's too, whose
    // item keeps the window open.
    let asked =
        |s: &mut Opened<ItemPane>| [&a, &b].map(|page| page.read_with(s.cx, |page, _| page.asked));
    b.update(s.cx, |page, _| page.answer = Answer::Keep);
    assert!(!s.cx.simulate_close());
    assert_eq!(asked(&mut s), [1, 1]);
    b.update(s.cx, |page, _| page.answer = Answer::Close);
    assert!(s.cx.simulate_close());
    assert_eq!(asked(&mut s), [2, 2]);
}

#[gpui::test]
fn a_pane_view_of_the_host_own_made_by_its_factory_and_new_peer(cx: &mut TestAppContext) {
    let made = Rc::new(Cell::new(0));
    let count = made.clone();
    let (workspace, cx) = cx.add_window_view(move |window, cx| {
        let new_pane = move |_: &mut Window, cx: &mut App| {
            count.set(count.get() + 1);
            cx.new(|cx| Sheet {
                state: PaneState::new(ui_theme(cx), cx),
                made: "factory",
            })
        };
        let first = new_pane(window, cx);
        Workspace::new(APP, first, new_pane, window, cx)
            .capabilities(linux(), cx)
            .title("Sheets")
    });
    let events = record(&workspace, cx);
    let mut s = Opened {
        workspace,
        events,
        cx,
    };
    s.draw();
    assert_eq!(made.get(), 1);
    assert_eq!(s.titles_shown().0, "Sheets");

    let a = s.page("a");
    s.add(&a);
    let first = s.active_pane();
    assert_eq!(first.read_with(s.cx, |sheet, _| sheet.made), "factory");

    // A split's pane is the active pane's peer.
    let right = s.split(Direction::Right);
    assert_eq!(made.get(), 1);
    assert_eq!(right.read_with(s.cx, |sheet, _| sheet.made), "peer");
    let copy = s.active_item().unwrap();
    assert!(s.page_focused(&copy));
    assert_eq!(s.titles_shown().0, "a — Pad");

    // Drops and emptied panes as with kiln's panes.
    s.drop_tab(
        &first,
        &a,
        Destination::Split {
            pane: right.clone(),
            direction: Direction::Down,
        },
    );
    assert_eq!(s.shape(), "V(P,P)");
    let lower = s.active_pane();
    assert_eq!(lower.read_with(s.cx, |sheet, _| sheet.made), "peer");
    assert!(s.page_focused(&a));

    // The factory, for the panes a restore makes.
    let workspace = s.workspace.clone();
    let made_pane = s
        .cx
        .update(|window, cx| workspace.update(cx, |workspace, cx| workspace.new_pane(window, cx)));
    assert_eq!(made.get(), 2);
    assert_eq!(made_pane.read_with(s.cx, |sheet, _| sheet.made), "factory");
    assert_eq!(s.panes().len(), 2);
}
