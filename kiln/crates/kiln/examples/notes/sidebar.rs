// SPDX-License-Identifier: Apache-2.0

//! The list of notes, in the left dock.

use kiln::gpui::{prelude::*, *};
use kiln::{View, prelude::*, *};

use crate::{note_view::NoteView, notes::Notes};

#[kiln::panel(title = "Notes", icon = FileText, dock = Left, toggle_key = "secondary-b")]
#[derive(Default)]
struct Sidebar;

impl View for Sidebar {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoElement {
        let shown = cx.workspace().read(cx).active_state::<NoteView>(cx);
        let selected = cx.theme().colors.list.row_selected;
        let ids: Vec<_> = cx.watch::<Notes>().list.iter().map(|note| note.0).collect();
        div().p_2().children(ids.into_iter().map(|id| {
            let open = cx.handler(move |_, cx| {
                cx.workspace()
                    .update(cx.cx, |ws, wcx| ws.open::<NoteView>(id, cx.window, wcx));
            });
            div()
                .id(("note", id))
                .debug_selector(move || format!("note-row:{id}"))
                .when(shown == Some(id), |row| row.bg(selected))
                .child(format!("Note {id}"))
                .on_click(open)
        }))
    }
}
