// SPDX-License-Identifier: LicenseRef-Proprietary

//! The notes, their count and the command that adds one.

use kiln::{gpui::App, prelude::*, serde::*, shell::Workspace, *};

use crate::note_view::NoteView;

pub(crate) type Note = (u64, String);

/// Every note, kept in the state directory.
#[kiln::model(persist)]
#[derive(Default, Serialize, Deserialize)]
#[serde(crate = "kiln::serde")]
pub(crate) struct Notes {
    pub(crate) list: Vec<Note>,
    next_id: u64,
}

impl Notes {
    fn add(&mut self) -> u64 {
        self.next_id += 1;
        self.list.push((self.next_id, String::new()));
        self.next_id
    }

    pub(crate) fn get(&self, id: u64) -> Option<&Note> {
        self.list.iter().find(|note| note.0 == id)
    }

    pub(crate) fn set_body(&mut self, id: u64, body: String) {
        let note = self.list.iter_mut().find(|note| note.0 == id);
        note.into_iter().for_each(|note| note.1 = body.clone());
    }
}

/// `[notes]` in settings.toml.
#[kiln::settings(section = "notes")]
pub(crate) struct NotesSettings {
    /// The note's font size, in points.
    #[setting(default = 14.0)]
    pub(crate) font_size: f32,
}

/// New Note
#[kiln::command(key = "secondary-n", menu = "File")]
fn new_note(workspace: &mut Workspace, cx: &mut Cx<Workspace>) {
    let id = cx.update_model(|notes: &mut Notes, _| notes.add());
    workspace.open::<NoteView>(id, cx.window, cx.cx);
}

#[kiln::status(side = Left)]
fn note_count(cx: &App) -> String {
    let count = model::entity_if::<Notes>(cx).map_or(0, |notes| notes.read(cx).list.len());
    format!("{count} note{}", if count == 1 { "" } else { "s" })
}
