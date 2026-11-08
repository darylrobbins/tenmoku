//! Applying a language server's workspace edit (a rename's, a code
//! action's, or one it pushes with `workspace/applyEdit`): fe-lsp's
//! [`FileEdits`], all or nothing as far as it can be for text edits, and
//! in order with the file operations it makes.
//!
//! - **Checked first.** An open target's buffer must still be at the
//!   version the edits are for, shown in an editor, and at the file they're
//!   for, not saved under another name since. A closed target's
//!   file is resolved (symlinks and `..` followed, as the workbench resolves
//!   the files it opens on servers), and must not be named twice, nor have
//!   been opened since by any path to it, in any window: shown in an
//!   editor, being read
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
//!   selections stay anchored. Edits to it in later runs (past a file
//!   operation) join that transaction, so one undo takes the whole edit
//!   back. It's a transaction of its own: one the user
//!   has open (a Vim insert) is set aside meanwhile, neither joined nor
//!   closed. Then each closed file is written back in
//!   its own encoding, BOM and line endings, atomically, as saving does
//!   (and refusing a file changed since it was read). A write failure stops
//!   there, and the error names the files already written.
//! - **In order.** Text edits and file operations (create, rename,
//!   delete) apply in the edit's order. Each run of text edits between
//!   operations is checked as above, then applied: a buffer an earlier run
//!   edited is still at the server's version for this edit's purposes,
//!   its edits moved past those as they were made (refused if they overlap
//!   them), so long as nothing else changed it since. An open buffer's
//!   editor is found in any window. Edits that depend on whether an
//!   operation `ignoreIfExists` may skip was made
//!   ([`EditTarget::Depends`]) take the way it went. The first entry that
//!   fails stops the rest, and what was applied before it stays applied,
//!   as LSP's `abort` failure handling has it. An operation is made as the
//!   project tree makes it, through `fe_project`'s `fs_ops` under the
//!   project's (canonical) root, refused outside it, and while
//!   [`Workbench::busy_under`] the path, as the tree's rename is. Servers
//!   are told, as the tree tells them (`didCreateFiles` and the rest).
//!   - **Create** makes an empty file, with the folders above it. An
//!     existing one is refused, unless `overwrite` or `ignoreIfExists`
//!     (skipped). A file it overwrites is emptied in place, keeping its
//!     owner, permissions, ACLs and extended attributes, and servers aren't
//!     told it was created; anything else there, as a symlink, is moved to
//!     the Trash and a file made in its place. One shown in
//!     any window's editor, being read into one or held by a Save As is
//!     refused.
//!   - **Rename** renames a file or folder within its folder (`fs_ops`
//!     moves nothing to another one: refused), editors of it and of the
//!     files in it following, in every window, as the tree's rename has
//!     them. An existing target is refused unless `overwrite` (it's moved
//!     to the Trash first) or `ignoreIfExists` (skipped); one any window
//!     shows, there or not, is refused. fe-lsp decodes text edits after it
//!     against the renamed file's document, so its buffer, following, gets
//!     them, checked as any open file's; a closed edit to a file an editor
//!     now shows is refused, as it's against text the server doesn't have.
//!   - **Delete** moves the file or folder to the Trash; a missing one is
//!     refused unless `ignoreIfNotExists`, and a folder unless
//!     `recursive`. Editors of it stay open with their buffers, as the
//!     tree's leave them.
//! - **applyEdit.** [`LspEvent::ApplyEdit`](fe_lsp::LspEvent::ApplyEdit),
//!   handled with the other events, is applied on the main thread and
//!   answered applied, or failed with the error. Its reply is claimed
//!   first ([`fe_lsp::ApplyReply::claim`]), so fe-lsp's timeout can't
//!   answer it not applied while it's being applied; one that can't be
//!   claimed (it timed out, or the server's session ended) is dropped,
//!   changing nothing.

