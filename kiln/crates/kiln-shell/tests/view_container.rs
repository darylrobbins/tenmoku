// SPDX-License-Identifier: Apache-2.0

//! A view container: views go by their order, one with an id that's there
//! replacing it; the views' collapse is saved with the workspace's layout
//! as it changes and comes back with a restore, a saved view the container
//! doesn't have ignored, whatever the panel's, views' or state's keys; a
//! dock hears the panels it was built with or given behind its back, and
//! a panel that comes to have state; and a panel with no state of its
//! own, or a layout saved before panels had state, saves and restores as
//! before.

use crate::support;

use std::{
    cell::Cell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, Modifiers, ParentElement as _, Render, SharedString, Styled as _, TestAppContext,
    VisualTestContext, Window, div, px,
};
use kiln_command::Value;
use kiln_shell::{
    Dock, DockItem, DockPosition, DockState, DockView as _, PanelId, ViewContainer, ViewId,
    ViewSpec, Workspace,
    layout::{LayoutStore, WindowLayout},
};
use kiln_ui::Icon;
use support::{APP, linux};

const EXPLORER: PanelId = PanelId("explorer");
const FOLDERS: ViewId = ViewId("folders");
const OUTLINE: ViewId = ViewId("outline");
const TIMELINE: ViewId = ViewId("timeline");

/// A view's body.
struct Body;

impl Render for Body {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

/// A panel with no state of its own.
struct Panel(PanelId, FocusHandle);

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.1).size_full()
    }
}

impl Focusable for Panel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.1.clone()
    }
}

impl DockItem for Panel {
    fn id(&self) -> PanelId {
        self.0
    }

    fn title(&self, _: &App) -> SharedString {
        self.0.0.into()
    }

    fn icon(&self, _: &App) -> Icon {
        Icon::Search
    }
}

fn spec(id: ViewId, order: i32, collapsed: bool, cx: &mut App) -> ViewSpec {
    ViewSpec {
        id,
        title: id.0.into(),
        order,
        default_collapsed: collapsed,
        view: cx.new(|_| Body).into(),
    }
}

/// The explorer: folders (order 10), the outline (20, collapsed at first)
/// and the timeline (30), added out of order.
fn explorer(cx: &mut App) -> Entity<ViewContainer> {
    cx.new(|cx| {
        let mut explorer = ViewContainer::new(EXPLORER, "Explorer", Icon::Files, cx);
        for (id, order, collapsed) in [
            (TIMELINE, 30, false),
            (FOLDERS, 10, false),
            (OUTLINE, 20, true),
        ] {
            explorer.add_view(spec(id, order, collapsed, cx), cx);
        }
        explorer
    })
}

fn collapsed(explorer: &Entity<ViewContainer>, view: ViewId, cx: &App) -> Option<bool> {
    explorer.read(cx).stack().read(cx).is_collapsed(view)
}

#[gpui::test]
fn views_go_by_order_and_the_same_id_replaces(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let explorer = explorer(cx);
        let ids = |cx: &App| explorer.read(cx).view_ids(cx);
        assert_eq!(ids(cx), [FOLDERS, OUTLINE, TIMELINE]);
        assert_eq!(collapsed(&explorer, OUTLINE, cx), Some(true));
        assert_eq!(collapsed(&explorer, FOLDERS, cx), Some(false));

        // Of the same order as one there, it goes below it; of the least,
        // at the top.
        explorer.update(cx, |explorer, cx| {
            explorer.add_view(spec(ViewId("search"), 20, false, cx), cx);
            explorer.add_view(spec(ViewId("open"), -5, false, cx), cx);
        });
        let all = [ViewId("open"), FOLDERS, OUTLINE, ViewId("search"), TIMELINE];
        assert_eq!(ids(cx), all);

        // The same id replaces the view, moving to its new order, keeping
        // its collapse; at its own order, it stays where it is.
        explorer.update(cx, |explorer, cx| {
            explorer.add_view(spec(OUTLINE, 40, false, cx), cx);
            explorer.add_view(spec(FOLDERS, 10, true, cx), cx);
        });
        let all = [ViewId("open"), FOLDERS, ViewId("search"), TIMELINE, OUTLINE];
        assert_eq!(ids(cx), all);
        assert_eq!(collapsed(&explorer, OUTLINE, cx), Some(true));
        assert_eq!(collapsed(&explorer, FOLDERS, cx), Some(false));
    });
}

