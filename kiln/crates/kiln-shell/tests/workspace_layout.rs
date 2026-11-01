// SPDX-License-Identifier: LicenseRef-Proprietary

//! The workspace's own saved layout: split panes, their items and the
//! docks, restored into a new window as they were; each change of shape
//! saved with no call from the host, but none while a restore reads; the
//! app's keys saved and read back, its typed state before the items and
//! not from a rejected restore; the full entry saved at quit and as
//! the window closes or the workspace goes, the last window's kept; and
//! the panes a restore leaves empty closing, focus going to the active
//! item.

use crate::support;

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    slice,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Task, TestAppContext,
    VisualTestContext, Window, div, px,
};
use kiln_command::Value;
use kiln_shell::{
    Destination, Direction, DockItem, DockPosition, DockView as _, ItemHandle, ItemPane, Member,
    PaneEvent, PaneItem, PaneView as _, PanelId, Workspace,
    layout::{self, LayoutStore, WindowLayout},
};
use kiln_ui::Icon;
use support::{APP, linux};

/// The documents on "disk", by name, and whether reading one waits a
/// second.
#[derive(Default)]
struct Disk {
    docs: HashMap<String, ()>,
    slow: bool,
}

impl gpui::Global for Disk {}

/// A view of a document, with its own caret.
struct Doc {
    name: SharedString,
    caret: usize,
    focus: FocusHandle,
}

impl Doc {
    fn new(name: &str, caret: usize, cx: &mut Context<Self>) -> Self {
        Self {
            name: name.to_owned().into(),
            caret,
            focus: cx.focus_handle(),
        }
    }
}

impl Render for Doc {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full()
    }
}

impl Focusable for Doc {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PaneItem for Doc {
    const KIND: &'static str = "test.layout_doc";

    fn tab_title(&self, _: &App) -> SharedString {
        self.name.clone()
    }

    fn shares_document(&self, other: &gpui::AnyView, cx: &App) -> bool {
        (other.clone().downcast::<Doc>()).is_ok_and(|other| other.read(cx).name == self.name)
    }

    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let (name, caret) = (self.name.clone(), self.caret);
        Some(cx.new(|cx| Doc::new(&name, caret, cx)))
    }

    fn serialize(&self, _: &App) -> Option<Value> {
        Some(Value::from_iter([
            ("name", Value::from(self.name.as_ref())),
            ("caret", Value::from(self.caret)),
        ]))
    }

    fn restore(state: Value, _: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        let name = state["name"].as_str().unwrap_or_default().to_owned();
        let caret = state["caret"].as_u64().unwrap_or(0) as usize;
        let disk = cx.global::<Disk>();
        // A document that's gone isn't restored.
        let there = disk.docs.contains_key(&name);
        let wait = (disk.slow).then(|| cx.background_executor().timer(Duration::from_secs(1)));
        cx.spawn(async move |cx| {
            if let Some(wait) = wait {
                wait.await;
            }
            there.then(|| cx.new(|cx| Doc::new(&name, caret, cx)))
        })
    }

    fn apply_state(&mut self, state: &Value, _: &mut Window, _: &mut Context<Self>) {
        self.caret = state["caret"].as_u64().unwrap_or(0) as usize;
    }
}

/// A dock panel.
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
        Icon::Files
    }
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
        "kiln-workspace-layout-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let store = LayoutStore::new(&dir, Path::new("/project"));
    (Scratch(dir), store)
}

/// The store's entries, without what the store adds.
fn saved(store: &LayoutStore) -> Vec<Value> {
    let mut entries = store.entries();
    for entry in &mut entries {
        let object = entry.as_object_mut().unwrap();
        object.remove("id");
        object.remove("saved");
        object.remove("session");
    }
    entries
}

fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let docs = ["a", "b", "c"].map(|name| (name.to_owned(), ())).into();
        cx.set_global(Disk { docs, slow: false });
        layout::register_item::<Doc>(cx);
    });
}

struct Opened<'a> {
    workspace: Entity<Workspace>,
    cx: &'a mut VisualTestContext,
}