use std::{
    collections::{HashMap, HashSet},
    fs, io,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_buffer::{BufferId, Origin, ProposedChange, TransactionId, Version};
use fe_editor::EditorModel;
use fe_lsp::{EditTarget, FileEdits, OperationId, decode_closed};
use fe_project::fs_ops;
use gpui::{App, Context, Entity};

use crate::{
    Workbench, loader, lsp,
    workbench::{Busy, resolved},
};

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
    /// To an open buffer, shown by `editor`, at its version now; `server`
    /// is the version the server's edits were for.
    Open {
        editor: Entity<fe_gpui::EditorView>,
        server: Version,
        edits: Vec<(Range<usize>, String)>,
    },
    /// A closed file, resolved, read and edited: the model saves it.
    Closed {
        path: PathBuf,
        model: Box<EditorModel>,
    },
}

/// A buffer this edit changed in an earlier run of text edits.
struct Edited {
    /// The version the server's edits to it are for.
    server: Version,
    /// Its version once this edit last changed it.
    now: Version,
    /// The edits made to it, run by run, each in the text it was made to:
    /// the range replaced, and the length of what replaced it.
    runs: Vec<Vec<(Range<usize>, usize)>>,
    /// The transaction its first run made, which later runs join.
    transaction: Option<TransactionId>,
}

/// What [`Workbench::apply_operation`] did.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Made {
    /// Made it.
    Done,
    /// Skipped it, as its `ignoreIfExists` or `ignoreIfNotExists` says.
    Skipped,
    /// Nothing to do: a rename to the same path.
    Nothing,
}

impl Workbench {
    /// Applies `edits`, a language server's workspace edit, in order: see
    /// the [module](self). Each run of text edits is checked before any of
    /// it changes; the error names the entry that refused the rest, or, if
    /// writing a closed file failed, that file and those already written.
    pub(crate) fn apply_file_edits(
        &mut self,
        edits: Vec<FileEdits>,
        cx: &mut Context<Self>,
    ) -> Result<AppliedEdits, String> {
        self.apply_file_edits_as(edits, false, cx)
    }

    /// [`Workbench::apply_file_edits`] for an edit the user asked for (a
    /// rename, a code action picked), which notes where it ended in each
    /// open buffer as the user's last edit there.
    pub(crate) fn apply_user_file_edits(
        &mut self,
        edits: Vec<FileEdits>,
        cx: &mut Context<Self>,
    ) -> Result<AppliedEdits, String> {
        self.apply_file_edits_as(edits, true, cx)
    }

    fn apply_file_edits_as(
        &mut self,
        edits: Vec<FileEdits>,
        by_user: bool,
        cx: &mut Context<Self>,
    ) -> Result<AppliedEdits, String> {
        let mut applied = AppliedEdits { files: 0, edits: 0 };
        let mut edited = HashMap::new();
        let others = Self::other_workbenches(cx);
        // The operations made so far, and whether each was skipped.
        let mut done: Vec<(Option<OperationId>, bool)> = Vec::new();
        let mut entries = edits.into_iter().peekable();
        while let Some(first) = entries.next() {
            if !is_text(&first) {
                let operation = first.operation;
                let made = self.apply_operation(first, &others, cx)?;
                if made == Made::Done {
                    applied.files += 1;
                }
                done.push((operation, made == Made::Skipped));
                continue;
            }
            let mut run = vec![first];
            run.extend(std::iter::from_fn(|| entries.next_if(is_text)));
            let run = (run.into_iter())
                .map(|file| Ok((file.path, resolve_depends(file.target, &done)?)))
                .collect::<Result<_, String>>()?;
            self.apply_text_edits(run, by_user, &others, &mut edited, &mut applied, cx)?;
        }
        cx.notify();
        Ok(applied)
    }

