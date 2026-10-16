// SPDX-License-Identifier: LicenseRef-Proprietary

//! Saved layouts: a window with one document in two split panes, each
//! view with its own caret, and a dock, saved and restored into another
//! window: the views share the document again, keep their carets, and one
//! closes without asking while the other shows the document. A window's
//! entry is written in the background as its shape changes, kept when the
//! last window closes and deleted when others stay open.

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, Render, SharedString, Styled as _, Task, TestAppContext,
    VisualTestContext, Window, WindowBounds, div, point, px, size,
};
use kiln_command::Value;
use kiln_core::layout::LayoutStore;
use kiln_shell::{
    Axis, Direction, DockItem, DockPosition, DockState, ItemPane, Member, PaneGroup, PaneItem,
    PaneView, PanelId,
    layout::{self, WindowLayout},
};
use kiln_ui::Icon;

/// The documents on "disk", by name, shared by the views that show them.
#[derive(Default)]
struct Disk(HashMap<String, Rc<RefCell<String>>>);

impl gpui::Global for Disk {}

/// A view of a document, with its own caret.
struct Doc {
    name: SharedString,
    text: Rc<RefCell<String>>,
    caret: usize,
    dirty: bool,
    focus: FocusHandle,
}

impl Doc {
    fn new(name: &str, text: Rc<RefCell<String>>, cx: &mut Context<Self>) -> Self {
        Self {
            name: name.to_owned().into(),
            text,
            caret: 0,
            dirty: false,
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
    const KIND: &'static str = "test.doc";

    fn tab_title(&self, _: &App) -> SharedString {
        self.name.clone()
    }

    fn is_dirty(&self, _: &App) -> bool {
        self.dirty
    }

    fn shares_document(&self, other: &AnyView, cx: &App) -> bool {
        other
            .clone()
            .downcast::<Doc>()
            .is_ok_and(|other| Rc::ptr_eq(&other.read(cx).text, &self.text))
    }

    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let (name, text, dirty) = (self.name.clone(), self.text.clone(), self.dirty);
        Some(cx.new(|cx| Doc {
            dirty,
            ..Doc::new(&name, text, cx)
        }))
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
        let text = cx.global::<Disk>().0.get(&name).cloned();
        // A document that's gone isn't restored.
        Task::ready(text.map(|text| {
            cx.new(|cx| Doc {
                caret,
                ..Doc::new(&name, text, cx)
            })
        }))
    }

    fn apply_state(&mut self, state: &Value, _: &mut Window, _: &mut Context<Self>) {
        self.caret = state["caret"].as_u64().unwrap_or(0) as usize;
    }
}

/// A dock panel.
struct Panel(PanelId, FocusHandle);

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
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

    fn always_on_rail(&self) -> bool {
        self.0.0 == "files"
    }
}

/// A window: a pane group and a dock.
struct Host {
    group: PaneGroup<ItemPane>,
    active: Entity<ItemPane>,
    dock: DockState,
}

impl Host {
    fn new(cx: &mut Context<Self>) -> Self {
        let pane = cx.new(ItemPane::new);
        let mut dock = DockState::new(DockPosition::Right);
        for id in ["files", "log"] {
            let panel = cx.new(|cx| Panel(PanelId(id), cx.focus_handle()));
            dock.add_panel(panel, cx);
        }
        Self {
            group: PaneGroup::new(pane.clone()),
            active: pane,
            dock,
        }
    }

    fn sync_peers(&mut self, cx: &mut Context<Self>) {
        let peers = Rc::new(RefCell::new(
            (self.group.panes().into_iter())
                .map(Entity::downgrade)
                .collect(),
        ));
        for pane in self.group.panes() {
            let peers = Rc::clone(&peers);
            pane.update(cx, |pane, _| pane.pane_mut().set_peers(peers));
        }
    }