/// A workspace with a left dock of `files` and a bottom one of `log` and
/// `terminal`.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let (workspace, cx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::item_panes(APP, window, cx).capabilities(linux(), cx);
        for (position, id) in [
            (DockPosition::Left, "files"),
            (DockPosition::Bottom, "log"),
            (DockPosition::Bottom, "terminal"),
        ] {
            let panel = cx.new(|cx| Panel(PanelId(id), cx.focus_handle()));
            workspace.add_panel(position, panel, window, cx);
        }
        workspace
    });
    let mut opened = Opened { workspace, cx };
    opened.draw();
    opened
}

impl Opened<'_> {
    fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
        self.cx.run_until_parked();
    }

    fn update<R>(
        &mut self,
        f: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) -> R,
    ) -> R {
        let workspace = self.workspace.clone();
        let result = (self.cx).update(|window, cx| workspace.update(cx, |w, cx| f(w, window, cx)));
        self.draw();
        result
    }

    fn add(&mut self, name: &str) -> Entity<Doc> {
        let doc = self.cx.new(|cx| Doc::new(name, 0, cx));
        let item = Box::new(doc.clone());
        self.update(|workspace, window, cx| workspace.add_item(item, window, cx));
        doc
    }

    fn set_layout(&mut self, store: &LayoutStore) {
        let layout = WindowLayout::new(store.clone(), store.claim_new().unwrap());
        self.update(|workspace, window, cx| workspace.set_layout(layout, window, cx));
    }

    fn entry(&mut self) -> Value {
        (self.workspace).read_with(self.cx, |workspace, cx| workspace.entry(cx))
    }

    fn restore(&mut self, entry: &Value) -> bool {
        self.update(|workspace, window, cx| workspace.restore(entry, window, cx))
    }

    fn panes(&mut self) -> Vec<Entity<ItemPane>> {
        (self.workspace).read_with(self.cx, |workspace, _| {
            workspace.group().panes().into_iter().cloned().collect()
        })
    }

    fn docs(&mut self, pane: &Entity<ItemPane>) -> Vec<Entity<Doc>> {
        pane.read_with(self.cx, |pane, _| {
            (pane.pane().items().iter())
                .map(|item| item.downcast::<Doc>().unwrap())
                .collect()
        })
    }

    fn names(&mut self, pane: &Entity<ItemPane>) -> Vec<String> {
        let docs = self.docs(pane);
        (docs.iter())
            .map(|doc| doc.read_with(self.cx, |doc, _| doc.name.to_string()))
            .collect()
    }

    fn active_pane(&mut self) -> Entity<ItemPane> {
        (self.workspace).read_with(self.cx, |workspace, _| workspace.active_pane().clone())
    }

    /// The dock at `position`: whether it shows, its active panel and its
    /// size.
    fn dock(&mut self, position: DockPosition) -> (bool, Option<PanelId>, f32) {
        (self.workspace).read_with(self.cx, |workspace, cx| {
            let dock = workspace.dock(position).unwrap().read(cx).dock();
            (dock.is_visible(), dock.active(), dock.current_size().into())
        })
    }

    fn show(&mut self, position: DockPosition, panel: &'static str) {
        self.update(|workspace, window, cx| {
            let dock = workspace.dock(position).unwrap().clone();
            dock.update(cx, |dock, cx| dock.show(PanelId(panel), window, cx));
        });
    }

    fn toggle(&mut self, position: DockPosition) {
        self.update(|workspace, window, cx| workspace.toggle_dock(position, window, cx));
    }

    fn focused(&mut self, doc: &Entity<Doc>) -> bool {
        let focus = doc.read_with(self.cx, |doc, _| doc.focus.clone());
        self.cx.update(|window, _| focus.is_focused(window))
    }

    /// A click in `pane`, as far as the workspace hears it.
    fn click(&mut self, pane: &Entity<ItemPane>) {
        pane.update(self.cx, |_, cx| cx.emit(PaneEvent::Activated));
        self.draw();
    }

    fn close_window(&mut self) {
        self.cx.update(|window, _| window.remove_window());
        self.cx.run_until_parked();
    }
}