    /// Applies `run`, text edits between two operations, once each is
    /// checked, counting them in `applied`; `edited` has the buffers
    /// earlier runs changed, and gets this one's. With `by_user`, notes
    /// where the edits ended in each open buffer as the user's last edit.
    fn apply_text_edits(
        &mut self,
        run: Vec<(PathBuf, EditTarget)>,
        by_user: bool,
        others: &[Entity<Workbench>],
        edited: &mut HashMap<BufferId, Edited>,
        applied: &mut AppliedEdits,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        // Any window's: a closed file's edits are written under them.
        let opened: HashSet<PathBuf> = (self.and_others(others, cx))
            .flat_map(|workbench| workbench.opened_files(cx))
            .collect();
        let mut targets = HashSet::new();
        let (mut open, mut closed) = (Vec::new(), Vec::new());
        for (path, target) in run {
            applied.files += 1;
            applied.edits += match &target {
                EditTarget::Open { edits, .. } => edits.len(),
                EditTarget::Closed { edits, .. } => edits.len(),
                _ => 0,
            };
            match self.check_file_edits(&path, target, &opened, &mut targets, others, edited, cx)? {
                Checked::Open {
                    editor,
                    server,
                    edits,
                } => open.push((editor, server, edits)),
                Checked::Closed { path, model } => closed.push((path, model)),
            }
        }

        for (editor, server, edits) in open {
            let id = editor.read(cx).model().buffer().id();
            let earlier = edited.get(&id).and_then(|edited| edited.transaction);
            let (buffer, now, made, transaction) = editor.update(cx, |view, cx| {
                let model = view.model_mut();
                model.sync_display();
                let mut buffer = model.buffer_mut();
                let version = buffer.version();
                let base = buffer.current();
                let mut change = ProposedChange::from_edits(base, edits, Origin::User);
                // As made: clipped, line endings normalized, and no-ops
                // left out.
                let hunks = change.hunks(base);
                let made: Vec<_> = (hunks.iter())
                    .map(|hunk| (hunk.range.clone(), hunk.new_text.len()))
                    .collect();
                // A later run joins the first's transaction: one undo step
                // takes the whole edit back from the buffer.
                let joined = earlier.and_then(|transaction| {
                    let edits = (hunks.into_iter()).map(|hunk| (hunk.range, hunk.new_text));
                    let joined = buffer.edit_in(transaction, edits).ok()?;
                    Some(joined.unwrap_or(transaction))
                });
                let transaction = joined.or_else(|| change.accept_all(&mut buffer).0);
                if by_user {
                    buffer.note_edits_since(version);
                }
                drop(buffer);
                cx.notify();
                let model = view.model();
                (
                    model.buffer().id(),
                    model.buffer().version(),
                    made,
                    transaction,
                )
            });
            let entry = edited.entry(buffer).or_insert(Edited {
                server,
                now,
                runs: Vec::new(),
                transaction: None,
            });
            entry.now = now;
            entry.runs.push(made);
            entry.transaction = entry.transaction.or(transaction);
            // Its other editors, in any window, show it on the next frame.
            let editors: Vec<_> = (self.and_others(others, cx))
                .flat_map(|workbench| workbench.editors_of(buffer, cx))
                .collect();
            for other in editors {
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
        Ok(())
    }

    /// Makes `file`'s operation, a create, rename or delete: what it did,
    /// or why it's refused.
    fn apply_operation(
        &mut self,
        file: FileEdits,
        others: &[Entity<Workbench>],
        cx: &mut Context<Self>,
    ) -> Result<Made, String> {
        let display = file.path.display().to_string();
        let root = (self.root().and_then(|root| dunce::canonicalize(root).ok()))
            .ok_or_else(|| format!("{display} isn't in the project"))?;
        // Resolved as the tree's operations take them: its folders, never
        // the entry itself, which may be a symlink.
        let in_project = |path: &Path| {
            let full = resolved(path);
            match full.strip_prefix(&root) {
                Ok(rel) if !rel.as_os_str().is_empty() => Ok(full),
                _ => Err(format!("{} isn't in the project", path.display())),
            }
        };
        let missing = |error: &io::Error| error.kind() == io::ErrorKind::NotFound;
        match file.target {
            EditTarget::Create {
                overwrite,
                ignore_if_exists,
            } => {
                let path = in_project(&file.path)?;
                let existing = match fs::symlink_metadata(&path) {
                    Ok(meta) => Some(meta),
                    Err(error) if missing(&error) => None,
                    Err(error) => return Err(format!("can't create {display}: {error}")),
                };
                match &existing {
                    Some(_) if !overwrite && ignore_if_exists => return Ok(Made::Skipped),
                    Some(_) if !overwrite => return Err(format!("{display} already exists")),
                    Some(meta) if meta.is_dir() => return Err(format!("{display} is a folder")),
                    _ => {}
                }
                if self.shows_under(&path, others, cx) {
                    return Err(format!("{display} is open"));
                }
                self.check_idle(&file.path, others, cx)?;
                match existing {
                    // Emptied in place, so it keeps its owner, permissions,
                    // ACLs and extended attributes, through a handle found
                    // as the tree finds files. It isn't new, so servers
                    // aren't told it was created.
                    Some(meta) if meta.is_file() => {
                        fs_ops::open_truncated(&root, &path)
                            .map_err(|error| format!("can't overwrite {display}: {error}"))?;
                    }
                    // Anything else, as a symlink, which could lead out of
                    // the project, is moved to the Trash, and a file made in
                    // its place through the tree's operations.
                    existing => {
                        let name = (path.strip_prefix(&root).ok())
                            .and_then(Path::to_str)
                            .ok_or_else(|| {
                                format!("can't create {display}: its name isn't UTF-8")
                            })?;
                        if existing.is_some() {
                            fs_ops::trash(&root, &[&path]).map_err(|error| {
                                format!("can't move {display} to the Trash: {error}")
                            })?;
                        }
                        fs_ops::create_file(&root, &root, name)
                            .map_err(|error| format!("can't create {display}: {error}"))?;
                        self.lsp_files_created(&[(file.path.clone(), false)]);
                    }
                }
            }
            EditTarget::Rename {
                to,
                overwrite,
                ignore_if_exists,
            } => {
                let (from, target) = (in_project(&file.path)?, in_project(&to)?);
                let to_display = to.display();
                let identity = |path: &Path| kiln::core::platform::path_identity(path, false);
                let source = identity(&from).map_err(|error| match missing(&error) {
                    true => format!("{display} doesn't exist"),
                    false => format!("can't rename {display}: {error}"),
                })?;
                if from == target {
                    return Ok(Made::Nothing);
                }
                let name = (target.parent() == from.parent())
                    .then(|| target.file_name()?.to_str())
                    .flatten()
                    .ok_or_else(|| {
                        format!("Tenmoku can't move {display} to another folder ({to_display})")
                    })?;
                let existing = match fs::symlink_metadata(&target) {
                    Ok(meta) => Some(meta),
                    Err(error) if missing(&error) => None,
                    Err(error) => return Err(format!("can't rename {display}: {error}")),
                };
                // The entry itself, under another case on a volume that
                // ignores case: `fs_ops::rename` renames it.
                let itself = existing.is_some() && identity(&target).ok() == Some(source);
                let replaced = match existing {
                    Some(_) if itself => None,
                    Some(_) if !overwrite && ignore_if_exists => return Ok(Made::Skipped),
                    Some(_) if !overwrite => return Err(format!("{to_display} already exists")),
                    other => other,
                };
                // There or not, an editor of it would share its path with
                // the editors that follow.
                if !itself && self.shows_under(&target, others, cx) {
                    return Err(format!("{to_display} is open"));
                }
                self.check_idle(&file.path, others, cx)?;
                self.check_idle(&to, others, cx)?;
                if let Some(existing) = replaced {
                    fs_ops::trash(&root, &[&target]).map_err(|error| {
                        format!("can't move {to_display} to the Trash: {error}")
                    })?;
                    self.lsp_files_deleted(&[(to.clone(), existing.is_dir())]);
                }
                // Planned while every spelling of the old path still
                // resolves.
                let moves = self.plan_rename_here(&file.path, &to, others, cx);
                fs_ops::rename(&root, &from, name)
                    .map_err(|error| format!("can't rename {display}: {error}"))?;
                self.lsp_files_renamed(&file.path, &to);
                self.follow_moves_everywhere(&moves, others, cx);
            }
            EditTarget::Delete {
                recursive,
                ignore_if_not_exists,
            } => {
                let path = in_project(&file.path)?;
                let folder = match fs::symlink_metadata(&path) {
                    Ok(meta) => meta.is_dir(),
                    Err(error) if missing(&error) && ignore_if_not_exists => {
                        return Ok(Made::Skipped);
                    }
                    Err(error) if missing(&error) => {
                        return Err(format!("{display} doesn't exist"));
                    }
                    Err(error) => return Err(format!("can't delete {display}: {error}")),
                };
                if folder && !recursive {
                    return Err(format!(
                        "{display} is a folder, and the edit doesn't delete what's in it"
                    ));
                }
                self.check_idle(&file.path, others, cx)?;
                fs_ops::trash(&root, &[&path])
                    .map_err(|error| format!("can't move {display} to the Trash: {error}"))?;
                self.lsp_files_deleted(&[(file.path.clone(), folder)]);
            }
            _ => return Err(format!("{display}: an edit Tenmoku doesn't know")),
        }
        Ok(Made::Done)
    }

    /// Why an operation on `path` must wait, as the tree's rename waits
    /// ([`Workbench::busy_under`]), if it must.
    fn check_idle(
        &self,
        path: &Path,
        others: &[Entity<Workbench>],
        cx: &App,
    ) -> Result<(), String> {
        let display = path.display();
        match self.busy_under_here(path, others, cx) {
            None => Ok(()),
            Some(Busy::Saving) => Err(format!("{display} is being saved")),
            Some(Busy::Opening) => Err(format!("{display} is being opened")),
            Some(Busy::Working) => Err(format!("{display} is being worked on")),
        }
    }

    /// Whether, in this window or any other, an editor shows `path`
    /// (resolved) or a file under it, a pane is reading one into a tab, or
    /// a Save As is to write one.
    fn shows_under(&self, path: &Path, others: &[Entity<Workbench>], cx: &App) -> bool {
        (self.and_others(others, cx)).any(|workbench| {
            (workbench.opened_files(cx).iter()).any(|opened| opened.starts_with(path))
        })
    }

    /// `target`, `path`'s edits, checked, and for a closed file read and
    /// edited, or why the whole edit is refused. `opened` is every file shown or being read,
    /// resolved; `targets`, the closed files checked so far; `edited`, the
    /// buffers earlier runs of the edit changed.
    #[allow(clippy::too_many_arguments)]
    fn check_file_edits(
        &self,
        path: &Path,
        target: EditTarget,
        opened: &HashSet<PathBuf>,
        targets: &mut HashSet<PathBuf>,
        others: &[Entity<Workbench>],
        edited: &HashMap<BufferId, Edited>,
        cx: &App,
    ) -> Result<Checked, String> {
        let display = path.display();
        match target {
            EditTarget::Open {
                buffer,
                version,
                edits,
            } => {
                // In any window: the server's document may be shown only
                // in another.
                let editor = (self.and_others(others, cx))
                    .find_map(|workbench| workbench.editors_of(buffer, cx).into_iter().next())
                    .ok_or_else(|| format!("{display} is no longer open"))?;
                let model = editor.read(cx).model();
                let current = model.buffer().version();
                let edits = match edited.get(&buffer) {
                    _ if current == version => edits,
                    // Changed since only by this edit: its edits moved
                    // past those.
                    Some(earlier) if earlier.server == version && earlier.now == current => {
                        rebase(edits, &earlier.runs).ok_or_else(|| {
                            format!("{display}: its edits overlap the edit's earlier ones")
                        })?
                    }
                    _ => return Err(format!("{display} has changed since")),
                };
                let now = model.path().and_then(lsp::resolve);
                if now.is_none() || now != lsp::resolve(path) {
                    return Err(format!("{display} has been saved under another name since"));
                }
                Ok(Checked::Open {
                    editor,
                    server: version,
                    edits: merge_inserts(edits),
                })
            }
            EditTarget::Closed { edits, encoding } => {
                let path = lsp::resolve(path).ok_or_else(|| format!("can't resolve {display}"))?;
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
            // Operations are made by `apply_operation`.
            _ => Err(format!("{display}: an edit Tenmoku doesn't know")),
        }
    }

    /// Every file an editor shows, a pane is reading into one, or a Save As
    /// is to write ([`crate::open::reserve`]), resolved, with the paths they
    /// go by.
    pub(crate) fn opened_files(&self, cx: &App) -> HashSet<PathBuf> {
        let shown = (self.open_editors(cx).iter())
            .filter_map(|editor| editor.read(cx).model().path().map(Path::to_owned))
            .collect::<Vec<_>>();
        let reading = (self.pane_group(cx).panes().iter())
            .flat_map(|pane| {
                pane.read(cx)
                    .reading()
                    .map(Path::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let launch = self.launch_reading(cx).map(Path::to_owned);
        let saving = crate::open::reserved_paths();
        (shown.into_iter().chain(reading).chain(launch).chain(saving))
            .flat_map(|path| [lsp::resolve(&path), Some(path)])
            .flatten()
            .collect()
    }
}

/// Whether `file` is text edits, not a file operation.
fn is_text(file: &FileEdits) -> bool {
    matches!(
        file.target,
        EditTarget::Open { .. } | EditTarget::Closed { .. } | EditTarget::Depends { .. }
    )
}

/// `target` with each [`EditTarget::Depends`] taken the way its operation
/// went: the one in `done` with its identity, made or skipped. Why the edits
/// can't apply that way, or if it wasn't made, refuses them. A walk down
/// one branch at a time, as deep as the operations it depends on.
fn resolve_depends(
    mut target: EditTarget,
    done: &[(Option<OperationId>, bool)],
) -> Result<EditTarget, String> {
    loop {
        let EditTarget::Depends { on, made, skipped } = target else {
            return Ok(target);
        };
        let was_skipped = (done.iter().rev())
            .find(|(operation, _)| *operation == Some(on))
            .map(|(_, skipped)| *skipped)
            .ok_or("a file the edit depends on isn't one of its operations")?;
        let branch = if was_skipped { skipped } else { made };
        target = Arc::unwrap_or_clone(branch)?;
    }
}

/// `edits`, byte edits of a buffer's text before the edit's earlier `runs`
/// changed it, moved to its text after them: `None` if one overlaps an
/// edit made, or meets an insert from inside it. An insert where one was
/// made goes after it, as the later edit.
fn rebase(
    mut edits: Vec<(Range<usize>, String)>,
    runs: &[Vec<(Range<usize>, usize)>],
) -> Option<Vec<(Range<usize>, String)>> {
    for made in runs {
        for (range, _) in &mut edits {
            let (mut added, mut removed) = (0, 0);
            for (done, len) in made {
                if done.end <= range.start {
                    // Before it, or an insert where it starts.
                    added += len;
                    removed += done.len();
                } else if done.start < range.end
                    || (done.start == range.start && range.start == range.end)
                {
                    // Into it, or it's an insert where a replacement starts.
                    return None;
                }
            }
            *range = (range.start + added - removed)..(range.end + added - removed);
        }
    }
    Some(edits)
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

#[cfg_attr(not(test), allow(dead_code))]
#[path = "refactor_preview.rs"]
pub(crate) mod refactor_preview;

#[cfg(test)]
#[path = "workspace_edit_tests.rs"]
mod tests;
