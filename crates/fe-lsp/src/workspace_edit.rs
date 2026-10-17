//! Workspace edits: a server's `WorkspaceEdit` (a rename's or a code
//! action's answer, or a `workspace/applyEdit`) as edits in bytes, for the
//! host to apply.
//!
//! - **Open files.** A file the server has open is decoded against the
//!   text the server has for it, as hover decodes ranges, into byte edits
//!   of that buffer version ([`EditTarget::Open`]). The host applies them
//!   only to that version, or rebases them.
//! - **Closed files.** A file the server doesn't have open keeps its LSP
//!   edits ([`EditTarget::Closed`]): the host reads the file and decodes
//!   them against its text with [`decode_closed`].
//! - **Refused.** The whole edit is refused if any part of it can't be
//!   applied as the server meant: a `TextDocumentEdit` for a version the
//!   server doesn't have (the buffer moved on, or the file isn't open),
//!   even one with no edits in it, a text edit to a file the edit deleted
//!   or renamed away earlier, overlapping edits in a file, a range that
//!   ends before it starts, or a URI that isn't a `file:` path (a resource
//!   operation's too).
//! - **Resource operations.** A `documentChanges` with a create, rename or
//!   delete keeps its order: the output lists, in order, each run of text
//!   edits (merged per file) and each operation ([`EditTarget::Create`],
//!   [`Rename`](EditTarget::Rename), [`Delete`](EditTarget::Delete)), with
//!   the operation's options. Text edits to a file created or renamed to
//!   earlier are [`EditTarget::Closed`], against empty text for a created
//!   one. fe-lsp decodes them but doesn't advertise them yet
//!   (`resourceOperations` stays empty), and the workbench refuses them.
//! - Both forms are read: `documentChanges` if the server sent it,
//!   otherwise `changes`. Several `TextDocumentEdit`s for one file are
//!   merged, all read against the same text, and each file's edits come
//!   sorted by start (inserts at one place keep the server's order).
//! - **applyEdit.** The router hands each `workspace/applyEdit` to the
//!   actor, which decodes it and sends it to the sink as
//!   [`LspEvent::ApplyEdit`](crate::LspEvent::ApplyEdit), or answers
//!   `applied: false` if it's refused. The host answers through its
//!   [`ApplyReply`], or [claims](ApplyReply::claim) it first, so the
//!   timeout no longer answers for it while it applies the edit; an edit
//!   with nothing in it is applied at once. When the session ends, crashed
//!   or stopped, the replies it handed out are answered failed, so none is
//!   applied after. The
//!   actor decodes against each document as last handed to the server
//!   (`server::Handed`), not as its newest version, whose notes may still
//!   wait in the outbox. An applyEdit that timed out before the actor got
//!   to it is dropped.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use fe_buffer::{BufferId, Version};
use fe_text::Text;
use futures::channel::oneshot;
use futures::{Future, FutureExt, select_biased};
use lsp_types::{
    ApplyWorkspaceEditParams, ApplyWorkspaceEditResponse, DocumentChangeOperation, DocumentChanges,
    OneOf, ResourceOp, TextEdit, Url, WorkspaceEdit,
};

use crate::clock::Clock;
use crate::document::{Admitted, Opened, SentDocument};
use crate::encoding::{PositionEncoding, from_lsp};
use crate::event::{LspError, ServerId};
use crate::manager::LspManager;
use crate::server::Command;

/// How long the host has to answer a `workspace/applyEdit` before it's
/// answered `applied: false` for it.
pub(crate) const APPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// One file's part of a workspace edit.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FileEdits {
    pub path: PathBuf,
    pub target: EditTarget,
}

/// Where a file's edits apply, and the edits.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditTarget {
    /// The server has the file open as `buffer`: byte edits of its text at
    /// `version`, sorted by start, none overlapping.
    Open {
        buffer: BufferId,
        version: Version,
        edits: Vec<(Range<usize>, String)>,
    },
    /// The server doesn't have the file open: its edits as sent, sorted by
    /// start, for [`decode_closed`] against the file's text.
    Closed {
        edits: Vec<TextEdit>,
        encoding: PositionEncoding,
    },
    /// Create the file (an entry in the edit's order; text edits to it come
    /// after, [`Closed`](Self::Closed), against empty text).
    Create {
        overwrite: bool,
        ignore_if_exists: bool,
    },
    /// Rename the file to `to`; text edits after it name `to`, closed.
    Rename {
        to: PathBuf,
        overwrite: bool,
        ignore_if_exists: bool,
    },
    /// Delete the file or folder.
    Delete {
        recursive: bool,
        ignore_if_not_exists: bool,
    },
}

