//! Ghost text: an inline suggestion shown at the caret, dimmed, that isn't
//! part of the buffer. GPUI-free; the view holds it, paints
//! [`remaining`](GhostText::remaining) and drops it to dismiss it.
//!
//! - **Anchored** before the offset it was offered at, so edits elsewhere,
//!   from anyone, move it with the text, and typing there goes after it.
//! - **Typing consumes it**: while the text from the anchor to the caret is
//!   a prefix of the suggestion, the rest still shows. Deleting back toward
//!   the anchor shows more again; a char that doesn't match, a caret moved
//!   away, a second caret or a selection ends it.
//! - **Accepting** inserts the rest, or its next word or line, at the caret
//!   as one [`Origin::User`] transaction (docs/decisions/command-model.md,
//!   "Accepting"): the user's own edit, which one undo takes back, joining
//!   a transaction already open, as vim's insert session holds, as typing
//!   does. The suggesting [`Origin`] is kept only as who proposed it.

use std::sync::Arc;

use fe_buffer::{Anchor, BufferId, BufferSnapshot, Origin, TransactionId, Version};

use crate::{CharClass, EditorModel};

/// A suggestion offered at an offset of a buffer.
#[derive(Clone, Debug)]
pub struct GhostText {
    buffer: BufferId,
    anchor: Anchor,
    text: Arc<str>,
    origin: Origin,
}

impl GhostText {
    /// `text`, any number of lines, suggested by `origin` at `offset` of
    /// `snapshot`.
    pub fn new(
        snapshot: &BufferSnapshot,
        offset: usize,
        text: impl Into<Arc<str>>,
        origin: Origin,
    ) -> Self {
        Self {
            buffer: snapshot.id(),
            anchor: snapshot.anchor_before(offset),
            text: text.into(),
            origin,
        }
    }

    /// The whole suggestion, as offered.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Who suggested it.
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The version its anchor is in. The buffer's owner must keep
    /// [`Buffer::compact_log`](fe_buffer::Buffer::compact_log)'s floor at
    /// or below it, or the suggestion ends.
    pub fn version(&self) -> Version {
        self.anchor.version()
    }

    /// Rebases the anchor onto `snapshot`, so resolving it stays cheap and
    /// the log can be compacted up to `snapshot`'s version. Nothing changes
    /// if `snapshot` is of another buffer, or can't map the anchor.
    pub fn refresh(&mut self, snapshot: &BufferSnapshot) {
        if self.maps_into(snapshot) {
            self.anchor = snapshot.refresh(&self.anchor);
        }
    }

    /// The part of the suggestion still to show at `model`'s caret, or
    /// `None` if it's over: see the module docs.
    pub fn remaining(&self, model: &EditorModel) -> Option<&str> {
        let current = model.snapshot();
        if !self.maps_into(current) {
            return None;
        }
        let selections = model.selections();
        let [selection] = &selections[..] else {
            return None;
        };
        if !selection.range.is_empty() {
            return None;
        }
        let (anchor, caret) = (current.resolve(&self.anchor), selection.range.start);
        let typed = caret.checked_sub(anchor)?;
        let live = typed < self.text.len()
            && self.text.is_char_boundary(typed)
            && current.text().range_eq(anchor..caret, &self.text[..typed]);
        live.then(|| &self.text[typed..])
    }

    /// Whether the suggestion still shows at `model`'s caret.
    pub fn is_live(&self, model: &EditorModel) -> bool {
        self.remaining(model).is_some()
    }

    /// Inserts all of [`remaining`](Self::remaining) at the caret, as the
    /// user. `None`, changing nothing, if the suggestion is over.
    pub fn accept(&self, model: &mut EditorModel) -> Option<TransactionId> {
        self.accept_with(model, str::len)
    }

    /// Inserts the next word of what remains: the non-word chars before it,
    /// then its word chars ([`CharClass`]), stopping before a line break
    /// unless what remains starts with one. The rest stays live.
    pub fn accept_word(&self, model: &mut EditorModel) -> Option<TransactionId> {
        self.accept_with(model, |rest| {
            let skip = usize::from(rest.starts_with('\n'));
            let mut chars = rest[skip..].char_indices().peekable();
            let mut word = false;
            while let Some(&(_, c)) = chars.peek() {
                let class = CharClass::of(c);
                if c == '\n' || (word && class != CharClass::Word) {
                    break;
                }
                word |= class == CharClass::Word;
                chars.next();
            }
            skip + chars.peek().map_or(rest.len() - skip, |&(ix, _)| ix)
        })
    }

    /// Inserts what remains up to and including the next line break, or all
    /// of it. The rest stays live.
    pub fn accept_line(&self, model: &mut EditorModel) -> Option<TransactionId> {
        self.accept_with(model, |rest| {
            rest.find('\n').map_or(rest.len(), |ix| ix + 1)
        })
    }

    /// Inserts the first `len(remaining)` bytes of what remains at the
    /// caret, as the user, as typing does.
    fn accept_with(
        &self,
        model: &mut EditorModel,
        len: impl FnOnce(&str) -> usize,
    ) -> Option<TransactionId> {
        model.sync_display();
        let rest = self.remaining(model)?;
        let piece = rest[..len(rest)].to_owned();
        let (_, produced) = model.run_as(Origin::User, |model| model.insert(&piece));
        let id = produced.first().copied();
        // A command accepting it produced this transaction too.
        if let (Some(id), Some(transactions)) = (id, &mut model.transactions) {
            transactions.push(id);
        }
        id
    }

    /// Whether the anchor resolves in `snapshot`: the same buffer, and a
    /// version the log still maps from, not newer than `snapshot`.
    fn maps_into(&self, snapshot: &BufferSnapshot) -> bool {
        let version = self.anchor.version();
        snapshot.id() == self.buffer
            && snapshot.oldest_version() <= version
            && version <= snapshot.version()
    }
}
