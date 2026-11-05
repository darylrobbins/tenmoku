// SPDX-License-Identifier: Apache-2.0

//! A pane item held through its handle: what it says forwarded, its type
//! got back, a split sharing its document, the default close veto for
//! unsaved changes, notifications heard, and the status items and crumbs
//! the active item gives.

use std::{cell::Cell, path::PathBuf, rc::Rc};

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, IntoElement, Render, SharedString,
    TestAppContext, Window, div,
};
use kiln_command::Value;
use kiln_shell::{
    CloseAnswer, ItemCrumb, ItemHandle, ItemId, PaneItem, StatusItem, StatusItems, StatusSide,
    crumbs::path_crumbs,
};
use kiln_ui::CrumbKind;

/// One note's text: shared by the notes that show it.
type Document = Rc<std::cell::RefCell<String>>;

/// A note, which may be split into a second view of its document.
struct Note {
    document: Document,
    title: SharedString,
    path: Option<PathBuf>,
    dirty: bool,
    focus: FocusHandle,
}

impl Note {
    fn new(title: &str, cx: &mut Context<Self>) -> Self {
        Self {
            document: Document::default(),
            title: title.to_owned().into(),
            path: Some(PathBuf::from(format!("/notes/{title}.txt"))),
            dirty: false,
            focus: cx.focus_handle(),
        }
    }
}

impl Render for Note {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl Focusable for Note {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PaneItem for Note {
    const KIND: &'static str = "test.note";

    fn tab_title(&self, _: &gpui::App) -> SharedString {
        self.title.clone()
    }

    fn is_dirty(&self, _: &gpui::App) -> bool {
        self.dirty
    }

    fn shares_document(&self, other: &gpui::AnyView, cx: &gpui::App) -> bool {
        other
            .clone()
            .downcast::<Note>()
            .is_ok_and(|other| Rc::ptr_eq(&other.read(cx).document, &self.document))
    }

    fn path(&self, _: &gpui::App) -> Option<&std::path::Path> {
        self.path.as_deref()
    }

    fn breadcrumbs(&self, _: &gpui::App) -> Vec<ItemCrumb> {
        vec![ItemCrumb::new(self.title.clone(), CrumbKind::Symbol)]
    }

    fn status_items(&self, _: &gpui::App) -> Vec<StatusItem> {
        let words = self.document.borrow().split_whitespace().count();
        vec![StatusItem::new(
            "note.words",
            StatusSide::Right,
            0,
            format!("{words} words"),
        )]
    }

    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let document = self.document.clone();
        let title = self.title.clone();
        Some(cx.new(|cx| Self {
            document,
            title,
            path: self.path.clone(),
            dirty: self.dirty,
            focus: cx.focus_handle(),
        }))
    }

    fn serialize(&self, _: &gpui::App) -> Option<Value> {
        Some(Value::from(self.title.to_string()))
    }
}

/// A view that's no note.
struct Other {
    focus: FocusHandle,
}

impl Render for Other {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl Focusable for Other {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PaneItem for Other {
    const KIND: &'static str = "test.other";