/// A fresh directory under the system's temporary one, removed when
/// dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn store() -> (Scratch, LayoutStore) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "kiln-view-container-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let store = LayoutStore::new(&dir, Path::new("/project"));
    (Scratch(dir), store)
}

/// A workspace whose left dock holds the explorer, and a search panel.
fn open(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<ViewContainer>,
    &mut VisualTestContext,
) {
    let (workspace, cx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::item_panes(APP, window, cx).capabilities(linux(), cx);
        workspace.add_panel(DockPosition::Left, explorer(cx), window, cx);
        let search = cx.new(|cx| Panel(PanelId("search"), cx.focus_handle()));
        workspace.add_panel(DockPosition::Left, search, window, cx);
        workspace
    });
    let explorer = workspace.read_with(cx, |workspace, cx| {
        let dock = workspace.dock(DockPosition::Left).unwrap().read(cx);
        dock.panel::<ViewContainer>().unwrap()
    });
    draw(cx);
    (workspace, explorer, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

#[gpui::test]
fn a_views_collapse_persists_through_the_workspace_layout(cx: &mut TestAppContext) {
    let (_dir, store) = store();
    let mut app = cx.clone();
    let (workspace, explorer, cx) = open(cx);
    let layout = WindowLayout::new(store.clone(), store.claim_new().unwrap());
    cx.update(|window, cx| workspace.update(cx, |w, cx| w.set_layout(layout, window, cx)));
    draw(cx);

    // A click on the outline's header expands it, and one on the
    // timeline's collapses it: the layout is saved, with no call from the
    // host.
    for view in [OUTLINE, TIMELINE] {
        let header = explorer.read_with(cx, |explorer, cx| {
            explorer
                .stack()
                .read(cx)
                .header_bounds(view)
                .expect("drawn")
        });
        cx.simulate_click(header.center(), Modifiers::none());
        draw(cx);
    }
    let entries = store.entries();
    assert_eq!(entries.len(), 1);
    let entry = entries[0].clone();
    let views = &entry["docks"]["left"]["panels"]["explorer"]["views"];
    assert_eq!(views["outline"]["collapsed"], false);
    assert_eq!(views["timeline"]["collapsed"], true);
    assert_eq!(views["folders"]["collapsed"], false);
    // The search panel has no state of its own.
    let panels = entry["docks"]["left"]["panels"].as_object().unwrap();
    assert_eq!(panels.keys().collect::<Vec<_>>(), ["explorer"]);

    // A new window's explorer, the outline collapsed at first, takes them
    // back; a saved view it doesn't have, and a saved panel the dock
    // doesn't have, are ignored.
    let mut saved = entry.clone();
    saved["docks"]["left"]["panels"]["explorer"]["views"]["gone"] =
        serde_json::json!({"collapsed": true, "weight": 3.0});
    saved["docks"]["left"]["panels"]["gone"] = serde_json::json!({"views": {}});
    let (restored, explorer, cx) = open(&mut app);
    let outline = cx.update(|_, cx| collapsed(&explorer, OUTLINE, cx));
    assert_eq!(outline, Some(true));
    let restoring = restored.clone();
    assert!(cx.update(|window, cx| restoring.update(cx, |w, cx| w.restore(&saved, window, cx))));
    draw(cx);
    explorer.read_with(cx, |explorer, cx| {
        assert_eq!(explorer.view_ids(cx), [FOLDERS, OUTLINE, TIMELINE]);
        let stack = explorer.stack().read(cx);
        assert_eq!(stack.is_collapsed(OUTLINE), Some(false));
        assert_eq!(stack.is_collapsed(TIMELINE), Some(true));
        assert_eq!(stack.is_collapsed(FOLDERS), Some(false));
    });
    let entry = restored.read_with(cx, |workspace, cx| workspace.entry(cx));
    assert_eq!(
        entry["docks"]["left"]["panels"],
        serde_json::json!({"explorer": {"views": {
            "folders": {"collapsed": false, "weight": 1.0},
            "outline": {"collapsed": false, "weight": 1.0},
            "timeline": {"collapsed": true, "weight": 1.0},
        }}})
    );
}

#[gpui::test]
fn panels_without_state_and_old_layouts_are_as_before(cx: &mut TestAppContext) {
    cx.update(|cx| {
        // A dock of panels with no state of their own saves what it did.
        let mut dock = DockState::new(DockPosition::Left);
        let search = cx.new(|cx| Panel(PanelId("search"), cx.focus_handle()));
        dock.add_panel(search, cx);
        let saved = dock.save(cx);
        let keys: Vec<_> = saved.as_object().unwrap().keys().cloned().collect();
        let before = [
            "visible",
            "active",
            "joined",
            "size",
            "side_size",
            "bottom_size",
        ];
        assert_eq!(keys, before);

        // A layout saved before panels had state restores the dock, and
        // leaves the explorer's views as they were built.
        let explorer = explorer(cx);
        let mut dock = DockState::new(DockPosition::Left);
        dock.add_panel(cx.new(|cx| Panel(PanelId("search"), cx.focus_handle())), cx);
        dock.add_panel(explorer.clone(), cx);
        let old = serde_json::json!({"visible": false, "active": "explorer", "side_size": 300.0});
        dock.restore(&old, cx);
        assert_eq!(dock.active(), Some(EXPLORER));
        assert!(!dock.is_visible());
        assert_eq!(dock.side_size(), px(300.));
        assert_eq!(collapsed(&explorer, OUTLINE, cx), Some(true));
        assert_eq!(collapsed(&explorer, FOLDERS, cx), Some(false));
        // Saved again, it has the explorer's state.
        let views = &dock.save(cx)["panels"]["explorer"]["views"];
        assert_eq!(views["outline"]["collapsed"], true);
    });
}

/// A panel with a count once it has one, its state, saved as
/// `{"kind": "count", "state": n}`, as a pane item's is.
struct Counter(Option<u64>, FocusHandle);

impl Counter {
    fn set(&mut self, count: u64, cx: &mut Context<Self>) {
        self.0 = Some(count);
        cx.notify();
    }
}

impl Render for Counter {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.1).size_full()
    }
}