/// The tree's shape: each axis's way, around its members.
fn shape(member: &Member<ItemPane>) -> String {
    match member {
        Member::Pane(_) => "pane".into(),
        Member::Axis { axis, members } => {
            let members: Vec<_> = members.iter().map(shape).collect();
            format!("{axis:?}({})", members.join(", "))
        }
    }
}

/// Two panes side by side, the right one active, with a, b | b, c, and
/// the left dock showing at 300 px, the bottom one hidden on `terminal`
/// at 200 px.
fn arrange(s: &mut Opened) {
    s.add("a");
    s.add("b");
    s.update(|workspace, window, cx| workspace.split(Direction::Right, window, cx));
    s.add("c");
    s.show(DockPosition::Left, "files");
    s.update(|workspace, _, cx| workspace.set_dock_size(DockPosition::Left, px(300.), cx));
    s.show(DockPosition::Bottom, "terminal");
    s.update(|workspace, _, cx| workspace.set_dock_size(DockPosition::Bottom, px(200.), cx));
    s.toggle(DockPosition::Bottom);
}

#[gpui::test]
fn a_workspace_restores_into_a_new_window_as_it_was(cx: &mut TestAppContext) {
    setup(cx);
    let mut app = cx.clone();
    let mut s = open(&mut app);
    arrange(&mut s);
    let entry = s.entry();
    let docks = &entry["docks"];
    assert_eq!(docks["left"]["visible"], true);
    assert_eq!(docks["bottom"]["active"], "terminal");
    assert!(docks.get("right").is_none(), "no dock there");

    let mut app = cx.clone();
    let mut t = open(&mut app);
    assert!(t.restore(&entry));
    let panes = t.panes();
    let root = t.workspace.read_with(t.cx, |w, _| shape(w.group().root()));
    assert_eq!(root, "Horizontal(pane, pane)");
    assert_eq!(t.active_pane(), panes[1]);
    assert_eq!(t.names(&panes[0]), ["a", "b"]);
    assert_eq!(t.names(&panes[1]), ["b", "c"]);
    let active = panes[1].read_with(t.cx, |pane, _| pane.pane().active());
    assert_eq!(active, Some(1));
    assert_eq!(
        t.dock(DockPosition::Left),
        (true, Some(PanelId("files")), 300.)
    );
    assert_eq!(
        t.dock(DockPosition::Bottom),
        (false, Some(PanelId("terminal")), 200.)
    );
    assert_eq!(t.entry(), entry);

    // An entry with no panes restores nothing.
    let none = Value::from_iter([("docks", Value::Null)]);
    assert!(!t.restore(&none));
    assert_eq!(t.panes(), panes);
}

