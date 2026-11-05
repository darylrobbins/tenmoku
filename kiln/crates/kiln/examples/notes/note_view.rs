// SPDX-License-Identifier: Apache-2.0

//! A note's tab: a text field editing it.

use kiln::gpui::{prelude::*, *};
use kiln::{Item, View, ui::*, *};

use crate::notes::{Notes, NotesSettings};

#[kiln::item]
pub(crate) struct NoteView {
    id: u64,
    pub(crate) body: Entity<TextField>,
}

impl Item for NoteView {
    type State = u64;

    fn new(id: u64, window: &mut Window, cx: &mut App) -> Option<Self> {
        let text = model::read::<Notes>(cx).get(id)?.1.clone();
        let body = cx.new(|cx| TextField::new(window, cx).text(&text));
        cx.subscribe(&body, move |body, _: &TextFieldEvent, cx| {
            let text = body.read(cx).value().to_owned();
            model::update(cx, |notes: &mut Notes, _| notes.set_body(id, text));
        })
        .detach();
        Some(Self { id, body })
    }

    fn state(&self) -> u64 {
        self.id
    }

    fn title(&self, _: &App) -> SharedString {
        format!("Note {}", self.id).into()
    }

    fn focus(&self, cx: &App) -> FocusHandle {
        self.body.read(cx).focus_handle(cx)
    }
}

impl View for NoteView {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoElement {
        let size = cx.setting::<NotesSettings>().font_size;
        self.body
            .update(cx.cx, |body, cx| body.set_font_size(px(size), cx));
        div().size_full().child(self.body.clone())
    }
}
