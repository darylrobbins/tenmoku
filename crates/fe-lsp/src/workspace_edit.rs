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
//!   the operation's options. A text edit to a file created or renamed to
//!   earlier is decoded against the text the server had for it: a
//!   rename's, the file it was renamed from as it was then, or empty for a
//!   created one. If the server has that file open, the edit is
//!   [`EditTarget::Open`] for its buffer and version, which the host
//!   checks as any other; otherwise [`EditTarget::Closed`]. A create or
//!   rename that `ignoreIfExists` may skip (without `overwrite`) is made
//!   only if its target isn't there, which only the host finds out: a text
//!   edit whose file depends on it is [`EditTarget::Depends`], decoded both
//!   ways (made, as above; skipped, the target's own text, and a rename's
//!   source still there). A version on an edit to a path an operation
//!   covers is refused, as the server can't know one for a file the edit
//!   may make; a null version is accepted. Operations
//!   cover the paths under a directory, compared by components: a delete
//!   removes them, a rename moves them (the old paths are gone, unless it
//!   may be skipped, and those under the new name are fresh), and a later
//!   create makes a path fresh again. fe-lsp advertises all three (`resourceOperations`), with
//!   `failureHandling` `abort`: the host applies them in order with the
//!   text edits, stopping at the first that fails.
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
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::path::{Path, PathBuf};
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
    /// Which occurrence of an operation this is: `Some` for each file
    /// operation, `None` for text edits. Unique across edits, so equal
    /// operations stay apart, and kept by joining.
    pub operation: Option<OperationId>,
}

/// The identity of one file operation in a decoded workspace edit, which
/// an [`EditTarget::Depends`] names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OperationId(u64);

impl OperationId {
    fn mint() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        Self(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}

/// Where a file's edits apply, and the edits.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditTarget {
    /// The server has the file open as `buffer`: byte edits of its text at
    /// `version`, sorted by start, none overlapping. For a path an earlier
    /// rename of the edit moved to, `buffer` is the file it was renamed
    /// from, which its editors follow to the path.
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
    /// Text edits whose file depends on whether `on`, a create or rename
    /// `ignoreIfExists` may skip, is made: the target if it's made, and if
    /// it's skipped, or why the edits can't apply that way. `on` is that
    /// operation's [`FileEdits::operation`]. Each is [`Open`](Self::Open),
    /// [`Closed`](Self::Closed), or another `Depends` on an earlier
    /// operation.
    Depends {
        on: OperationId,
        made: Arc<Result<EditTarget, String>>,
        skipped: Arc<Result<EditTarget, String>>,
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
    /// The edit created it, renamed another file to it, or renamed a
    /// directory to a path above it, earlier: a version for it is refused.
    fresh: bool,
    /// What the path holds, which the edits are against: a node of the
    /// reader's [`Graph`] ([`Reader::holds`]).
    holds: usize,
}

/// What a path holds once the edit's earlier operations are made, for
/// decoding the text edits to it: a node of a [`Graph`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Holds {
    /// The text this file had before the edit: its document, if the server
    /// has it open, else what's on disk.
    File(PathBuf),
    /// Created, empty.
    Empty,
    /// Deleted or renamed away: why edits to it are refused.
    Gone(String),
    /// Depending on whether the `op`th operation, which `ignoreIfExists`
    /// may skip, is made: the nodes it holds then.
    Either {
        op: usize,
        made: usize,
        skipped: usize,
    },
}

/// What the edit's paths come to, each node once: equal nodes are one,
/// and a node's parts come before it. Operations that may be skipped
/// branch what a path holds, and branches meet again, so the nodes are
/// shared rather than a tree, which would double with each.
#[derive(Default)]
struct Graph {
    nodes: Vec<Holds>,
    numbers: HashMap<Holds, usize>,
    /// For each node, the [`Holds::Gone`] it comes to whichever way the
    /// operations go, if it does.
    gone: Vec<Option<usize>>,
    /// The node a path holds once a number of effects are made, for each
    /// worked out.
    memo: HashMap<(PathBuf, usize), usize>,
}

impl Graph {
    /// `holds`'s number, made if it's new; an [`Holds::Either`] the same
    /// both ways is the node it is.
    fn node(&mut self, holds: Holds) -> usize {
        if let Holds::Either { made, skipped, .. } = holds
            && made == skipped
        {
            return made;
        }
        if let Some(&number) = self.numbers.get(&holds) {
            return number;
        }
        let number = self.nodes.len();
        let gone = match &holds {
            Holds::Gone(_) => Some(number),
            Holds::Either { made, skipped, .. } => self.gone[*skipped].and(self.gone[*made]),
            Holds::File(_) | Holds::Empty => None,
        };
        self.nodes.push(holds.clone());
        self.numbers.insert(holds, number);
        self.gone.push(gone);
        number
    }
}