impl Focusable for Counter {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.1.clone()
    }
}

impl DockItem for Counter {
    fn id(&self) -> PanelId {
        PanelId("state")
    }

    fn title(&self, _: &App) -> SharedString {
        "Counter".into()
    }

    fn icon(&self, _: &App) -> Icon {
        Icon::Search
    }

    fn save_state(&self, _: &App) -> Option<Value> {
        let count = self.0?;
        Some(serde_json::json!({"kind": "count", "state": count}))
    }

    fn restore_state(&mut self, saved: &Value, _: &mut Context<Self>) {
        self.0 = saved["state"].as_u64();
    }
}

const STATE: ViewId = ViewId("state");
const BOUNDS: ViewId = ViewId("bounds");

/// A container named "bounds", of views named "state" and "bounds".
fn bounds(cx: &mut App) -> Entity<ViewContainer> {
    cx.new(|cx| {
        let mut bounds = ViewContainer::new(PanelId("bounds"), "Bounds", Icon::Files, cx);
        bounds.add_view(spec(STATE, 0, false, cx), cx);
        bounds.add_view(spec(BOUNDS, 1, false, cx), cx);
        bounds
    })
}

/// A workspace whose left dock holds the "bounds" container and the
/// counter, with no count.
fn open_bounds(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    (Entity<ViewContainer>, Entity<Counter>),
    &mut VisualTestContext,
) {
    let (workspace, cx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::item_panes(APP, window, cx).capabilities(linux(), cx);
        workspace.add_panel(DockPosition::Left, bounds(cx), window, cx);
        let counter = cx.new(|cx| Counter(None, cx.focus_handle()));
        workspace.add_panel(DockPosition::Left, counter, window, cx);
        workspace
    });
    let panels = workspace.read_with(cx, |workspace, cx| {
        let dock = workspace.dock(DockPosition::Left).unwrap().read(cx);
        let panels = (dock.panel::<ViewContainer>(), dock.panel::<Counter>());
        (panels.0.unwrap(), panels.1.unwrap())
    });
    draw(cx);
    (workspace, panels, cx)
}

