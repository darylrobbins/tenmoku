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
//!
//! Each incarnation (identity.rs) keeps a ring of the last [`RING`]
//! versions sent, to admit requests and decode replies against. Its LSP
//! versions go on across reopens within a session: a reopen's `didOpen`
//! uses the URI's last version plus 1. A document whose text is over its
//! size limit isn't open on the server: it's closed when an edit takes it
//! over, and opened again once one brings it back under.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use fe_buffer::{BufferEvent, BufferId, Edit, Subscription, Version};
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
use crate::event::{BUFFER_CHANGED, LspError, NOT_OPEN, ServerId, TOO_LARGE};
use crate::identity::{Generation, Identities, IncarnationId, Point, SessionId};
use crate::server::Outgoing;

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
    /// A save a catch-up drained that applies to the version it caught up
    /// to, for the `didOpen` that follows to send after itself.
    caught_up_save: Option<Version>,
}

impl Subscribed {
    pub(crate) fn new(subscription: Subscription) -> Self {
        Self(Arc::new(Mutex::new(Feed {
            subscription: Some(subscription),
            detached: Vec::new(),
            caught_up_save: None,
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

    /// Remembers `save` as the save of the version a catch-up reached, or
    /// forgets the remembered one for a later edit.
    fn set_caught_up_save(&self, save: Option<Version>) {
        self.lock().caught_up_save = save;
    }

    fn take_caught_up_save(&self) -> Option<Version> {
        self.lock().caught_up_save.take()
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
    /// The method and document, as `didClose file:///a`.
    #[cfg(test)]
    pub(crate) fn describe(&self) -> String {
        let (method, uri) = match self {
            Self::Open(p) => ("didOpen", &p.text_document.uri),
            Self::Change(p) => ("didChange", &p.text_document.uri),
            Self::Save(p) => ("didSave", &p.text_document.uri),
            Self::Close(p) => ("didClose", &p.text_document.uri),
        };
        format!("{method} {uri}")
    }

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
    /// The open generation the manager issued it under.
    pub(crate) generation: Generation,
    /// A text longer than this, in bytes, isn't synced.
    pub(crate) max_bytes: usize,
}

impl Opening {
    /// Brings the opening up to the buffer's latest edit, so a document
    /// waiting for a server doesn't keep a queue of events.
    pub(crate) fn catch_up(&mut self) {
        let mut save = self.subscription.0.take_caught_up_save();
        for event in self.subscription.0.take_events() {
            match event {
                BufferEvent::Edited { to, text, .. } if to > self.version => {
                    self.text = text;
                    self.version = to;
                    save = None;
                }
                BufferEvent::Saved { version } if version >= self.version => {
                    save = Some(version);
                }
                _ => {}
            }
        }
        self.subscription.0.set_caught_up_save(save);
    }
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

/// How many versions sent each open document keeps (§4.1).
pub(crate) const RING: usize = 32;

/// One version of a document as sent to its server: an entry of the
/// document's ring.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SentVersion {
    /// The `version` the server was told.
    pub lsp_version: i32,
    /// The buffer version it is.
    pub version: Version,
    /// The text the server has at it.
    pub text: Text,
    /// The batches that took the previous version sent to this one, in
    /// order: none for a `didOpen`, one or more for a `didChange`.
    pub edits: Vec<Arc<[Edit]>>,
}

/// A document open on a server, at the version the server has.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SentDocument {
    pub buffer: BufferId,
    pub uri: Url,
    /// The incarnation the server has open, to check a reply against
    /// [`LspManager::active_identity`](crate::LspManager::active_identity)
    /// when it lands.
    pub identity: (SessionId, IncarnationId),
    pub sent: SentVersion,
}

/// What a request on a document went out with (§4.1), to decode its reply
/// against.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Admitted {
    /// The session's position encoding.
    pub encoding: PositionEncoding,
    /// The document, at the version the request named.
    pub document: SentDocument,
    /// Every other document the server has open, at its current version.
    pub others: Vec<SentDocument>,
}

/// What a document needs from its server's actor to sync.
pub(crate) struct Link {
    pub(crate) policy: SyncPolicy,
    pub(crate) encoding: PositionEncoding,
    pub(crate) identities: Identities,
    pub(crate) server: ServerId,
    pub(crate) session: SessionId,
    /// The last LSP version sent for each URI closed in this session, so
    /// a reopen goes on from it.
    pub(crate) versions: HashMap<Url, i32>,
    /// Document notifications and admitted requests waiting for room in
    /// async-lsp's queue, sent ahead of anything else.
    pub(crate) outbox: VecDeque<Outgoing>,
}

impl Link {
    fn send(&mut self, note: Note) {
        self.outbox.push_back(Outgoing::Note(note));
    }
}

/// One open document on one server.
pub(crate) struct Document {
    buffer: BufferId,
    uri: Url,
    language_id: String,
    subscription: Attached,
    generation: Generation,
    max_bytes: usize,
    /// The buffer's text as of the last batch drained.
    text: Text,
    /// The buffer version `text` is.
    at: Version,
    /// While the server has the document.
    synced: Option<Synced>,
}

/// An incarnation: the document from its `didOpen` to its `didClose`.
struct Synced {
    identity: (SessionId, IncarnationId),
    /// The last [`RING`] versions sent, oldest first. Never empty: the
    /// newest is what the server has.
    ring: VecDeque<SentVersion>,
}

impl Synced {
    fn newest(&self) -> &SentVersion {
        self.ring
            .back()
            .expect("a synced document's ring has its didOpen")
    }

    fn push(&mut self, sent: SentVersion) {
        if self.ring.len() == RING {
            self.ring.pop_front();
        }
        self.ring.push_back(sent);
    }
}

/// The `didChange` being built from a run of batches.
#[derive(Default)]
struct Run {
    batches: Vec<Arc<[Edit]>>,
    /// Send the whole text rather than `changes`.
    whole: bool,
    changes: Vec<TextDocumentContentChangeEvent>,
}

impl Document {
    /// Starts syncing `opening`: activates a new incarnation, then queues
    /// its `didOpen` with the LF text, at the URI's last version in this
    /// session plus 1, or 0. A document over its size limit isn't opened
    /// until it's back under.
    ///
    /// `None` if the open is stale: the manager closed the buffer since it
    /// issued the open. It then inserts nothing and sends nothing.
    pub(crate) fn open(opening: Opening, link: &mut Link) -> Option<Self> {
        let mut document = Self {
            buffer: opening.buffer,
            uri: opening.uri,
            language_id: opening.language_id,
            subscription: opening.subscription,
            generation: opening.generation,
            max_bytes: opening.max_bytes,
            text: opening.text,
            at: opening.version,
            synced: None,
        };
        let saved = document.subscription.0.take_caught_up_save();
        link.identities.pause(Point::Activate);
        if !link.policy.open_close || document.oversized() {
            // Nothing to send, but its events still drain.
            return link
                .identities
                .is_current(document.buffer, document.generation)
                .then_some(document);
        }
        if !document.activate(link) {
            return None;
        }
        // A save `catch_up` drained, of the version the `didOpen` carries.
        if saved == Some(document.at) {
            document.send_save(document.at, link);
        }
        Some(document)
    }

    /// Queues `didSave` for `version`, if the server wants saves.
    fn send_save(&mut self, version: Version, link: &mut Link) {
        let Some(include_text) = link.policy.save else {
            return;
        };
        let text = if !include_text {
            None
        } else if version == self.at {
            Some(self.text.to_string())
        } else {
            // Can't happen: events arrive in order, and every batch up to
            // `version` came before.
            log::warn!(
                "{}: saved {version:?}, but only {:?} was seen; didSave carries no text",
                self.uri,
                self.at,
            );
            None
        };
        link.send(Note::Save(DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: self.uri.clone(),
            },
            text,
        }));
    }

    fn oversized(&self) -> bool {
        self.text.len() > self.max_bytes
    }

    /// The document as an opening for another server, at the text drained
    /// so far: the subscription keeps what the buffer did since. Its
    /// session is ending, which retires its identity.
    pub(crate) fn into_opening(self) -> Opening {
        Opening {
            buffer: self.buffer,
            text: self.text,
            version: self.at,
            subscription: self.subscription,
            uri: self.uri,
            language_id: self.language_id,
            generation: self.generation,
            max_bytes: self.max_bytes,
        }
    }

    /// Activates a new incarnation and queues its `didOpen`. False if the
    /// open is stale.
    fn activate(&mut self, link: &mut Link) -> bool {
        let Some(incarnation) =
            link.identities
                .activate(self.buffer, self.generation, link.server, link.session)
        else {
            return false;
        };
        let version = link.versions.get(&self.uri).map_or(0, |last| last + 1);
        link.send(Note::Open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: self.uri.clone(),
                language_id: self.language_id.clone(),
                version,
                text: self.text.to_string(),
            },
        }));
        self.synced = Some(Synced {
            identity: (link.session, incarnation),
            ring: VecDeque::from([SentVersion {
                lsp_version: version,
                version: self.at,
                text: self.text.clone(),
                edits: Vec::new(),
            }]),
        });
        true
    }

    /// Queues `didClose`, after a last drain of what the buffer did until
    /// the manager detached it, so a save just before closing still
    /// reaches the server. The manager has retired its identity already.
    pub(crate) fn close(mut self, link: &mut Link) {
        self.drain(link);
        self.deactivate(link);
    }

    /// Queues `didClose`, if the server has the document, and keeps its
    /// last version for a reopen.
    fn deactivate(&mut self, link: &mut Link) {
        let Some(synced) = self.synced.take() else {
            return;
        };
        // The highest, should two buffers have had the URI open.
        let last = synced.newest().lsp_version;
        link.versions
            .entry(self.uri.clone())
            .and_modify(|version| *version = last.max(*version))
            .or_insert(last);
        link.send(Note::Close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: self.uri.clone(),
            },
        }));
    }

    /// Closes a document that grew past its size limit: retires its own
    /// identity, unless the manager has closed the buffer since, then
    /// `didClose`.
    fn close_oversized(&mut self, link: &mut Link) {
        let Some(synced) = &self.synced else { return };
        link.identities.pause(Point::Retire);
        link.identities
            .retire(self.buffer, self.generation, link.server, synced.identity);
        self.deactivate(link);
    }

    /// Takes the subscription's events and queues what they become. A
    /// batch that takes the document over its size limit closes it, and
    /// one that brings it back under opens it again.
    pub(crate) fn drain(&mut self, link: &mut Link) {
        let events = self.subscription.0.take_events();
        if !link.policy.open_close {
            // Nothing is sent, but the queue mustn't grow either, and the
            // size limit still goes by the latest text.
            for event in events {
                if let BufferEvent::Edited { to, text, .. } = event
                    && to > self.at
                {
                    self.text = text;
                    self.at = to;
                }
            }
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
                    // A gap (`from` isn't what was drained) means batches
                    // went missing, so only the whole text is right.
                    let gap = from != self.at;
                    if self.synced.is_some() && text.len() > self.max_bytes {
                        // The run so far goes as it was, never with the
                        // oversized text, and then the document closes.
                        self.flush(std::mem::take(&mut run), link);
                        self.close_oversized(link);
                    }
                    let before = std::mem::replace(&mut self.text, text);
                    self.at = to;
                    if self.synced.is_none() {
                        if !self.oversized() {
                            self.activate(link);
                        }
                        continue;
                    }
                    run.whole |= whole || gap || link.policy.change == TextDocumentSyncKind::FULL;
                    if run.whole {
                        run.changes.clear();
                    } else {
                        run.changes.extend(edits.iter().rev().map(|edit| {
                            TextDocumentContentChangeEvent {
                                range: Some(Range::new(
                                    to_lsp(&before, edit.old.start, link.encoding),
                                    to_lsp(&before, edit.old.end, link.encoding),
                                )),
                                range_length: None,
                                text: self.text.slice_to_string(edit.new.clone()),
                            }
                        }));
                    }
                    run.batches.push(edits);
                }
                BufferEvent::Saved { version } => {
                    if version < self.at {
                        // Saved before the version the document opened with.
                        continue;
                    }
                    self.flush(std::mem::take(&mut run), link);
                    if self.synced.is_none() {
                        continue;
                    }
                    self.send_save(version, link);
                }
            }
        }
        self.flush(run, link);
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
    /// is told of, and keeps the version it makes in the ring.
    fn flush(&mut self, run: Run, link: &mut Link) {
        let Some(synced) = &mut self.synced else {
            return;
        };
        if run.batches.is_empty() || link.policy.change == TextDocumentSyncKind::NONE {
            return;
        }
        let content_changes = if run.whole {
            vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: self.text.to_string(),
            }]
        } else if run.changes.is_empty() {
            return;
        } else {
            run.changes
        };
        let version = synced.newest().lsp_version + 1;
        synced.push(SentVersion {
            lsp_version: version,
            version: self.at,
            text: self.text.clone(),
            edits: run.batches,
        });
        link.send(Note::Change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: self.uri.clone(),
                version,
            },
            content_changes,
        }));
    }

    /// Admits a request computed on buffer version `version`, once drained
    /// (§4.1): the version the server has, if that's `version`.
    pub(crate) fn admit(&self, version: Version) -> Result<SentDocument, LspError> {
        if self.oversized() {
            return Err(LspError::Unavailable(TOO_LARGE.into()));
        }
        let Some(synced) = &self.synced else {
            return Err(LspError::Unavailable(NOT_OPEN.into()));
        };
        let sent = synced.newest();
        if sent.version == version {
            Ok(SentDocument {
                buffer: self.buffer,
                uri: self.uri.clone(),
                identity: synced.identity,
                sent: sent.clone(),
            })
        } else {
            // The buffer moved on while the request waited: it isn't
            // rebased. (A version newer than the server has can only be
            // one the server isn't told of: a server syncing no changes.)
            if sent.version < version {
                log::debug!(
                    "{}: a request on {version:?}, but the server has {:?}",
                    self.uri,
                    sent.version
                );
            }
            Err(LspError::Unavailable(BUFFER_CHANGED.into()))
        }
    }

    /// The document at the version the server has, if it has it.
    pub(crate) fn current(&self) -> Option<SentDocument> {
        self.synced.as_ref().map(|synced| SentDocument {
            buffer: self.buffer,
            uri: self.uri.clone(),
            identity: synced.identity,
            sent: synced.newest().clone(),
        })
    }

    /// The LSP and buffer versions in the ring, oldest first.
    #[cfg(test)]
    pub(crate) fn ring(&self) -> Vec<(i32, Version)> {
        self.synced.as_ref().map_or_else(Vec::new, |synced| {
            synced
                .ring
                .iter()
                .map(|sent| (sent.lsp_version, sent.version))
                .collect()
        })
    }
}