/// What a path holds after some effects, in terms of what it held before
/// the latest of them that covers it.
enum Next {
    Is(Holds),
    /// The same as this path after this many effects.
    Same(PathBuf, usize),
    /// Depending on the `op`th operation: `made`, a node or a path after a
    /// number of effects, or, skipped, this one.
    Choice {
        op: usize,
        made: Result<Holds, (PathBuf, usize)>,
        skipped: (PathBuf, usize),
    },
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
    /// What its operations did to paths, in order. An effect covers the
    /// path and every path under it, by components; the last one that
    /// covers a path is the one that holds.
    effects: Vec<(PathBuf, Effect)>,
    /// The operations read so far.
    operations: Vec<OperationId>,
    graph: Graph,
}

/// What an operation did to a path and those under it. `kept` is the
/// operation's number if `ignoreIfExists` (without `overwrite`) may skip
/// it, leaving the path as it was.
#[derive(Clone, Debug)]
enum Effect {
    /// Deleted, or renamed away: how.
    Removed {
        how: &'static str,
        kept: Option<usize>,
    },
    Created {
        kept: Option<usize>,
    },
    /// Renamed to, from `from`. Always just after `from`'s
    /// [`Effect::Removed`].
    Renamed {
        from: PathBuf,
        kept: Option<usize>,
    },
}

impl Reader {
    /// The latest effect an earlier operation had on `path`, if any.
    fn effect(&self, path: &Path) -> Option<&Effect> {
        (self.effects.iter().rev())
            .find(|(under, _)| path.starts_with(under))
            .map(|(_, effect)| effect)
    }

    /// What `path` holds once the first `before` effects are made: its
    /// node. Each path after a number of effects is worked out once, from
    /// the latest effect covering it, without recursing: a long chain of
    /// renames is a long walk, not a deep stack.
    fn holds(&mut self, path: &Path, before: usize) -> usize {
        let mut stack = vec![(path.to_owned(), before)];
        while let Some(key) = stack.last().cloned() {
            if self.graph.memo.contains_key(&key) {
                stack.pop();
                continue;
            }
            let number = match self.next(&key.0, key.1) {
                Next::Is(holds) => Some(self.graph.node(holds)),
                Next::Same(path, before) => {
                    let found = self.graph.memo.get(&(path.clone(), before)).copied();
                    if found.is_none() {
                        stack.push((path, before));
                    }
                    found
                }
                Next::Choice { op, made, skipped } => {
                    let made = match made {
                        Ok(holds) => Some(self.graph.node(holds)),
                        Err(key) => self.graph.memo.get(&key).copied().or_else(|| {
                            stack.push(key);
                            None
                        }),
                    };
                    let skipped = self.graph.memo.get(&skipped).copied().or_else(|| {
                        stack.push(skipped);
                        None
                    });
                    made.zip(skipped)
                        .map(|(made, skipped)| self.graph.node(Holds::Either { op, made, skipped }))
                }
            };
            if let Some(number) = number {
                self.graph.memo.insert(key, number);
                stack.pop();
            }
        }
        self.graph.memo[&(path.to_owned(), before)]
    }

    /// What `path` holds after the first `before` effects, by the latest
    /// of them that covers it. Skipped, an operation leaves the path as it
    /// was before it.
    fn next(&self, path: &Path, before: usize) -> Next {
        let found = (self.effects[..before].iter().enumerate().rev())
            .find(|(_, (under, _))| path.starts_with(under));
        let Some((at, (under, effect))) = found else {
            return Next::Is(Holds::File(path.to_owned()));
        };
        let kept_or = |kept: &Option<usize>, made: Result<Holds, (PathBuf, usize)>, before| match (
            kept, made,
        ) {
            (Some(op), made) => Next::Choice {
                op: *op,
                made,
                skipped: (path.to_owned(), before),
            },
            (None, Ok(holds)) => Next::Is(holds),
            (None, Err((path, before))) => Next::Same(path, before),
        };
        match effect {
            Effect::Removed { how, kept } => {
                let display = path.display();
                let gone = Holds::Gone(format!("it edits {display}, which it {how} earlier"));
                kept_or(kept, Ok(gone), at)
            }
            Effect::Created { kept } => kept_or(kept, Ok(Holds::Empty), at),
            Effect::Renamed { from, kept } => {
                let rest = path.strip_prefix(under).unwrap_or(Path::new(""));
                let from = match rest.as_os_str().is_empty() {
                    true => from.clone(),
                    false => from.join(rest),
                };
                // Before the rename, whose source's removal comes just
                // ahead of this.
                kept_or(kept, Err((from, at - 1)), at - 1)
            }
        }
    }

