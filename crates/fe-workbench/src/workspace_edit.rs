//! Applying a language server's workspace edit (a rename's, a code
//! action's, or one it pushes with `workspace/applyEdit`): fe-lsp's
//! [`FileEdits`], all or nothing as far as it can be.
//!
//! - **Checked first.** An open target's buffer must still be at the
//!   version the edits are for, shown in an editor, and at the file they're
//!   for, not saved under another name since. A closed target's
//!   file is resolved (symlinks and `..` followed, as the workbench resolves
//!   the files it opens on servers), and must not be named twice, nor have
//!   been opened since by any path to it: shown in an editor, being read
//!   into one (a host's read of the launch file included), or a pending
//!   Save As's destination, which that save would overwrite. It must read
//!   cleanly: it's read as the editor loads files
//!   ([`loader::read`], detecting its encoding), refused if any of it isn't
//!   valid in that encoding, its edits decoded ([`decode_closed`]) and
//!   applied there, and the result encoded, so an edit its encoding can't
//!   write is refused here too. Any failure refuses the whole edit,
//!   changing nothing, and names the file.
//! - **Then applied.** Each open buffer gets its edits as one transaction
//!   of the user, who asked for the rename or the action, as when
//!   accepting a completion, so undo in its editor takes it back;
//!   selections stay anchored. It's a transaction of its own: one the user
//!   has open (a Vim insert) is set aside meanwhile, neither joined nor
//!   closed. Then each closed file is written back in
//!   its own encoding, BOM and line endings, atomically, as saving does
//!   (and refusing a file changed since it was read). A write failure stops
//!   there, and the error names the files already written.
//! - **applyEdit.** [`LspEvent::ApplyEdit`](fe_lsp::LspEvent::ApplyEdit),
//!   handled with the other events, is applied on the main thread and
//!   answered applied, or failed with the error. Its reply is claimed
//!   first ([`fe_lsp::ApplyReply::claim`]), so fe-lsp's timeout can't
//!   answer it not applied while it's being applied; one that can't be
//!   claimed (it timed out, or the server's session ended) is dropped,
//!   changing nothing.

use std::{
    collections::HashSet,
    io,
    ops::Range,
    path::{Path, PathBuf},
};

use fe_buffer::{Origin, ProposedChange};
use fe_editor::EditorModel;
use fe_lsp::{EditTarget, FileEdits, decode_closed};
use gpui::{App, Context, Entity};

use crate::{Workbench, loader, lsp};

/// What [`Workbench::apply_file_edits`] changed, for the UI's message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AppliedEdits {
    /// The files changed, open or not.
    pub(crate) files: usize,
    /// The edits made in them.
    pub(crate) edits: usize,
}

/// One file's edits, checked and ready to apply.
enum Checked {
    /// To an open buffer, shown by `editor`, at its version now.
    Open {
        editor: Entity<fe_gpui::EditorView>,
        edits: Vec<(Range<usize>, String)>,
    },
    /// A closed file, resolved, read and edited: the model saves it.
    Closed {
        path: PathBuf,
        model: Box<EditorModel>,
    },
}

impl Workbench {
    /// Applies `edits`, a language server's workspace edit: see the
    /// [module](self). Every file is checked before any changes; the error
    /// names the file that refused it, or, if writing a closed file failed,
    /// that file and those already written.
    pub(crate) fn apply_file_edits(
        &mut self,
        edits: Vec<FileEdits>,
        cx: &mut Context<Self>,
    ) -> Result<AppliedEdits, String> {
        let mut applied = AppliedEdits { files: 0, edits: 0 };
        let opened = self.opened_files(cx);
        let mut targets = HashSet::new();
        let (mut open, mut closed) = (Vec::new(), Vec::new());
        for file in edits {
            applied.files += 1;
            applied.edits += match &file.target {
                EditTarget::Open { edits, .. } => edits.len(),
                EditTarget::Closed { edits, .. } => edits.len(),
                _ => 0,
            };
            match self.check_file_edits(file, &opened, &mut targets, cx)? {
                Checked::Open { editor, edits } => open.push((editor, edits)),
                Checked::Closed { path, model } => closed.push((path, model)),
            }
        }

        for (editor, edits) in open {
            let buffer = editor.update(cx, |view, cx| {
                let model = view.model_mut();
                model.sync_display();
                let mut buffer = model.buffer_mut();
                let mut change = ProposedChange::from_edits(buffer.current(), edits, Origin::User);
                change.accept_all(&mut buffer);
                drop(buffer);
                cx.notify();
                model.buffer().id()
            });
            // Its other editors show it on the next frame.
            for other in self.editors_of(buffer, cx) {
                other.update(cx, |_, cx| cx.notify());
            }
        }
        let mut written: Vec<PathBuf> = Vec::new();
        for (path, mut model) in closed {
            if let Err(error) = model.save() {
                let mut message = format!("can't write {}: {error}", path.display());
                if !written.is_empty() {
                    let names: Vec<_> = (written.iter())
                        .map(|path| path.display().to_string())
                        .collect();
                    message += &format!(" ({} already written)", names.join(", "));
                }
                return Err(message);
            }
            written.push(path);
        }
        cx.notify();
        Ok(applied)
    }