    fn entry(&self, cx: &App) -> Value {
        Value::from_iter([
            ("panes", layout::save_panes(&self.group, &self.active, cx)),
            ("dock", self.dock.save()),
        ])
    }
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

fn open(cx: &mut TestAppContext) -> (Entity<Host>, &mut VisualTestContext) {
    cx.add_window_view(|_, cx| Host::new(cx))
}

fn doc(name: &str, caret: usize, cx: &mut VisualTestContext) -> Entity<Doc> {
    let text = cx.update(|_, cx| {
        let disk = cx.default_global::<Disk>();
        disk.0.entry(name.to_owned()).or_default().clone()
    });
    cx.new(|cx| Doc {
        caret,
        ..Doc::new(name, text, cx)
    })
}

fn docs(pane: &Entity<ItemPane>, cx: &mut VisualTestContext) -> Vec<Entity<Doc>> {
    pane.read_with(cx, |pane, _| {
        (pane.pane().items().iter())
            .map(|item| item.downcast::<Doc>().unwrap())
            .collect()
    })
}

#[gpui::test]
fn two_split_views_of_a_document_round_trip(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(Disk::default());
        layout::register_item::<Doc>(cx);
    });
    let (host, cx) = open(cx);
    let a = doc("a.txt", 1, cx);
    let b = doc("b.txt", 0, cx);
    let gone = doc("gone.txt", 0, cx);
    let left = host.read_with(cx, |host, _| host.active.clone());
    left.update(cx, |pane, cx| {
        pane.add_item(Box::new(a.clone()), cx);
        pane.add_item(Box::new(gone), cx);
    });
    let copy = cx.update(|window, cx| a.update(cx, |a, cx| a.split(window, cx)).unwrap());
    copy.update(cx, |copy, _| copy.caret = 3);
    let right = host.update(cx, |host, cx| {
        let right = left.update(cx, |pane, cx| cx.new(|cx| pane.new_peer(cx)));
        host.group.split(&left, right.clone(), Direction::Right);
        host.sync_peers(cx);
        host.active = right.clone();
        right
    });
    right.update(cx, |pane, cx| {
        pane.add_item(Box::new(b), cx);
        pane.add_item(Box::new(copy), cx);
    });
    cx.update(|window, cx| {
        host.update(cx, |host, cx| host.dock.show(PanelId("log"), window, cx));
    });
    let entry = host.read_with(cx, |host, cx| host.entry(cx));
    let tree = &entry["panes"]["tree"];
    assert_eq!(tree["axis"], "horizontal");
    assert_eq!(entry["panes"]["active"], Value::from_iter([1]));
    let documents = |pane: &Value| -> Vec<u64> {
        (pane["pane"]["items"].as_array().unwrap().iter())
            .map(|item| item["document"].as_u64().unwrap())
            .collect()
    };
    let (saved_left, saved_right) = (&tree["members"][0], &tree["members"][1]);
    assert_eq!(documents(saved_left), [0, 1]);
    assert_eq!(
        documents(saved_right),
        [2, 0],
        "the copy shares a's document"
    );
    assert_eq!(saved_right["pane"]["active"], 1);

    // gone.txt is gone by the time it's restored.
    cx.update(|_, cx| cx.global_mut::<Disk>().0.remove("gone.txt"));
    let mut app = cx.cx.clone();
    let (restored, cx) = open(&mut app);
    let filled = cx.update(|window, cx| {
        let mut new_pane = |_: &mut Window, cx: &mut App| cx.new(ItemPane::new);
        let panes = layout::restore_panes(&entry["panes"], &mut new_pane, window, cx).unwrap();
        restored.update(cx, |host, cx| {
            host.group.set_root(panes.root);
            host.active = panes.active;
            host.sync_peers(cx);
            host.dock.restore(&entry["dock"]);
        });
        panes.filled
    });
    let empty = Rc::new(RefCell::new(None));
    let landed = empty.clone();
    cx.spawn(async move |_| *landed.borrow_mut() = Some(filled.await))
        .detach();
    cx.run_until_parked();
    assert_eq!(empty.borrow().as_ref().map(Vec::len), Some(0));
    let (panes, active, ratios) = restored.read_with(cx, |host, _| {
        let panes: Vec<_> = host.group.panes().into_iter().cloned().collect();
        let ratios = match host.group.root() {
            Member::Axis { axis, members } => {
                assert_eq!(*axis, Axis::Horizontal);
                members.ratios().to_vec()
            }
            Member::Pane(_) => panic!("two panes"),
        };
        (panes, host.active.clone(), ratios)
    });
    assert_eq!(panes.len(), 2);
    assert_eq!(active, panes[1]);
    assert_eq!(ratios, [0.5, 0.5]);
    let (left, right) = (docs(&panes[0], cx), docs(&panes[1], cx));
    assert_eq!(left.len(), 1, "gone.txt is dropped");
    let titles = |docs: &[Entity<Doc>], cx: &mut VisualTestContext| -> Vec<String> {
        docs.iter()
            .map(|doc| doc.read_with(cx, |doc, _| doc.name.to_string()))
            .collect()
    };
    assert_eq!(titles(&left, cx), ["a.txt"]);
    assert_eq!(titles(&right, cx), ["b.txt", "a.txt"]);
    let active_item = panes[1].read_with(cx, |pane, _| pane.pane().active());
    assert_eq!(active_item, Some(1));

    // The two views share a's document, each with its own caret.
    let (one, two) = (left[0].clone(), right[1].clone());
    let shared = cx.update(|_, cx| Rc::ptr_eq(&one.read(cx).text, &two.read(cx).text));
    assert!(shared);
    assert_eq!(one.read_with(cx, |doc, _| doc.caret), 1);
    assert_eq!(two.read_with(cx, |doc, _| doc.caret), 3);
    one.update(cx, |doc, _| doc.text.borrow_mut().push_str("edit"));
    assert_eq!(
        two.read_with(cx, |doc, _| doc.text.borrow().clone()),
        "edit"
    );

    // Dirty, one closes without asking: the other shows its document.
    for doc in [&one, &two] {
        doc.update(cx, |doc, _| doc.dirty = true);
    }
    cx.update(|window, cx| panes[0].update(cx, |pane, cx| pane.close_active_item(window, cx)));
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert_eq!(docs(&panes[0], cx).len(), 0);

    // The dock.
    restored.read_with(cx, |host, _| {
        assert_eq!(host.dock.active(), Some(PanelId("log")));
        assert_eq!(host.dock.joined(), [PanelId("log")]);
        assert!(host.dock.is_visible());
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

fn scratch() -> Scratch {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "kiln-layout-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    Scratch(dir)
}

#[gpui::test]
fn entries_save_as_the_shape_changes_and_go_as_windows_close(cx: &mut TestAppContext) {
    let dir = scratch();
    let store = LayoutStore::new(&dir.0, Path::new("/project"));
    let entry = |name: &str, caret: u64| {
        let state = Value::from_iter([("caret", Value::from(caret))]);
        let item = Value::from_iter([("kind", Value::from(name)), ("state", state)]);
        Value::from_iter([("items", Value::from_iter([item]))])
    };
    let saved = |store: &LayoutStore| -> Vec<Value> {
        let mut entries = store.entries();
        for entry in &mut entries {
            let object = entry.as_object_mut().unwrap();
            object.remove("id");
            object.remove("saved");
        }
        entries
    };
    let mut first = WindowLayout::new(store.clone(), store.claim_new().unwrap());
    cx.update(|cx| first.save_if_changed(entry("a", 1), cx));
    cx.run_until_parked();
    assert_eq!(saved(&store), [entry("a", 1)]);
    // A caret alone isn't a change of shape.
    cx.update(|cx| first.save_if_changed(entry("a", 2), cx));
    cx.run_until_parked();
    assert_eq!(saved(&store), [entry("a", 1)]);
    cx.update(|cx| first.save_if_changed(entry("b", 2), cx));
    cx.run_until_parked();
    assert_eq!(saved(&store), [entry("b", 2)]);

    // Closing while another window stays deletes the entry and frees its
    // id; closing the last keeps it.
    let second = WindowLayout::new(store.clone(), store.claim_new().unwrap());
    let id = first.id().unwrap();
    cx.update(|cx| first.close(entry("b", 3), true, cx));
    cx.run_until_parked();
    assert_eq!(saved(&store), Vec::<Value>::new());
    assert!(store.claim(id).unwrap().is_some());
    cx.update(|cx| second.close(entry("c", 0), false, cx));
    cx.run_until_parked();
    assert_eq!(saved(&store), [entry("c", 0)]);
    assert_eq!(store.restorable().len(), 1, "its claim is released");

    // Bounds round trip.
    let bounds = WindowBounds::Windowed(gpui::Bounds {
        origin: point(px(10.), px(20.)),
        size: size(px(800.), px(600.)),
    });
    assert_eq!(
        layout::restore_bounds(&layout::save_bounds(bounds)),
        Some(bounds)
    );
}