#[gpui::test]
fn each_change_of_shape_saves_but_none_while_a_restore_reads(cx: &mut TestAppContext) {
    setup(cx);
    let (_dir, store) = store();
    let mut s = open(cx);
    s.add("a");
    s.add("b");
    s.set_layout(&store);
    let mut last = s.entry();
    assert_eq!(saved(&store), [last.clone()]);
    let mut saves = |s: &mut Opened, what: &str| {
        let entry = s.entry();
        assert_ne!(entry, last, "{what} changes the entry");
        assert_eq!(saved(&store), slice::from_ref(&entry), "{what} is saved");
        last = entry;
    };

    let left = s.active_pane();
    let right = s.update(|workspace, window, cx| workspace.split(Direction::Right, window, cx));
    saves(&mut s, "a split");
    let a = s.docs(&left)[0].clone();
    let dropped = kiln_shell::pane::TabDropped {
        dragged: kiln_shell::pane::DraggedTab {
            pane: left.downgrade(),
            item: ItemHandle::id(&a),
        },
        destination: Destination::Pane {
            pane: right.clone(),
            index: None,
        },
    };
    right.update(s.cx, |_, cx| cx.emit(dropped));
    s.draw();
    assert_eq!(s.names(&right), ["b", "a"]);
    saves(&mut s, "a tab moved");
    s.update(|workspace, window, cx| workspace.close_pane(&left, window, cx));
    assert_eq!(s.panes(), slice::from_ref(&right));
    saves(&mut s, "a pane closed");
    s.toggle(DockPosition::Left);
    saves(&mut s, "a dock toggled");
    s.update(|workspace, _, cx| workspace.set_dock_size(DockPosition::Left, px(321.), cx));
    saves(&mut s, "a dock resized");
    s.show(DockPosition::Bottom, "terminal");
    saves(&mut s, "a panel shown");

    // A caret alone isn't a change of shape.
    a.update(s.cx, |doc, cx| {
        doc.caret = 7;
        cx.notify();
    });
    s.update(|_, _, cx| cx.notify());
    assert_ne!(s.entry(), last);
    assert_eq!(saved(&store), [last.clone()]);

    // While a restore reads, nothing is saved, even as the docks change;
    // once it's done, it is.
    let mut app = cx.clone();
    let mut t = open(&mut app);
    t.cx.update(|_, cx| cx.global_mut::<Disk>().slow = true);
    let restored = last.clone();
    t.set_layout(&store);
    let before = saved(&store);
    assert_eq!(before.len(), 2);
    assert!(t.restore(&restored));
    assert!(t.workspace.read_with(t.cx, |w, _| w.restoring()));
    t.toggle(DockPosition::Left);
    t.update(|_, _, cx| cx.notify());
    assert_eq!(saved(&store), before, "nothing saved while reading");
    t.cx.executor().advance_clock(Duration::from_secs(1));
    t.draw();
    assert!(!t.workspace.read_with(t.cx, |w, _| w.restoring()));
    let pane = t.active_pane();
    assert_eq!(t.names(&pane), ["b", "a"]);
    let now = t.entry();
    assert!(saved(&store).contains(&now), "saved once it's read");
}

#[gpui::test]
fn the_app_keys_are_saved_and_read_back(cx: &mut TestAppContext) {
    setup(cx);
    let (_dir, store) = store();
    let mut s = open(cx);
    s.add("a");
    s.update(|workspace, _, _| {
        workspace.on_save(|_| vec![("breadcrumbs", Value::from(true))]);
        workspace.on_save(|_| vec![("theme", Value::from("dark"))]);
    });
    s.set_layout(&store);
    let entry = s.entry();
    assert_eq!(entry["breadcrumbs"], true);
    assert_eq!(saved(&store), slice::from_ref(&entry));

    // The last window closes; the next restores what it kept.
    s.close_window();
    let (_, restorable) = store.restorable().pop().unwrap();
    assert_eq!(restorable["breadcrumbs"], true);
    assert_eq!(restorable["theme"], "dark");
    let mut app = cx.clone();
    let mut t = open(&mut app);
    assert!(t.restore(&restorable));
    let pane = t.active_pane();
    assert_eq!(t.names(&pane), ["a"]);
}

/// The documents on disk, kept by the app itself in the entry.
fn persist_disk(workspace: &mut Workspace) {
    let save = |cx: &App| {
        let mut names: Vec<String> = cx.global::<Disk>().docs.keys().cloned().collect();
        names.sort();
        names
    };
    workspace.persist("disk", save, |names: Vec<String>, _, cx| {
        let disk = cx.global_mut::<Disk>();
        disk.docs = names.into_iter().map(|name| (name, ())).collect();
    });
}

#[gpui::test]
fn the_apps_state_comes_back_before_the_items_and_not_from_a_rejected_restore(
    cx: &mut TestAppContext,
) {
    setup(cx);
    let mut s = open(cx);
    s.add("a");
    s.update(|workspace, _, _| persist_disk(workspace));
    let entry = s.entry();
    assert_eq!(entry["disk"], Value::from(vec!["a", "b", "c"]));

    // The next run's disk has none of them until the entry says so.
    cx.update(|cx| cx.global_mut::<Disk>().docs.clear());
    let mut app = cx.clone();
    let mut t = open(&mut app);
    let kept = t.add("kept");
    t.update(|workspace, _, _| persist_disk(workspace));
    let pane = t.active_pane();

    // A tree that isn't one: nothing comes back, and nothing changes.
    let mut bad = entry.clone();
    bad["panes"]["tree"] = Value::Null;
    assert!(!t.restore(&bad));
    assert!(cx.read(|cx| cx.global::<Disk>().docs.is_empty()));
    assert_eq!(t.panes(), slice::from_ref(&pane));
    assert_eq!(t.docs(&pane), [kept]);

    // The disk comes back before the item that reads it.
    assert!(t.restore(&entry));
    let pane = t.active_pane();
    assert_eq!(t.names(&pane), ["a"]);
    assert_eq!(cx.read(|cx| cx.global::<Disk>().docs.len()), 3);
}