    /// `file` checked, and for a closed file read and edited, or why the
    /// whole edit is refused. `opened` is every file shown or being read,
    /// resolved; `targets`, the closed files checked so far.
    fn check_file_edits(
        &self,
        file: FileEdits,
        opened: &HashSet<PathBuf>,
        targets: &mut HashSet<PathBuf>,
        cx: &App,
    ) -> Result<Checked, String> {
        let display = file.path.display();
        match file.target {
            EditTarget::Open {
                buffer,
                version,
                edits,
            } => {
                let editor = (self.editors_of(buffer, cx).into_iter().next())
                    .ok_or_else(|| format!("{display} is no longer open"))?;
                let model = editor.read(cx).model();
                if model.buffer().version() != version {
                    return Err(format!("{display} has changed since"));
                }
                let now = model.path().and_then(lsp::resolve);
                if now.is_none() || now != lsp::resolve(&file.path) {
                    return Err(format!("{display} has been saved under another name since"));
                }
                Ok(Checked::Open {
                    editor,
                    edits: merge_inserts(edits),
                })
            }
            EditTarget::Closed { edits, encoding } => {
                let path =
                    lsp::resolve(&file.path).ok_or_else(|| format!("can't resolve {display}"))?;
                if opened.contains(&path) {
                    return Err(format!("{display} has been opened since"));
                }
                if !targets.insert(path.clone()) {
                    return Err(format!("{display} is named twice, by another path"));
                }
                let buffer = match loader::read(path.clone()) {
                    Ok(Some(buffer)) => buffer,
                    Ok(None) => return Err(format!("{display} doesn't exist")),
                    Err(error) => return Err(format!("can't read {display}: {error}")),
                };
                let label = buffer.encoding().label();
                // Unedited, it can be written back only if it read cleanly.
                if let Err(error) = buffer.save_snapshot().write_to(io::sink()) {
                    return Err(format!("{display} isn't valid {label}: {error}"));
                }
                let edits = decode_closed(buffer.text(), &edits, encoding)
                    .map_err(|why| format!("{display}: {why}"))?;
                let mut model = EditorModel::new(buffer, Some(path.clone()));
                model.buffer_mut().edit(edits, Origin::User);
                // What the save will write, so one that would fail does
                // before anything is changed.
                let encoded = model.buffer().save_snapshot().write_to(io::sink());
                if let Err(error) = encoded {
                    return Err(format!("{display} can't be written as {label}: {error}"));
                }
                Ok(Checked::Closed {
                    path,
                    model: Box::new(model),
                })
            }
            _ => Err(format!("{display}: an edit fastedit doesn't know")),
        }
    }

    /// Every file an editor shows, a pane is reading into one, or a Save As
    /// is to write ([`crate::open::reserve`]), resolved, with the paths they
    /// go by.
    fn opened_files(&self, cx: &App) -> HashSet<PathBuf> {
        let shown = (self.open_editors(cx).iter())
            .filter_map(|editor| editor.read(cx).model().path().map(Path::to_owned))
            .collect::<Vec<_>>();
        let reading = (self.pane_group().panes().iter())
            .flat_map(|pane| {
                pane.read(cx)
                    .reading()
                    .map(Path::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let launch = self.launch_reading().map(Path::to_owned);
        let saving = crate::open::reserved_paths();
        (shown.into_iter().chain(reading).chain(launch).chain(saving))
            .flat_map(|path| [lsp::resolve(&path), Some(path)])
            .flatten()
            .collect()
    }
}

/// `edits`, sorted and apart, with inserts at one place joined in their
/// order, as one replacement there would make them: a
/// [`ProposedChange`] takes no two at one place.
fn merge_inserts(edits: Vec<(Range<usize>, String)>) -> Vec<(Range<usize>, String)> {
    let mut merged: Vec<(Range<usize>, String)> = Vec::with_capacity(edits.len());
    for (range, text) in edits {
        match merged.last_mut() {
            Some((last, last_text)) if last.start == last.end && range == last.clone() => {
                last_text.push_str(&text);
            }
            _ => merged.push((range, text)),
        }
    }
    merged
}

#[cfg(test)]
#[path = "workspace_edit_tests.rs"]
mod tests;