/// `edits` (a [`EditTarget::Closed`]'s) as byte edits of `text`, sorted by
/// start, or why they can't be applied: they overlap, or one ends before it
/// starts.
pub fn decode_closed(
    text: &Text,
    edits: &[TextEdit],
    encoding: PositionEncoding,
) -> Result<Vec<(Range<usize>, String)>, String> {
    // Decoding clamps, but keeps the order of positions, so edits sorted
    // and apart stay so in bytes.
    Ok((sorted(edits.to_vec())?.into_iter())
        .map(|edit| {
            let start = from_lsp(text, edit.range.start, encoding);
            let end = from_lsp(text, edit.range.end, encoding);
            (start..end, edit.new_text)
        })
        .collect())
}

/// `edits` sorted by start (and end, so inserts at one place keep their
/// order), or why they can't be applied: they overlap, or one ends before
/// it starts.
fn sorted(mut edits: Vec<TextEdit>) -> Result<Vec<TextEdit>, String> {
    if let Some(edit) = edits.iter().find(|edit| edit.range.end < edit.range.start) {
        let (start, end) = (edit.range.start, edit.range.end);
        return Err(format!(
            "an edit ends ({}:{}) before it starts ({}:{})",
            end.line, end.character, start.line, start.character
        ));
    }
    edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
    if (edits.windows(2)).any(|w| w[1].range.start < w[0].range.end) {
        return Err("its edits overlap".into());
    }
    Ok(edits)
}

impl LspManager {
    /// `edit`, from `server`, decoded against the documents the server has
    /// open now, at the versions handed to it. A reply to a request on a
    /// document is better decoded against what it was
    /// [`Admitted`] with ([`Admitted::decode_workspace_edit`]): a buffer
    /// may have moved on since.
    ///
    /// `Err(LspError::RefusedEdit)` if any of it can't be applied, and
    /// `ServerDown` if the server isn't running.
    pub fn decode_workspace_edit(
        &self,
        server: ServerId,
        edit: WorkspaceEdit,
    ) -> impl Future<Output = Result<Vec<FileEdits>, LspError>> + Send + 'static + use<> {
        let (answer, answered) = oneshot::channel();
        self.send(
            server,
            Command::Documents(Box::new(move |opened| {
                let _ = answer.send(opened.decode_workspace_edit(edit));
            })),
        );
        answered.map(|answer| answer.unwrap_or(Err(LspError::ServerDown)))
    }
}

impl Opened {
    /// `edit` decoded against these documents: see the
    /// [module](crate::workspace_edit). `Err(LspError::RefusedEdit)` if any
    /// of it can't be applied.
    pub fn decode_workspace_edit(&self, edit: WorkspaceEdit) -> Result<Vec<FileEdits>, LspError> {
        decode(edit, &self.documents, self.encoding).map_err(LspError::RefusedEdit)
    }
}

impl Admitted {
    /// `edit` decoded against the request's document, at the version the
    /// request named, and the others the server had open: see the
    /// [module](crate::workspace_edit). `Err(LspError::RefusedEdit)` if any
    /// of it can't be applied.
    pub fn decode_workspace_edit(&self, edit: WorkspaceEdit) -> Result<Vec<FileEdits>, LspError> {
        let mut documents = Vec::with_capacity(self.others.len() + 1);
        documents.push(self.document.clone());
        documents.extend(self.others.iter().cloned());
        decode(edit, &documents, self.encoding).map_err(LspError::RefusedEdit)
    }
}

/// One file's edits as the server sent them, before decoding.
struct Sent {
    uri: Url,
    path: PathBuf,
    /// The versions the server named for it, if any.
    versions: Vec<i32>,
    edits: Vec<TextEdit>,
    /// The edit created it, or renamed another file to it, earlier: it's
    /// closed, whatever the server has open under the path.
    fresh: bool,
}

/// A step of a workspace edit, in the order the server sent them.
enum Step {
    /// A run of text edits, merged per file.
    Edits(Vec<Sent>),
    Operation(FileEdits),
}

/// Reads an edit's parts in order, tracking which files its operations
/// have made or removed.
#[derive(Default)]
struct Reader {
    steps: Vec<Step>,
    /// The current run of text edits.
    files: Vec<Sent>,
    /// Created, or renamed to, earlier in the edit.
    fresh: HashSet<PathBuf>,
    /// Deleted or renamed away earlier in the edit, and which.
    gone: HashMap<PathBuf, &'static str>,
}