#[gpui::test]
fn the_full_entry_is_saved_at_quit(cx: &mut TestAppContext) {
    setup(cx);
    let (_dir, store) = store();
    let mut app = cx.clone();
    let mut s = open(&mut app);
    let a = s.add("a");
    s.set_layout(&store);
    let mut app = cx.clone();
    let mut t = open(&mut app);
    t.add("b");
    t.set_layout(&store);
    // A caret alone isn't saved as it changes...
    a.update(s.cx, |doc, _| doc.caret = 5);
    s.update(|_, _, cx| cx.notify());
    let carets = |store: &LayoutStore| -> Vec<Value> {
        (saved(store).iter())
            .map(|entry| entry["panes"]["tree"]["pane"]["items"][0]["state"]["caret"].clone())
            .collect()
    };
    assert_eq!(carets(&store), [0, 0]);

    // ...but at quit, both windows' entries are kept, in full.
    cx.quit();
    let mut carets = carets(&store);
    carets.sort_by_key(|caret| caret.as_u64());
    assert_eq!(carets, [0, 5]);
}

#[gpui::test]
fn a_closed_window_keeps_its_entry_only_if_it_was_the_last(cx: &mut TestAppContext) {
    setup(cx);
    let (_dir, store) = store();
    let mut app = cx.clone();
    let mut s = open(&mut app);
    s.add("a");
    s.set_layout(&store);
    let mut app = cx.clone();
    let mut t = open(&mut app);
    let b = t.add("b");
    t.set_layout(&store);
    assert_eq!(saved(&store).len(), 2);

    // Another window stays open: the entry goes.
    s.close_window();
    let names: Vec<_> = (saved(&store).iter())
        .map(|entry| entry["panes"]["tree"]["pane"]["items"][0]["state"]["name"].clone())
        .collect();
    assert_eq!(names, ["b"]);

    // The last window's is kept, in full.
    b.update(t.cx, |doc, _| doc.caret = 9);
    t.close_window();
    let entries = saved(&store);
    assert_eq!(entries.len(), 1);
    let item = &entries[0]["panes"]["tree"]["pane"]["items"][0]["state"];
    assert_eq!((&item["name"], &item["caret"]), (&"b".into(), &9.into()));
}

/// a above c, c's pane active, and c gone from disk: the entry.
fn a_above_a_gone_c(cx: &mut TestAppContext) -> Value {
    let mut app = cx.clone();
    let mut s = open(&mut app);
    let bottom = s.update(|workspace, window, cx| workspace.split(Direction::Down, window, cx));
    s.add("c");
    let top = s.panes()[0].clone();
    s.click(&top);
    s.add("a");
    s.click(&bottom);
    let entry = s.entry();
    assert_eq!(entry["panes"]["active"], Value::from_iter([1]));
    s.cx.update(|_, cx| cx.global_mut::<Disk>().docs.remove("c"));
    entry
}

#[gpui::test]
fn panes_a_restore_leaves_empty_close_and_focus_goes_to_the_active_item(cx: &mut TestAppContext) {
    setup(cx);
    let entry = a_above_a_gone_c(cx);
    let mut app = cx.clone();
    let mut t = open(&mut app);
    t.cx.update(|_, cx| cx.global_mut::<Disk>().slow = true);
    assert!(t.restore(&entry));
    assert_eq!(t.panes().len(), 2, "both panes, empty, while reading");
    t.cx.executor().advance_clock(Duration::from_secs(1));
    t.draw();
    let panes = t.panes();
    assert_eq!(panes.len(), 1, "the pane left empty closed");
    assert_eq!(t.active_pane(), panes[0]);
    assert_eq!(t.names(&panes[0]), ["a"]);
    let a = t.docs(&panes[0])[0].clone();
    assert!(t.focused(&a), "focus is on the active item");
}

