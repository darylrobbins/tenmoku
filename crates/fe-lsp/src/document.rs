//! An open document's sync with its server (docs/decisions/lsp-client.md
//! §4.1): `didOpen`, then each batch the buffer's [`Subscription`] queued
//! as `didChange`, `didSave` at each save, and `didClose`.
//!
//! The server's actor drains the subscription, on its own thread, when the
//! host nudges ([`LspManager::changed`](crate::LspManager::changed)) and on
//! a backstop tick. A drain turns its events into [`Note`]s, in order:
//!
//! - Consecutive `Edited` batches coalesce into one `didChange`, their
//!   changes concatenated in batch order.
//! - A `Saved` is a barrier: the `didChange` for the batches before it,
//!   then `didSave` (with the text as sent, which is the saved text, when
//!   the server asks for it), then a new `didChange` for those after it.
//! - A `didChange` is one change with no range, holding the whole text,
//!   when the server syncs `Full`, when the drain's batches hold more than
//!   [`MAX_EDITS`] edits, or when their new text is more than half the
//!   document they leave. Otherwise each batch's edits become one change
//!   each, walked in reverse against the text as sent before the batch, so
//!   no change moves a later one's range.
//!
//! The subscription is shared with the manager ([`Subscribed`]), so that
//! [`LspManager::close`](crate::LspManager::close) can detach it on the
//! host's thread: the events queued so far stay for the actor's drains, in
//! order, and an edit made after `close` returns is never sent. Whatever
//! ends the actor's hold on a document ([`Attached`], dropped with the
//! document or with an opening still queued: a close, a crash, a failed
//! start, a panic) drops the subscription too, so no buffer keeps queueing
//! events for a server that will never drain them.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use fe_buffer::{BufferEvent, BufferId, Subscription, Version};
use fe_text::Text;
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, Range, ServerCapabilities, TextDocumentContentChangeEvent,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncSaveOptions, Url, VersionedTextDocumentIdentifier,
};

use crate::encoding::{PositionEncoding, to_lsp};

/// The most edits one `didChange` sends as ranges; more go as the whole
/// text.
pub(crate) const MAX_EDITS: usize = 64;

/// How a server asked for documents to be synced (`textDocumentSync`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SyncPolicy {
    /// `didOpen` and `didClose`. Without them nothing is sent for a
    /// document: a server can't be told of changes to one it doesn't have.
    open_close: bool,
    change: TextDocumentSyncKind,
    /// `didSave`, and whether it carries the text.
    save: Option<bool>,
}

impl SyncPolicy {
    /// What `capabilities` asks for. As in the LSP spec, an absent
    /// `textDocumentSync` means `None`, and so does an absent field of its
    /// options. A bare kind other than `None` means open, close and change
    /// with that kind, and `didSave` without the text, as VS Code reads it.
    pub(crate) fn from_capabilities(capabilities: &ServerCapabilities) -> Self {
        match &capabilities.text_document_sync {
            None => Self::NONE,
            Some(TextDocumentSyncCapability::Kind(kind)) if *kind == TextDocumentSyncKind::NONE => {
                Self::NONE
            }
            Some(TextDocumentSyncCapability::Kind(kind)) => Self {
                open_close: true,
                change: known(*kind),
                save: Some(false),
            },
            Some(TextDocumentSyncCapability::Options(options)) => Self {
                open_close: options.open_close.unwrap_or(false),
                change: options.change.map_or(TextDocumentSyncKind::NONE, known),
                save: match &options.save {
                    None | Some(TextDocumentSyncSaveOptions::Supported(false)) => None,
                    Some(TextDocumentSyncSaveOptions::Supported(true)) => Some(false),
                    Some(TextDocumentSyncSaveOptions::SaveOptions(save)) => {
                        Some(save.include_text.unwrap_or(false))
                    }
                },
            },
        }
    }

    const NONE: Self = Self {
        open_close: false,
        change: TextDocumentSyncKind::NONE,
        save: None,
    };
}

/// `kind`, or `Full` for a kind the protocol doesn't define: the whole
/// text is right whatever the server meant.
fn known(kind: TextDocumentSyncKind) -> TextDocumentSyncKind {
    if [
        TextDocumentSyncKind::NONE,
        TextDocumentSyncKind::FULL,
        TextDocumentSyncKind::INCREMENTAL,
    ]
    .contains(&kind)
    {
        kind
    } else {
        log::warn!("unknown textDocumentSync kind {kind:?}; syncing the whole text");
        TextDocumentSyncKind::FULL
    }
}