impl Reader {
    fn add(&mut self, uri: Url, version: Option<i32>, edits: Vec<TextEdit>) -> Result<(), String> {
        let path = file_path(&uri)?;
        if let Some(how) = self.gone.get(&path) {
            return Err(format!(
                "it edits {}, which it {how} earlier",
                path.display()
            ));
        }
        let at = match self.files.iter().position(|file| file.path == path) {
            Some(at) => at,
            None => {
                self.files.push(Sent {
                    uri,
                    fresh: self.fresh.contains(&path),
                    path,
                    versions: Vec::new(),
                    edits: Vec::new(),
                });
                self.files.len() - 1
            }
        };
        self.files[at].versions.extend(version);
        self.files[at].edits.extend(edits);
        Ok(())
    }

    fn operation(&mut self, op: ResourceOp) -> Result<(), String> {
        if !self.files.is_empty() {
            self.steps
                .push(Step::Edits(std::mem::take(&mut self.files)));
        }
        let (path, target) = match op {
            ResourceOp::Create(create) => {
                let path = file_path(&create.uri)?;
                let options = create.options;
                self.gone.remove(&path);
                self.fresh.insert(path.clone());
                let target = EditTarget::Create {
                    overwrite: options.as_ref().and_then(|o| o.overwrite).unwrap_or(false),
                    ignore_if_exists: options
                        .as_ref()
                        .and_then(|o| o.ignore_if_exists)
                        .unwrap_or(false),
                };
                (path, target)
            }
            ResourceOp::Rename(rename) => {
                let (from, to) = (file_path(&rename.old_uri)?, file_path(&rename.new_uri)?);
                let options = rename.options;
                self.fresh.remove(&from);
                self.gone.insert(from.clone(), "renamed");
                self.gone.remove(&to);
                self.fresh.insert(to.clone());
                let target = EditTarget::Rename {
                    to,
                    overwrite: options.as_ref().and_then(|o| o.overwrite).unwrap_or(false),
                    ignore_if_exists: options
                        .as_ref()
                        .and_then(|o| o.ignore_if_exists)
                        .unwrap_or(false),
                };
                (from, target)
            }
            ResourceOp::Delete(delete) => {
                let path = file_path(&delete.uri)?;
                let options = delete.options;
                self.fresh.remove(&path);
                self.gone.insert(path.clone(), "deleted");
                let target = EditTarget::Delete {
                    recursive: options.as_ref().and_then(|o| o.recursive).unwrap_or(false),
                    ignore_if_not_exists: options
                        .as_ref()
                        .and_then(|o| o.ignore_if_not_exists)
                        .unwrap_or(false),
                };
                (path, target)
            }
        };
        self.steps.push(Step::Operation(FileEdits { path, target }));
        Ok(())
    }

    fn finish(mut self) -> Vec<Step> {
        if !self.files.is_empty() {
            self.steps.push(Step::Edits(self.files));
        }
        self.steps
    }
}

/// `edit` decoded against `documents`, the documents the server has open,
/// or why it's refused.
pub(crate) fn decode(
    edit: WorkspaceEdit,
    documents: &[SentDocument],
    encoding: PositionEncoding,
) -> Result<Vec<FileEdits>, String> {
    let mut reader = Reader::default();
    match edit.document_changes {
        Some(DocumentChanges::Edits(edits)) => {
            for edit in edits {
                let edits = edit.edits.into_iter().map(text_edit).collect();
                reader.add(edit.text_document.uri, edit.text_document.version, edits)?;
            }
        }
        Some(DocumentChanges::Operations(operations)) => {
            for operation in operations {
                match operation {
                    DocumentChangeOperation::Edit(edit) => {
                        let edits = edit.edits.into_iter().map(text_edit).collect();
                        reader.add(edit.text_document.uri, edit.text_document.version, edits)?;
                    }
                    DocumentChangeOperation::Op(op) => reader.operation(op)?,
                }
            }
        }
        None => {
            // A map: its files in an order that doesn't change.
            let mut changes: Vec<_> = edit.changes.unwrap_or_default().into_iter().collect();
            changes.sort_by(|a, b| a.0.cmp(&b.0));
            for (uri, edits) in changes {
                reader.add(uri, None, edits)?;
            }
        }
    }
    let mut decoded = Vec::new();
    for step in reader.finish() {
        match step {
            Step::Edits(files) => {
                for file in files {
                    decoded.push(decode_file(file, documents, encoding)?);
                }
            }
            Step::Operation(operation) => decoded.push(operation),
        }
    }
    // Only once every file's version has been checked.
    decoded.retain(|file| match &file.target {
        EditTarget::Open { edits, .. } => !edits.is_empty(),
        EditTarget::Closed { edits, .. } => !edits.is_empty(),
        _ => true,
    });
    Ok(decoded)
}

