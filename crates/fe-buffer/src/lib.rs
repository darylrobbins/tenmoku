//! Editable buffers for fastedit.
//!
//! M1 fills this in: transactions tagged with an [`Origin`], an undo history
//! scoped by origin, anchors that survive concurrent edits, and change events.
//! It must never depend on GPUI.

use fe_text::Text;

/// Who made an edit. Undo is grouped by origin so the AI's edits can be
/// reverted without touching the user's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    User,
    Ai { agent: String, session: String },
    Lsp,
    Format,
    Host(String),
}

pub struct Buffer {
    text: Text,
}

impl Buffer {
    pub fn new(text: &str) -> Self {
        Self {
            text: Text::new(text),
        }
    }

    /// A cheap, immutable view of the current text.
    pub fn snapshot(&self) -> Text {
        self.text.clone()
    }
}