#[gpui::test]
fn state_and_bounds_as_ids_or_keys_are_saved_as_they_change(cx: &mut TestAppContext) {
    let (_dir, store) = store();
    let mut app = cx.clone();
    let (workspace, (container, counter), cx) = open_bounds(cx);
    let layout = WindowLayout::new(store.clone(), store.claim_new().unwrap());
    cx.update(|window, cx| workspace.update(cx, |w, cx| w.set_layout(layout, window, cx)));
    draw(cx);
    let saved = |key: &str| store.entries()[0]["docks"]["left"]["panels"][key].clone();
    assert_eq!(saved("state"), Value::Null, "no count yet");

    // Each change is saved with no call from the host: a view's collapse,
    // and the counter's, from none to one and on.
    let stack = container.read_with(cx, |container, _| container.stack().clone());
    stack.update(cx, |stack, cx| stack.set_collapsed(STATE, true, cx));
    draw(cx);
    assert_eq!(saved("bounds")["views"]["state"]["collapsed"], true);
    stack.update(cx, |stack, cx| stack.set_collapsed(BOUNDS, true, cx));
    draw(cx);
    assert_eq!(saved("bounds")["views"]["bounds"]["collapsed"], true);
    counter.update(cx, |counter, cx| counter.set(1, cx));
    draw(cx);
    assert_eq!(
        saved("state"),
        serde_json::json!({"kind": "count", "state": 1})
    );
    counter.update(cx, |counter, cx| counter.set(2, cx));
    draw(cx);
    assert_eq!(
        saved("state"),
        serde_json::json!({"kind": "count", "state": 2})
    );

    // And all come back with a restore.
    let entry = store.entries()[0].clone();
    let (restored, (container, counter), cx) = open_bounds(&mut app);
    let restoring = restored.clone();
    assert!(cx.update(|window, cx| restoring.update(cx, |w, cx| w.restore(&entry, window, cx))));
    draw(cx);
    container.read_with(cx, |container, cx| {
        let stack = container.stack().read(cx);
        assert_eq!(stack.is_collapsed(STATE), Some(true));
        assert_eq!(stack.is_collapsed(BOUNDS), Some(true));
    });
    assert_eq!(counter.read_with(cx, |counter, _| counter.0), Some(2));
}

/// A window that draws `dock`.
struct DockHost(Entity<Dock>);

impl Render for DockHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(self.0.clone())
    }
}

#[gpui::test]
fn a_dock_hears_every_panel_with_state_however_it_came(cx: &mut TestAppContext) {
    // The explorer is in the state the dock is built with; the counter is
    // added to the state behind the dock's back.
    let (host, cx) = cx.add_window_view(|_, cx| {
        let mut state = DockState::new(DockPosition::Left);
        state.add_panel(explorer(cx), cx);
        DockHost(cx.new(|_| Dock::with_state(state)))
    });
    let dock = host.read_with(cx, |host, _| host.0.clone());
    let counter = cx.new(|cx| Counter(None, cx.focus_handle()));
    dock.update(cx, |dock, cx| {
        dock.dock_mut().add_panel(counter.clone(), cx);
        cx.notify();
    });
    draw(cx);
    let explorer = dock.read_with(cx, |dock, _| dock.panel::<ViewContainer>().unwrap());

    let notified = Rc::new(Cell::new(0));
    let count = notified.clone();
    let _observing = cx.update(|_, cx| cx.observe(&dock, move |_, _| count.set(count.get() + 1)));
    let notifies = |cx: &mut VisualTestContext, change: &dyn Fn(&mut VisualTestContext)| {
        let before = notified.get();
        change(cx);
        draw(cx);
        notified.get() > before
    };
    // A view's collapse, a count from none to one and on.
    assert!(notifies(cx, &|cx| {
        let stack = explorer.read_with(cx, |explorer, _| explorer.stack().clone());
        stack.update(cx, |stack, cx| assert!(stack.toggle(OUTLINE, cx)));
    }));
    assert!(notifies(cx, &|cx| counter
        .update(cx, |counter, cx| counter.set(1, cx))));
    assert!(notifies(cx, &|cx| counter
        .update(cx, |counter, cx| counter.set(2, cx))));
    // A notify that doesn't change a panel's state doesn't the dock's.
    assert!(!notifies(cx, &|cx| counter.update(cx, |_, cx| cx.notify())));
    // Nor does a panel the dock no longer has: one replaced by its id.
    let replacement = cx.new(|cx| Counter(Some(5), cx.focus_handle()));
    dock.update(cx, |dock, cx| dock.add_panel(replacement.clone(), cx));
    draw(cx);
    assert!(!notifies(cx, &|cx| counter
        .update(cx, |counter, cx| counter.set(3, cx))));
    assert!(notifies(cx, &|cx| replacement
        .update(cx, |counter, cx| counter.set(6, cx))));
}