    fn tab_title(&self, _: &gpui::App) -> SharedString {
        "Other".into()
    }
}

#[gpui::test]
fn the_handle_forwards_and_downcasts(cx: &mut TestAppContext) {
    let (note, cx) = cx.add_window_view(|_, cx| Note::new("todo", cx));
    let item: Box<dyn ItemHandle> = Box::new(note.clone());
    assert_eq!(item.id(), ItemId(note.entity_id()));
    assert_eq!(item.kind(), "test.note");
    cx.update(|_, cx| {
        assert_eq!(item.tab_title(cx), "todo");
        assert_eq!(item.window_title(cx), "todo", "the tab's by default");
        assert_eq!(item.tab_icon(cx), None);
        assert_eq!(item.path(cx), Some(PathBuf::from("/notes/todo.txt")));
        let crumbs = item.breadcrumbs(cx);
        assert_eq!(crumbs.len(), 1);
        assert_eq!(
            (crumbs[0].label.as_ref(), crumbs[0].kind),
            ("todo", CrumbKind::Symbol)
        );
        assert_eq!(item.serialize(cx), Some(Value::from("todo")));
        assert_eq!(item.item_focus_handle(cx), note.read(cx).focus);
    });

    assert_eq!(item.downcast::<Note>(), Some(note.clone()));
    assert_eq!(item.downcast_ref::<Note>(), Some(&note));
    assert!(item.downcast::<Other>().is_none());
    assert!(item.downcast_ref::<Other>().is_none());
    let clone = item.clone();
    assert_eq!(clone.id(), item.id());
    assert_eq!(
        format!("{clone:?}"),
        format!("ItemHandle {{ kind: \"test.note\", id: {:?} }}", item.id())
    );
}

#[gpui::test]
fn a_split_shares_the_document(cx: &mut TestAppContext) {
    let (note, cx) = cx.add_window_view(|_, cx| Note::new("todo", cx));
    let item: Box<dyn ItemHandle> = Box::new(note.clone());
    let split = cx
        .update(|window, cx| item.split(window, cx))
        .expect("splits");
    assert_ne!(split.id(), item.id());
    let other: Box<dyn ItemHandle> = Box::new(cx.update(|_, cx| {
        cx.new(|cx| Other {
            focus: cx.focus_handle(),
        })
    }));
    let unrelated: Box<dyn ItemHandle> =
        Box::new(cx.update(|_, cx| cx.new(|cx| Note::new("todo", cx))));
    cx.update(|_, cx| {
        assert!(item.shares_document(&*split, cx));
        assert!(split.shares_document(&*item, cx));
        assert!(!item.shares_document(&*item, cx), "not with itself");
        assert!(!item.shares_document(&*other, cx));
        assert!(!item.shares_document(&*unrelated, cx), "another document");
        assert!(!other.shares_document(&*item, cx), "the default");
    });

    // A change through one shows in the other's status.
    note.update(cx, |note, cx| {
        note.document.borrow_mut().push_str("one two three");
        cx.notify();
    });
    let status = cx.update(|_, cx| split.status_items(cx));
    assert_eq!(status[0].text, "3 words");
    assert_eq!(
        cx.update(|window, cx| other.split(window, cx)).map(|_| ()),
        None
    );
}

#[gpui::test]
fn closing_keeps_unsaved_changes_by_default(cx: &mut TestAppContext) {
    let (note, cx) = cx.add_window_view(|_, cx| Note::new("todo", cx));
    let item: Box<dyn ItemHandle> = Box::new(note.clone());
    let answer = cx.update(|window, cx| item.may_close(window, cx));
    assert!(matches!(answer, CloseAnswer::Close), "{answer:?}");
    note.update(cx, |note, _| note.dirty = true);
    assert!(cx.update(|_, cx| item.is_dirty(cx)));
    let answer = cx.update(|window, cx| item.may_close(window, cx));
    assert!(matches!(answer, CloseAnswer::Keep), "{answer:?}");
    // Nothing to save, by default.
    assert!(cx.update(|window, cx| item.save(window, cx)).is_none());
}

#[gpui::test]
fn observing_hears_notifications(cx: &mut TestAppContext) {
    let (note, cx) = cx.add_window_view(|_, cx| Note::new("todo", cx));
    let item: Box<dyn ItemHandle> = Box::new(note.clone());
    let heard = Rc::new(Cell::new(0));
    let subscription = cx.update(|_, cx| {
        let heard = heard.clone();
        item.observe(Box::new(move |_| heard.set(heard.get() + 1)), cx)
    });
    note.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(heard.get(), 1);
    drop(subscription);
    note.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(heard.get(), 1, "not once dropped");
}

#[gpui::test]
fn the_active_items_status_and_crumbs(cx: &mut TestAppContext) {
    let (note, cx) = cx.add_window_view(|_, cx| Note::new("todo", cx));
    let item: Box<dyn ItemHandle> = Box::new(note.clone());
    let mut host = StatusItems::default();
    host.set(StatusItem::new(
        "host.ready",
        StatusSide::Right,
        10,
        "Ready",
    ));
    let texts = |items: Vec<StatusItem>| -> Vec<String> {
        items
            .into_iter()
            .map(|item| item.text.to_string())
            .collect()
    };
    cx.update(|_, cx| {
        assert_eq!(
            texts(host.items_for(Some(&*item), cx)),
            ["0 words", "Ready"]
        );
        assert_eq!(texts(host.items_for(None, cx)), ["Ready"], "no item");
    });
    // A host item with the item's id shows in its place.
    host.set(StatusItem::new(
        "note.words",
        StatusSide::Right,
        0,
        "Host words",
    ));
    cx.update(|_, cx| {
        assert_eq!(
            texts(host.items_for(Some(&*item), cx)),
            ["Host words", "Ready"]
        );
    });

    // Its crumbs, as kiln-ui draws them.
    let crumbs: Vec<_> = cx.update(|_, cx| {
        item.breadcrumbs(cx)
            .into_iter()
            .map(ItemCrumb::into_crumb)
            .collect()
    });
    assert_eq!(crumbs.len(), 1);
    assert_eq!(
        (crumbs[0].label.as_ref(), crumbs[0].kind),
        ("todo", CrumbKind::Symbol)
    );
    // Its path as path crumbs, under the root.
    let path = cx.update(|_, cx| item.path(cx));
    let path_crumbs = path_crumbs(path.as_deref(), Some(std::path::Path::new("/notes")));
    assert_eq!(path_crumbs.len(), 1);
    assert_eq!(path_crumbs[0].label, "todo.txt");
}

#[gpui::test]
fn observing_in_a_window(cx: &mut TestAppContext) {
    let (note, cx) = cx.add_window_view(|_, cx| Note::new("todo", cx));
    let item: Box<dyn ItemHandle> = Box::new(note.clone());
    let heard = Rc::new(Cell::new(0));
    let _subscription = cx.update(|window, cx| {
        let heard = heard.clone();
        let changed = move |window: &mut Window, _: &mut gpui::App| {
            assert!(window.viewport_size().width > gpui::px(0.));
            heard.set(heard.get() + 1);
        };
        item.observe_in(window, Box::new(changed), cx)
    });
    note.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(heard.get(), 1);
}