/// One file's edits, decoded if `documents` has it open: at the version
/// there, which every version the edit names must be.
fn decode_file(
    file: Sent,
    documents: &[SentDocument],
    encoding: PositionEncoding,
) -> Result<FileEdits, String> {
    let display = file.path.display();
    let mut open = (documents.iter())
        .filter(|document| {
            document.uri == file.uri || file_path(&document.uri).is_ok_and(|path| path == file.path)
        })
        .filter(|_| !file.fresh);
    let target = match (open.next(), open.next()) {
        (Some(_), Some(_)) => return Err(format!("{display} is open twice")),
        (Some(document), None) => {
            let sent = &document.sent;
            // Every one named, whatever the rest are.
            if let Some(version) = (file.versions.iter()).find(|&&v| v != sent.lsp_version) {
                return Err(format!(
                    "{display} has changed: the edit is for version {version}, \
                     the server has {}",
                    sent.lsp_version
                ));
            }
            let edits = decode_closed(&sent.text, &file.edits, encoding)
                .map_err(|why| format!("{display}: {why}"))?;
            EditTarget::Open {
                buffer: document.buffer,
                version: sent.version,
                edits,
            }
        }
        (None, _) => {
            if let Some(version) = file.versions.first().filter(|_| !file.fresh) {
                return Err(format!("{display} isn't open at version {version}"));
            }
            let edits = sorted(file.edits).map_err(|why| format!("{display}: {why}"))?;
            EditTarget::Closed { edits, encoding }
        }
    };
    Ok(FileEdits {
        path: file.path,
        target,
    })
}

fn text_edit(edit: OneOf<TextEdit, lsp_types::AnnotatedTextEdit>) -> TextEdit {
    match edit {
        OneOf::Left(edit) => edit,
        OneOf::Right(annotated) => annotated.text_edit,
    }
}

/// The local path `uri` names, or why it names none.
pub(crate) fn file_path(uri: &Url) -> Result<PathBuf, String> {
    if uri.scheme() != "file" {
        return Err(format!("{uri} isn't a file"));
    }
    uri.to_file_path()
        .map_err(|()| format!("{uri} isn't a local file"))
}

/// Where a `workspace/applyEdit`'s answer goes: taken by the first answer,
/// the host's or the timeout's.
type Slot = Mutex<Option<oneshot::Sender<ApplyWorkspaceEditResponse>>>;

/// The host's answer to a `workspace/applyEdit`, sent with
/// [`LspEvent::ApplyEdit`](crate::LspEvent::ApplyEdit). Clones share it,
/// and the first answer is the one the server gets, or the host
/// [claims](Self::claim) it. Dropped unanswered, unanswered after 5 s, or
/// unanswered when the server's session ends, it answers `applied: false`.
#[derive(Clone)]
pub struct ApplyReply(Arc<Slot>);

impl ApplyReply {
    /// The edit was applied.
    pub fn applied(self) {
        answer(
            &self.0,
            ApplyWorkspaceEditResponse {
                applied: true,
                failure_reason: None,
                failed_change: None,
            },
        );
    }

    /// The edit wasn't applied, for `reason`.
    pub fn failed(self, reason: impl Into<String>) {
        answer(&self.0, not_applied(reason));
    }

    /// Takes the answer over, unless it has been answered or claimed: the
    /// timeout and the session's end no longer answer it, and the server
    /// waits for the claim's answer. The host claims it before it starts
    /// changing anything, so the server never hears `applied: false` for
    /// an edit that was applied.
    pub fn claim(&self) -> Option<ApplyClaim> {
        let mut slot = lock(&self.0);
        slot.take_if(|answer| !answer.is_canceled()).map(ApplyClaim)
    }

    /// Whether it has been answered, or no longer can be: it timed out, or
    /// the server's session ended, or it was claimed. An answer then
    /// reaches nothing.
    pub fn is_answered(&self) -> bool {
        lock(&self.0)
            .as_ref()
            .is_none_or(oneshot::Sender::is_canceled)
    }
}

/// A reply handed to the host, held weakly, so the host dropping every
/// [`ApplyReply`] still answers it.
pub(crate) struct HandedReply(std::sync::Weak<Slot>);

impl HandedReply {
    pub(crate) fn of(reply: &ApplyReply) -> Self {
        Self(Arc::downgrade(&reply.0))
    }

    /// Whether the host may still answer it.
    pub(crate) fn is_open(&self) -> bool {
        (self.0.upgrade()).is_some_and(|slot| !ApplyReply(slot).is_answered())
    }

