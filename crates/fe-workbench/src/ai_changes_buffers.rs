//! The [`BufferSet`] a [`fe_editor::Changeset`] reaches the workbench's
//! buffers through: each open editor's shared buffer by its [`BufferId`],
//! and the buffers made for the files a turn creates.
//!
//! An editor's buffer is lent as its [`BufferMut`] guard, whose drop catches
//! the editor's model up, so the guards of every distinct buffer are held at
//! once, nested one editor update per buffer ([`with_editors`]). Splits of
//! one buffer are lent once.

use std::path::PathBuf;

use fe_buffer::{Buffer, BufferId, Origin};
use fe_editor::{BufferMut, BufferSet};
use fe_gpui::EditorView;
use gpui::{App, Entity};

/// Buffers made for new files, not yet in an editor, and the buffers the
/// changeset asked to remove.
#[derive(Default)]
pub(crate) struct Made {
    /// Each new file's path and buffer, until the workbench opens it.
    pub(crate) buffers: Vec<(PathBuf, Buffer)>,
    /// Buffers the changeset removed that an editor may still show.
    pub(crate) removed: Vec<BufferId>,
}

/// The open editors' buffers, one nested guard per editor, and at the root
/// the buffers in [`Made`].
pub(crate) struct Editors<'a, 'b> {
    slot: Option<(BufferId, BufferMut<'a>)>,
    rest: Option<&'b mut (dyn BufferSet + 'b)>,
    made: Option<&'b mut Made>,
}

impl BufferSet for Editors<'_, '_> {
    fn buffer_mut(&mut self, id: BufferId) -> Option<&mut Buffer> {
        if self.slot.as_ref().is_some_and(|(held, _)| *held == id) {
            return self.slot.as_mut().map(|(_, guard)| &mut **guard);
        }
        if let Some(rest) = self.rest.as_deref_mut() {
            return rest.buffer_mut(id);
        }
        let made = self.made.as_deref_mut()?;
        (made.buffers.iter_mut())
            .find(|(_, buffer)| buffer.id() == id)
            .map(|(_, buffer)| buffer)
    }

    fn create(&mut self, path: &std::path::Path, text: &str) -> Option<BufferId> {
        if let Some(rest) = self.rest.as_deref_mut() {
            return rest.create(path, text);
        }
        // Typed into an empty buffer and marked unsaved, so it's dirty even
        // when empty: closing it asks, and saving makes the file.
        let mut buffer = Buffer::new("");
        buffer.edit([(0..0, text)], Origin::User);
        buffer.mark_unsaved();
        let id = buffer.id();
        (self.made.as_deref_mut()?)
            .buffers
            .push((path.to_owned(), buffer));
        Some(id)
    }

    fn remove(&mut self, id: BufferId) -> bool {
        if let Some(rest) = self.rest.as_deref_mut() {
            return rest.remove(id);
        }
        let Some(made) = self.made.as_deref_mut() else {
            return false;
        };
        match made
            .buffers
            .iter()
            .position(|(_, buffer)| buffer.id() == id)
        {
            Some(at) => {
                made.buffers.remove(at);
            }
            None => made.removed.push(id),
        }
        true
    }
}

/// Runs `f` over the buffers of `editors` (one guard per distinct buffer)
/// and `made`. No editor may be being updated.
pub(crate) fn with_editors<R>(
    editors: &[Entity<EditorView>],
    made: &mut Made,
    cx: &mut App,
    f: impl FnOnce(&mut Editors<'_, '_>) -> R,
) -> R {
    let mut distinct: Vec<(BufferId, Entity<EditorView>)> = Vec::new();
    for editor in editors {
        let id = editor.read(cx).model().buffer().id();
        if distinct.iter().all(|(held, _)| *held != id) {
            distinct.push((id, editor.clone()));
        }
    }
    let editors: Vec<Entity<EditorView>> = distinct.into_iter().map(|(_, editor)| editor).collect();
    let mut root = Editors {
        slot: None,
        rest: None,
        made: Some(made),
    };
    descend(&editors, &mut root, cx, f)
}

fn descend<R>(
    editors: &[Entity<EditorView>],
    parent: &mut Editors<'_, '_>,
    cx: &mut App,
    f: impl FnOnce(&mut Editors<'_, '_>) -> R,
) -> R {
    let Some((first, rest)) = editors.split_first() else {
        return f(parent);
    };
    first.update(cx, |view, cx| {
        let guard = view.model_mut().buffer_mut();
        let id = guard.id();
        let mut node = Editors {
            slot: Some((id, guard)),
            rest: Some(parent),
            made: None,
        };
        descend(rest, &mut node, cx, f)
    })
}