#[gpui::test]
fn a_pane_given_an_item_while_a_restore_reads_stays(cx: &mut TestAppContext) {
    setup(cx);
    let entry = a_above_a_gone_c(cx);
    let mut app = cx.clone();
    let mut t = open(&mut app);
    t.cx.update(|_, cx| cx.global_mut::<Disk>().slow = true);
    assert!(t.restore(&entry));
    // Opened in the active pane, whose saved c is gone.
    let d = t.add("d");
    t.cx.executor().advance_clock(Duration::from_secs(1));
    t.draw();
    let panes = t.panes();
    assert_eq!(panes.len(), 2, "the pane with d stays");
    assert_eq!(t.names(&panes[0]), ["a"]);
    assert_eq!(t.names(&panes[1]), ["d"]);
    assert_eq!(t.active_pane(), panes[1]);
    assert!(t.focused(&d));
}

#[gpui::test]
fn a_tab_reordered_in_its_pane_is_saved(cx: &mut TestAppContext) {
    setup(cx);
    let (_dir, store) = store();
    let mut s = open(cx);
    s.add("a");
    s.add("b");
    s.add("c");
    let pane = s.active_pane();
    s.update(|_, window, cx| pane.update(cx, |pane, cx| pane.activate_item(1, window, cx)));
    s.set_layout(&store);
    let before = s.entry();
    assert_eq!(saved(&store), slice::from_ref(&before));

    // a before c: the documents are still numbered 0, 1, 2 in order, with
    // the second active.
    let a = s.docs(&pane)[0].clone();
    let dropped = kiln_shell::pane::TabDropped {
        dragged: kiln_shell::pane::DraggedTab {
            pane: pane.downgrade(),
            item: ItemHandle::id(&a),
        },
        destination: Destination::Pane {
            pane: pane.clone(),
            index: Some(2),
        },
    };
    pane.update(s.cx, |_, cx| cx.emit(dropped));
    s.draw();
    assert_eq!(s.names(&pane), ["b", "a", "c"]);
    let after = s.entry();
    assert_eq!(layout::shape(&after), layout::shape(&before));
    assert_eq!(saved(&store), [after]);
}

/// A window whose root holds a workspace, which it can let go of first.
struct Host(Option<Entity<Workspace>>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().children(self.0.clone())
    }
}

/// A window whose root holds a workspace of a, its layout set.
fn hosted(cx: &mut TestAppContext, store: &LayoutStore) -> (Entity<Host>, VisualTestContext) {
    let layout = WindowLayout::new(store.clone(), store.claim_new().unwrap());
    let mut app = cx.clone();
    let (host, cx) = app.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| {
            let mut workspace = Workspace::item_panes(APP, window, cx);
            let a = cx.new(|cx| Doc::new("a", 0, cx));
            workspace.add_item(Box::new(a), window, cx);
            workspace.set_layout(layout, window, cx);
            workspace
        });
        Host(Some(workspace))
    });
    cx.run_until_parked();
    (host, cx.clone())
}

#[gpui::test]
fn a_workspace_going_before_its_window_keeps_its_entry_if_it_was_the_last(cx: &mut TestAppContext) {
    setup(cx);
    let (_dir, store) = store();
    let (first, mut first_cx) = hosted(cx, &store);
    let (second, mut second_cx) = hosted(cx, &store);
    assert_eq!(saved(&store).len(), 2);

    // Another window stays open: the entry goes.
    first.update(&mut first_cx, |host, cx| {
        host.0 = None;
        cx.notify();
    });
    first_cx.run_until_parked();
    assert_eq!(saved(&store).len(), 1);

    // The first window's gone too: the last workspace keeps its entry
    // though its own window is still open.
    first_cx.update(|window, _| window.remove_window());
    first_cx.run_until_parked();
    second.update(&mut second_cx, |host, cx| {
        host.0 = None;
        cx.notify();
    });
    second_cx.run_until_parked();
    assert_eq!(saved(&store).len(), 1);
    assert_eq!(store.restorable().len(), 1, "its claim is released");
}