    /// Answers it failed, for `reason`, unless it's answered or claimed.
    pub(crate) fn fail(self, reason: &str) {
        if let Some(slot) = self.0.upgrade() {
            answer(&slot, not_applied(reason));
        }
    }
}

/// A `workspace/applyEdit`'s answer, [claimed](ApplyReply::claim) by the
/// host: only it answers now. Dropped unanswered, it answers `applied:
/// false`.
#[derive(Debug)]
pub struct ApplyClaim(oneshot::Sender<ApplyWorkspaceEditResponse>);

impl ApplyClaim {
    /// The edit was applied.
    pub fn applied(self) {
        let _ = self.0.send(ApplyWorkspaceEditResponse {
            applied: true,
            failure_reason: None,
            failed_change: None,
        });
    }

    /// The edit wasn't applied, for `reason`.
    pub fn failed(self, reason: impl Into<String>) {
        let _ = self.0.send(not_applied(reason));
    }
}

/// Answers `slot` with `response`, unless it has been answered. The answer
/// is sent under the lock, so whoever finds it taken knows it has been
/// sent.
fn answer(slot: &Slot, response: ApplyWorkspaceEditResponse) {
    let mut slot = lock(slot);
    if let Some(answer) = slot.take() {
        let _ = answer.send(response);
    }
}

fn lock(
    slot: &Slot,
) -> std::sync::MutexGuard<'_, Option<oneshot::Sender<ApplyWorkspaceEditResponse>>> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

impl PartialEq for ApplyReply {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for ApplyReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApplyReply")
            .field("answered", &self.is_answered())
            .finish()
    }
}

pub(crate) fn not_applied(reason: impl Into<String>) -> ApplyWorkspaceEditResponse {
    ApplyWorkspaceEditResponse {
        applied: false,
        failure_reason: Some(reason.into()),
        failed_change: None,
    }
}

/// A `workspace/applyEdit` the router read, for the actor to decode, and
/// where its answer goes.
pub(crate) struct Asked {
    pub(crate) params: ApplyWorkspaceEditParams,
    pub(crate) reply: ApplyReply,
}

/// `workspace/applyEdit`s the router has read and the actor hasn't decoded
/// yet. Both run on the server's thread.
#[derive(Default)]
pub(crate) struct Edits {
    queue: RefCell<VecDeque<Asked>>,
    /// The actor, waiting for one.
    waiting: Cell<Option<Waker>>,
}

impl Edits {
    /// Queues `params` for the actor; resolves to the answer for the
    /// server: `applied: false` if none comes within [`APPLY_TIMEOUT`] on
    /// `clock`, which answers its [`ApplyReply`] too, or if every
    /// [`ApplyReply`] is dropped unanswered.
    pub(crate) fn ask(
        &self,
        params: ApplyWorkspaceEditParams,
        clock: &dyn Clock,
    ) -> impl Future<Output = ApplyWorkspaceEditResponse> + Send + 'static + use<> {
        let (sender, answered) = oneshot::channel();
        let slot = Arc::new(Mutex::new(Some(sender)));
        // Weak, so the replies' drop drops the sender.
        let timed_out = Arc::downgrade(&slot);
        self.queue.borrow_mut().push_back(Asked {
            params,
            reply: ApplyReply(slot),
        });
        if let Some(actor) = self.waiting.take() {
            actor.wake();
        }
        let timeout = clock.sleep(APPLY_TIMEOUT);
        async move {
            let mut answered = answered.fuse();
            let mut timeout = timeout.fuse();
            let not_answered = || not_applied("not answered");
            select_biased! {
                answer = answered => answer.unwrap_or_else(|oneshot::Canceled| not_answered()),
                () = timeout => {
                    if let Some(slot) = timed_out.upgrade() {
                        answer(&slot, not_applied("not answered in time"));
                    }
                    // Taken by the timeout, or by an answer that has been
                    // sent, and here now; or by a claim, which answers
                    // when it's done; or dropped, and nothing will come.
                    answered.await.unwrap_or_else(|oneshot::Canceled| not_answered())
                }
            }
        }
    }

    /// Ready with every request queued that can still be answered, once
    /// there's one.
    pub(crate) fn poll_take(&self, cx: &Context<'_>) -> Poll<Vec<Asked>> {
        let mut queue = self.queue.borrow_mut();
        queue.retain(|asked| !asked.reply.is_answered());
        if queue.is_empty() {
            self.waiting.set(Some(cx.waker().clone()));
            return Poll::Pending;
        }
        Poll::Ready(queue.drain(..).collect())
    }
}

#[cfg(test)]
#[path = "workspace_edit_tests.rs"]
mod tests;