    fn add(&mut self, uri: Url, version: Option<i32>, edits: Vec<TextEdit>) -> Result<(), String> {
        let path = file_path(&uri)?;
        let holds = self.holds(&path, self.effects.len());
        if let Some(gone) = self.graph.gone[holds]
            && let Holds::Gone(why) = &self.graph.nodes[gone]
        {
            return Err(why.clone());
        }
        let fresh = self.effect(&path).is_some();
        let at = match self.files.iter().position(|file| file.path == path) {
            Some(at) => at,
            None => {
                self.files.push(Sent {
                    uri,
                    fresh,
                    holds,
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
        let op_number = self.operations.len();
        let kept = |overwrite: bool, ignore_if_exists: bool| {
            (ignore_if_exists && !overwrite).then_some(op_number)
        };
        let (path, target) = match op {
            ResourceOp::Create(create) => {
                let path = file_path(&create.uri)?;
                let options = create.options;
                let overwrite = options.as_ref().and_then(|o| o.overwrite).unwrap_or(false);
                let ignore_if_exists = (options.as_ref())
                    .and_then(|o| o.ignore_if_exists)
                    .unwrap_or(false);
                let kept = kept(overwrite, ignore_if_exists);
                self.effects.push((path.clone(), Effect::Created { kept }));
                let target = EditTarget::Create {
                    overwrite,
                    ignore_if_exists,
                };
                (path, target)
            }
            ResourceOp::Rename(rename) => {
                let (from, to) = (file_path(&rename.old_uri)?, file_path(&rename.new_uri)?);
                let options = rename.options;
                let overwrite = options.as_ref().and_then(|o| o.overwrite).unwrap_or(false);
                let ignore_if_exists = (options.as_ref())
                    .and_then(|o| o.ignore_if_exists)
                    .unwrap_or(false);
                let kept = kept(overwrite, ignore_if_exists);
                let removed = Effect::Removed {
                    how: "renamed",
                    kept,
                };
                self.effects.push((from.clone(), removed));
                let renamed = Effect::Renamed {
                    from: from.clone(),
                    kept,
                };
                self.effects.push((to.clone(), renamed));
                let target = EditTarget::Rename {
                    to,
                    overwrite,
                    ignore_if_exists,
                };
                (from, target)
            }
            ResourceOp::Delete(delete) => {
                let path = file_path(&delete.uri)?;
                let options = delete.options;
                let removed = Effect::Removed {
                    how: "deleted",
                    kept: None,
                };
                self.effects.push((path.clone(), removed));
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
        let id = OperationId::mint();
        self.operations.push(id);
        self.steps.push(Step::Operation(FileEdits {
            path,
            target,
            operation: Some(id),
        }));
        Ok(())
    }

    /// The steps, the operations, which [`Holds::Either`] numbers, and
    /// the graph.
    fn finish(mut self) -> (Vec<Step>, Vec<OperationId>, Graph) {
        if !self.files.is_empty() {
            self.steps.push(Step::Edits(self.files));
        }
        (self.steps, self.operations, self.graph)
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
    let (steps, operations, graph) = reader.finish();
    for step in steps {
        match step {
            Step::Edits(files) => {
                for file in files {
                    let empty = file.edits.is_empty();
                    let file = decode_file(file, documents, &operations, &graph, encoding)?;
                    // Only once every file's version has been checked.
                    if !empty {
                        decoded.push(file);
                    }
                }
            }
            Step::Operation(operation) => decoded.push(operation),
        }
    }
    Ok(decoded)
}

/// One file's edits, decoded against what its path holds
/// ([`Holds`]): for a file `documents` has open, at the version there,
/// which every version the edit names must be.
fn decode_file(
    file: Sent,
    documents: &[SentDocument],
    operations: &[OperationId],
    graph: &Graph,
    encoding: PositionEncoding,
) -> Result<FileEdits, String> {
    if let Some(version) = file.versions.first().filter(|_| file.fresh) {
        return Err(format!(
            "{} is made by the edit, which can't know its version {version}",
            file.path.display()
        ));
    }
    let target = Arc::unwrap_or_clone(decode_holds(&file, documents, operations, graph, encoding))?;
    Ok(FileEdits {
        path: file.path,
        target,
        operation: None,
    })
}

/// A decoded target, or why the edits can't apply that way.
type Decoded = Arc<Result<EditTarget, String>>;

/// `file`'s edits against what its path holds, each node it can come to
/// decoded once, parts first. Equal targets are one: a leaf by what decodes
/// it, a [`EditTarget::Depends`] by its operation and parts, which collapses
/// if its parts are one.
fn decode_holds(
    file: &Sent,
    documents: &[SentDocument],
    operations: &[OperationId],
    graph: &Graph,
    encoding: PositionEncoding,
) -> Decoded {
    let display = file.path.display();
    // The nodes it can come to, in order: each node's parts come before
    // it.
    let mut reachable = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![file.holds];
    while let Some(number) = stack.pop() {
        if !seen.insert(number) {
            continue;
        }
        reachable.push(number);
        if let Holds::Either { made, skipped, .. } = &graph.nodes[number] {
            stack.extend([*made, *skipped]);
        }
    }
    reachable.sort_unstable();
    let mut decoded: HashMap<usize, Decoded> = HashMap::new();
    let mut closed: Option<Decoded> = None;
    let mut open: HashMap<usize, Decoded> = HashMap::new();
    // By operation and the addresses of its parts.
    let mut depends: HashMap<(usize, usize, usize), Decoded> = HashMap::new();
    for number in reachable {
        let mut closed = || {
            closed
                .get_or_insert_with(|| {
                    Arc::new(
                        sorted(file.edits.clone())
                            .map(|edits| EditTarget::Closed { edits, encoding })
                            .map_err(|why| format!("{display}: {why}")),
                    )
                })
                .clone()
        };
        let target = match &graph.nodes[number] {
            Holds::Gone(why) => Arc::new(Err(why.clone())),
            Holds::Empty => closed(),
            Holds::Either { op, made, skipped } => {
                let (made, skipped) = (&decoded[made], &decoded[skipped]);
                if Arc::ptr_eq(made, skipped) || (made.is_err() && skipped.is_err()) {
                    made.clone()
                } else {
                    let key = (*op, Arc::as_ptr(made).addr(), Arc::as_ptr(skipped).addr());
                    (depends.entry(key))
                        .or_insert_with(|| {
                            Arc::new(Ok(EditTarget::Depends {
                                on: operations[*op],
                                made: made.clone(),
                                skipped: skipped.clone(),
                            }))
                        })
                        .clone()
                }
            }
            Holds::File(origin) => {
                let mut matching = (documents.iter().enumerate()).filter(|(_, document)| {
                    (!file.fresh && document.uri == file.uri)
                        || file_path(&document.uri).is_ok_and(|path| path == *origin)
                });
                match (matching.next(), matching.next()) {
                    (Some(_), Some(_)) => Arc::new(Err(format!("{display} is open twice"))),
                    (Some((at, document)), None) => open
                        .entry(at)
                        .or_insert_with(|| Arc::new(decode_open(file, document, encoding)))
                        .clone(),
                    (None, _) => match file.versions.first() {
                        Some(version) => {
                            Arc::new(Err(format!("{display} isn't open at version {version}")))
                        }
                        None => closed(),
                    },
                }
            }
        };
        decoded.insert(number, target);
    }
    decoded
        .remove(&file.holds)
        .expect("its own node is reachable")
}

/// `file`'s edits against `document`, which the server has open.
fn decode_open(
    file: &Sent,
    document: &SentDocument,
    encoding: PositionEncoding,
) -> Result<EditTarget, String> {
    let display = file.path.display();
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
    Ok(EditTarget::Open {
        buffer: document.buffer,
        version: sent.version,
        edits,
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
    /// Why no more are taken: the session is over, and the router, still
    /// reading while the server shuts down, answers them at once.
    closed: Cell<Option<&'static str>>,
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
        let reply = ApplyReply(slot);
        if let Some(why) = self.closed.get() {
            reply.failed(why);
        } else {
            self.queue.borrow_mut().push_back(Asked { params, reply });
            if let Some(actor) = self.waiting.take() {
                actor.wake();
            }
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

    /// Answers every request still queued failed, for `reason`: the actor
    /// won't take them, its session is over.
    pub(crate) fn fail_all(&self, reason: &'static str) {
        self.closed.set(Some(reason));
        for asked in self.queue.borrow_mut().drain(..) {
            asked.reply.failed(reason);
        }
    }
}

#[cfg(test)]
#[path = "workspace_edit_tests.rs"]
mod tests;