/// A document's subscription, shared by the manager and the actor.
#[derive(Clone)]
pub(crate) struct Subscribed(Arc<Mutex<Feed>>);

#[derive(Default)]
struct Feed {
    /// Until the manager closes the document, or the actor lets it go.
    subscription: Option<Subscription>,
    /// Events taken from the subscription at `close`, still to drain.
    detached: Vec<BufferEvent>,
}

impl Subscribed {
    pub(crate) fn new(subscription: Subscription) -> Self {
        Self(Arc::new(Mutex::new(Feed {
            subscription: Some(subscription),
            detached: Vec::new(),
        })))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Feed> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes every event queued so far.
    fn take_events(&self) -> Vec<BufferEvent> {
        let mut feed = self.lock();
        let mut events = std::mem::take(&mut feed.detached);
        if let Some(subscription) = &feed.subscription {
            events.extend(subscription.drain());
        }
        events
    }

    /// Drops the subscription, keeping the events it queued until now for
    /// the actor's drains.
    pub(crate) fn detach(&self) {
        let mut feed = self.lock();
        if let Some(subscription) = feed.subscription.take() {
            feed.detached.extend(subscription.drain());
        }
    }

    /// Whether the buffer still queues events for it.
    #[cfg(test)]
    pub(crate) fn is_attached(&self) -> bool {
        self.lock().subscription.is_some()
    }
}

/// The actor's hold on a [`Subscribed`]. Dropping it drops the
/// subscription and any events still queued, however the hold ends.
pub(crate) struct Attached(pub(crate) Subscribed);

impl Drop for Attached {
    fn drop(&mut self) {
        let mut feed = self.0.lock();
        feed.subscription = None;
        feed.detached = Vec::new();
    }
}

/// A document notification, waiting its turn in the actor's queue.
#[derive(Debug)]
pub(crate) enum Note {
    Open(DidOpenTextDocumentParams),
    Change(DidChangeTextDocumentParams),
    Save(DidSaveTextDocumentParams),
    Close(DidCloseTextDocumentParams),
}

impl Note {
    pub(crate) fn send(self, socket: &async_lsp::ServerSocket) -> async_lsp::Result<()> {
        match self {
            Self::Open(params) => socket.notify::<DidOpenTextDocument>(params),
            Self::Change(params) => socket.notify::<DidChangeTextDocument>(params),
            Self::Save(params) => socket.notify::<DidSaveTextDocument>(params),
            Self::Close(params) => socket.notify::<DidCloseTextDocument>(params),
        }
    }
}

/// What [`LspManager::open`](crate::LspManager::open) hands the actor.
pub(crate) struct Opening {
    pub(crate) buffer: BufferId,
    /// The buffer's text at `version`.
    pub(crate) text: Text,
    pub(crate) version: Version,
    /// Subscribed no later than `version`: edits up to it are skipped,
    /// and so are saves of earlier versions.
    pub(crate) subscription: Attached,
    pub(crate) uri: Url,
    pub(crate) language_id: String,
}

/// The LSP `languageId` for a buffer of fe-syntax's `language` (§3.2): a
/// fixed table, with `.jsx` files told apart from plain JavaScript by
/// their extension. Other languages pass their name through.
pub(crate) fn language_id(language: &str, path: &Path) -> String {
    match language {
        "javascript" if path.extension().is_some_and(|ext| ext == "jsx") => "javascriptreact",
        "tsx" => "typescriptreact",
        other => other,
    }
    .to_owned()
}

/// One open document on one server.
pub(crate) struct Document {
    uri: Url,
    subscription: Attached,
    /// The text the server has.
    sent: Text,
    /// The buffer version `sent` is.
    at: Version,
    /// The LSP version of the last `didOpen` or `didChange`.
    version: i32,
}

/// The `didChange` being built from a run of batches.
#[derive(Default)]
struct Run {
    batches: usize,
    /// Send the whole text rather than `changes`.
    whole: bool,
    changes: Vec<TextDocumentContentChangeEvent>,
}

impl Document {
    /// Queues `didOpen`, at LSP version 0, with the LF text.
    pub(crate) fn open(opening: Opening, policy: SyncPolicy, out: &mut VecDeque<Note>) -> Self {
        let document = Self {
            uri: opening.uri,
            subscription: opening.subscription,
            sent: opening.text,
            at: opening.version,
            version: 0,
        };
        if policy.open_close {
            out.push_back(Note::Open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: document.uri.clone(),
                    language_id: opening.language_id,
                    version: document.version,
                    text: document.sent.to_string(),
                },
            }));
        }
        document
    }

    /// Queues `didClose`, after a last drain of what the buffer did until
    /// the manager detached it, so a save just before closing still
    /// reaches the server.
    pub(crate) fn close(
        mut self,
        policy: SyncPolicy,
        encoding: PositionEncoding,
        out: &mut VecDeque<Note>,
    ) {
        self.drain(policy, encoding, out);
        if policy.open_close {
            out.push_back(Note::Close(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: self.uri },
            }));
        }
    }

    /// Takes the subscription's events and queues what they become.
    pub(crate) fn drain(
        &mut self,
        policy: SyncPolicy,
        encoding: PositionEncoding,
        out: &mut VecDeque<Note>,
    ) {
        let events = self.subscription.0.take_events();
        if !policy.open_close {
            // Nothing is sent, but the queue mustn't grow either.
            return;
        }
        let whole = self.too_much(&events);
        let mut run = Run::default();
        for event in events {
            match event {
                BufferEvent::Edited {
                    from,
                    to,
                    edits,
                    text,
                    ..
                } => {
                    if to <= self.at {
                        // Already in the text the document opened with.
                        continue;
                    }
                    run.batches += 1;
                    // A gap (`from` isn't what was sent) means batches went
                    // missing, so only the whole text is right.
                    run.whole |=
                        whole || from != self.at || policy.change == TextDocumentSyncKind::FULL;
                    if run.whole {
                        run.changes.clear();
                    } else {
                        run.changes.extend(edits.iter().rev().map(|edit| {
                            TextDocumentContentChangeEvent {
                                range: Some(Range::new(
                                    to_lsp(&self.sent, edit.old.start, encoding),
                                    to_lsp(&self.sent, edit.old.end, encoding),
                                )),
                                range_length: None,
                                text: text.slice_to_string(edit.new.clone()),
                            }
                        }));
                    }
                    self.sent = text;
                    self.at = to;
                }
                BufferEvent::Saved { version } => {
                    if version < self.at {
                        // Saved before the version the document opened with.
                        continue;
                    }
                    self.flush(std::mem::take(&mut run), policy, out);
                    if let Some(include_text) = policy.save {
                        let text = if !include_text {
                            None
                        } else if version == self.at {
                            Some(self.sent.to_string())
                        } else {
                            // Can't happen: events arrive in order, and
                            // every batch up to `version` came before.
                            log::warn!(
                                "{}: saved {version:?}, but only {:?} was seen; didSave carries no text",
                                self.uri,
                                self.at,
                            );
                            None
                        };
                        out.push_back(Note::Save(DidSaveTextDocumentParams {
                            text_document: TextDocumentIdentifier {
                                uri: self.uri.clone(),
                            },
                            text,
                        }));
                    }
                }
            }
        }
        self.flush(run, policy, out);
    }

    /// Whether the batches in `events` that aren't sent yet hold more than
    /// [`MAX_EDITS`] edits, or new text more than half the document they
    /// leave: then every `didChange` of the drain sends the whole text.
    fn too_much(&self, events: &[BufferEvent]) -> bool {
        let mut edits = 0;
        let mut new_bytes = 0;
        let mut len = None;
        for event in events {
            if let BufferEvent::Edited {
                to,
                edits: batch,
                text,
                ..
            } = event
                && *to > self.at
            {
                edits += batch.len();
                new_bytes += batch.iter().map(|edit| edit.new.len()).sum::<usize>();
                len = Some(text.len());
            }
        }
        len.is_some_and(|len| edits > MAX_EDITS || new_bytes * 2 > len)
    }

    /// Queues the `didChange` for `run`, if it changed anything the server
    /// is told of.
    fn flush(&mut self, run: Run, policy: SyncPolicy, out: &mut VecDeque<Note>) {
        if run.batches == 0 || policy.change == TextDocumentSyncKind::NONE {
            return;
        }
        let content_changes = if run.whole {
            vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: self.sent.to_string(),
            }]
        } else if run.changes.is_empty() {
            return;
        } else {
            run.changes
        };
        self.version += 1;
        out.push_back(Note::Change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: self.uri.clone(),
                version: self.version,
            },
            content_changes,
        }));
    }
}
