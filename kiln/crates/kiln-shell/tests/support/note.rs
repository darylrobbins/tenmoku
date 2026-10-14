// SPDX-License-Identifier: LicenseRef-Proprietary

//! A note: a pane item over a document that a split shares.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, Render, SharedString, Styled as _, VisualTestContext,
    Window, div,
};
use kiln_shell::PaneItem;

/// One note's text, shared by the notes that show it.
pub(crate) type Document = Rc<RefCell<String>>;

pub(crate) struct Note {
    pub(crate) document: Document,
    pub(crate) title: SharedString,
    pub(crate) dirty: bool,
    pub(crate) tint: Option<Hsla>,
    pub(crate) focus: FocusHandle,
}

impl Render for Note {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full()
    }
}

impl Focusable for Note {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PaneItem for Note {
    const KIND: &'static str = "test.note";

    fn tab_title(&self, _: &App) -> SharedString {
        self.title.clone()
    }

    fn is_dirty(&self, _: &App) -> bool {
        self.dirty
    }

    fn tab_title_color(&self, _: &App) -> Option<Hsla> {
        self.tint
    }

    fn shares_document(&self, other: &AnyView, cx: &App) -> bool {
        other
            .clone()
            .downcast::<Note>()
            .is_ok_and(|other| Rc::ptr_eq(&other.read(cx).document, &self.document))
    }

    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let (document, title) = (self.document.clone(), self.title.clone());
        Some(cx.new(|cx| Note {
            document,
            title,
            dirty: false,
            tint: None,
            focus: cx.focus_handle(),
        }))
    }
}

/// A note titled `title`, over a document of its own.
pub(crate) fn note(title: &str, cx: &mut VisualTestContext) -> Entity<Note> {
    let title = SharedString::from(title.to_owned());
    cx.new(|cx| Note {
        document: Document::default(),
        title,
        dirty: false,
        tint: None,
        focus: cx.focus_handle(),
    })
}
