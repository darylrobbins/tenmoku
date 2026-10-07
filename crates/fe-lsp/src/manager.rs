//! [`LspManager`]: starts and stops servers, and carries requests to them.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use fe_buffer::{BufferId, Subscription, Version};
use fe_text::Text;
use futures::channel::{mpsc, oneshot};
use futures::future::FutureExt;
use lsp_types::request::Request;
use lsp_types::{ServerCapabilities, Url};

use crate::clock::{Clock, SystemClock};
use crate::document::{Admitted, Attached, Opening, Subscribed, language_id};
use crate::encoding::PositionEncoding;
use crate::event::{LspError, LspEvent, NOT_OPEN, ServerId};
use crate::identity::{Identities, IncarnationId, SessionId};
use crate::launcher::{ServerLauncher, ServerSpec};
use crate::server::{Admit, Command, Issue, Server, Session, Sink, send_request};

/// The default [`LspManager::max_file_bytes`]: 50 MB, the size past which
/// fastedit highlights only the viewport.
pub const DEFAULT_MAX_FILE_BYTES: usize = 50 * 1024 * 1024;

/// Runs language servers, one thread each, and reports on them through an
/// event sink.
pub struct LspManager {
    launcher: Arc<dyn ServerLauncher>,
    clock: Arc<dyn Clock>,
    sink: Sink,
    servers: HashMap<ServerId, Handle>,
    /// The server each open document is synced to, and its subscription,
    /// shared with that server's thread.
    documents: HashMap<BufferId, (ServerId, Subscribed)>,
    /// Which identity of each open buffer is active on each server,
    /// shared with the servers' threads.
    identities: Identities,
    max_file_bytes: usize,
    next_id: u64,
}

struct Handle {
    name: String,
    root: PathBuf,
    commands: mpsc::UnboundedSender<Command>,
    stop: oneshot::Sender<()>,
    session: Arc<Mutex<Session>>,
    thread: JoinHandle<()>,
}

impl LspManager {
    /// `sink` gets every [`LspEvent`], on the servers' threads.
    pub fn new(
        launcher: Arc<dyn ServerLauncher>,
        sink: impl FnMut(LspEvent) + Send + 'static,
    ) -> Self {
        Self::with_clock(launcher, Arc::new(SystemClock), sink)
    }

    /// As [`new`](Self::new), with every timeout measured on `clock`.
    pub fn with_clock(
        launcher: Arc<dyn ServerLauncher>,
        clock: Arc<dyn Clock>,
        sink: impl FnMut(LspEvent) + Send + 'static,
    ) -> Self {
        Self {
            launcher,
            clock,
            sink: Arc::new(Mutex::new(sink)),
            servers: HashMap::new(),
            documents: HashMap::new(),
            identities: Identities::default(),
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            next_id: 0,
        }
    }

    /// The largest text, in bytes, a document may have and be synced
    /// (§4.1): one over it isn't opened on its server, and one that grows
    /// past it is closed there until it's back under. Requests on it
    /// answer `Unavailable("too large for the language server")`.
    /// [`DEFAULT_MAX_FILE_BYTES`] unless set.
    pub fn max_file_bytes(&self) -> usize {
        self.max_file_bytes
    }

    /// Sets [`max_file_bytes`](Self::max_file_bytes) for documents opened
    /// from now on.
    pub fn set_max_file_bytes(&mut self, bytes: usize) {
        self.max_file_bytes = bytes;
    }

    /// Starts `spec`'s server for the workspace at `root`, an absolute
    /// path, on a thread of its own, unless one with the same name is
    /// already running there. The launcher runs on that thread, and the
    /// sink hears how the start went. Errs only if the thread can't be
    /// made.
    pub fn start(&mut self, spec: ServerSpec, root: &Path) -> io::Result<ServerId> {
        self.servers
            .retain(|_, handle| !handle.commands.is_closed() && !handle.thread.is_finished());
        let (servers, identities) = (&self.servers, &self.identities);
        self.documents.retain(|&buffer, (server, _)| {
            let live = servers.contains_key(server);
            if !live {
                identities.close(buffer);
            }
            live
        });
        if let Some((&id, _)) = self
            .servers
            .iter()
            .find(|(_, handle)| handle.name == spec.name && handle.root == root)
        {
            return Ok(id);
        }
        let id = ServerId(self.next_id);
        self.next_id += 1;
        let (commands, receiver) = mpsc::unbounded();
        let (stop, stop_requested) = oneshot::channel();
        let session = Arc::new(Mutex::new(Session::default()));
        let name = spec.name.clone();
        let thread = Server {
            id,
            spec,
            root: root.to_owned(),
            launcher: self.launcher.clone(),
            clock: self.clock.clone(),
            sink: self.sink.clone(),
            session: session.clone(),
            session_id: self.identities.new_session(),
            identities: self.identities.clone(),
            commands: receiver,
            stop: stop_requested,
        }
        .spawn()?;
        self.servers.insert(
            id,
            Handle {
                name,
                root: root.to_owned(),
                commands,
                stop,
                session,
                thread,
            },
        );
        Ok(id)
    }

    /// Shuts `server` down (§3.4) on its own thread; the sink hears
    /// `Stopped(None)` when it has gone. Requests still waiting get
    /// [`LspError::ServerDown`].
    pub fn stop(&mut self, server: ServerId) {
        if let Some(handle) = self.servers.remove(&server) {
            let _ = handle.stop.send(());
        }
        // Its documents go with it.
        let identities = &self.identities;
        self.documents.retain(|&buffer, (synced_to, _)| {
            let kept = *synced_to != server;
            if !kept {
                identities.close(buffer);
            }
            kept
        });
    }

    /// Sends `R` to `server`. A request made while the server starts is
    /// sent once it has initialized. The future may be awaited anywhere.
    pub fn request<R: Request>(
        &self,
        server: ServerId,
        params: R::Params,
    ) -> impl Future<Output = Result<R::Result, LspError>> + Send + 'static {
        self.request_with_id::<R>(server, params)
            .map(|answer| answer.and_then(|(_, result)| result))
    }

    /// As [`request`](Self::request), with the id the actor mirrored for
    /// the request.
    pub(crate) fn request_with_id<R: Request>(
        &self,
        server: ServerId,
        params: R::Params,
    ) -> impl Future<Output = Result<(i32, Result<R::Result, LspError>), LspError>> + Send + 'static
    {
        let (answer, answered) = oneshot::channel();
        let issue: Issue = Box::new(move |socket, id| {
            let reply = send_request::<R>(socket, params);
            Box::pin(async move {
                let _ = answer.send((id, reply.await.map_err(LspError::from_lsp)));
            })
        });
        if let Some(handle) = self.servers.get(&server) {
            // A server that has gone drops the request, and with it `answer`.
            let _ = handle.commands.unbounded_send(Command::Request(issue));
        }
        answered.map(|answer| answer.map_err(|oneshot::Canceled| LspError::ServerDown))
    }

    /// Sends `R` about open buffer `buffer`, computed on its `version`
    /// (§4.1). The server's thread brings the document up to date first,
    /// then:
    ///
    /// - if the server now has `version`, builds the request's params with
    ///   `params` and sends it, answering with the reply and what it was
    ///   [`Admitted`] with, to decode the reply against;
    /// - if the buffer has moved on, answers
    ///   `Unavailable("buffer changed")`: the request isn't rebased;
    /// - if the document is over [`max_file_bytes`](Self::max_file_bytes),
    ///   `Unavailable("too large for the language server")`.
    ///
    /// The future may be awaited anywhere.
    pub fn document_request<R: Request>(
        &self,
        buffer: BufferId,
        version: Version,
        params: impl FnOnce(&Admitted) -> R::Params + Send + 'static,
    ) -> impl Future<Output = Result<(R::Result, Admitted), LspError>> + Send + 'static {
        let (answer, answered) = oneshot::channel();
        let admit: Admit = Box::new(move |admission| {
            let admitted = match admission {
                Ok(admitted) => admitted,
                Err(err) => {
                    let _ = answer.send(Err(err));
                    return None;
                }
            };
            Some(Box::new(move |socket, _| {
                let reply = send_request::<R>(socket, params(&admitted));
                Box::pin(async move {
                    let reply = reply.await.map_err(LspError::from_lsp);
                    let _ = answer.send(reply.map(|result| (result, admitted)));
                })
            }))
        });
        match self.documents.get(&buffer) {
            Some(&(server, _)) => self.send(
                server,
                Command::Admit {
                    buffer,
                    version,
                    admit,
                },
            ),
            None => {
                admit(Err(LspError::Unavailable(NOT_OPEN.into())));
            }
        }
        answered.map(|answer| answer.unwrap_or(Err(LspError::ServerDown)))
    }

    /// Starts syncing buffer `buffer` to `server` (§4.1): `didOpen` with
    /// `text`, the buffer's text at `version`, with the `languageId` for
    /// fe-syntax's `language` (§3.2). Its LSP version is 0, or, for a URI
    /// the server had open before in this session, the last one it was
    /// sent plus 1. A text over [`max_file_bytes`](Self::max_file_bytes)
    /// isn't opened until edits bring it under.
    ///
    /// The open activates a new identity ([`active_identity`]) before its
    /// `didOpen` is sent, unless the buffer was closed meanwhile.
    ///
    /// [`active_identity`]: Self::active_identity
    ///
    /// From then on `server`'s thread drains `subscription`, which must
    /// have been made no later than `version` (edits up to it, and saves
    /// of earlier versions, are skipped): on [`changed`](Self::changed), and every 250 ms in case a
    /// nudge was missed. Each run of edits becomes a `didChange` and each
    /// save a `didSave`, in the buffer's order. Sent while the server
    /// starts, it goes once the server has initialized.
    ///
    /// A buffer already open is closed first. Errs if `path` isn't
    /// absolute, or `server` isn't running.
    #[allow(clippy::too_many_arguments)] // Each is one thing the document needs.
    pub fn open(
        &mut self,
        server: ServerId,
        buffer: BufferId,
        text: Text,
        version: Version,
        subscription: Subscription,
        path: &Path,
        language: &str,
    ) -> Result<(), LspError> {
        let uri = Url::from_file_path(path).map_err(|()| {
            LspError::Unavailable(format!("{} isn't an absolute path", path.display()))
        })?;
        if !self.servers.contains_key(&server) {
            return Err(LspError::ServerDown);
        }
        // Closed before the open is sent, so a server dropping the open
        // drops nothing the manager still tracks.
        self.close(buffer);
        let subscription = Subscribed::new(subscription);
        let opening = Opening {
            buffer,
            text,
            version,
            subscription: Attached(subscription.clone()),
            uri,
            language_id: language_id(language, path),
            generation: self.identities.open(buffer),
            max_bytes: self.max_file_bytes,
        };
        if self.servers[&server]
            .commands
            .unbounded_send(Command::Open(Box::new(opening)))
            .is_err()
        {
            self.identities.close(buffer);
            return Err(LspError::ServerDown);
        }
        self.documents.insert(buffer, (server, subscription));
        Ok(())
    }

    /// Stops syncing `buffer`: what it did until now that its server
    /// hasn't been sent yet is sent, in order, then `didClose`. The
    /// subscription is dropped here, on the calling thread, so edits made
    /// after this returns send nothing, and its identity is retired here
    /// too, before `didClose` is sent.
    pub fn close(&mut self, buffer: BufferId) {
        if let Some((server, subscribed)) = self.documents.remove(&buffer) {
            self.identities.close(buffer);
            subscribed.detach();
            self.send(server, Command::Close(buffer));
        }
    }

    /// The `(session, incarnation)` of `buffer` that `server` has open now
    /// (§4.1), if any. It's set before the incarnation's `didOpen` is
    /// sent, and cleared when [`close`](Self::close) returns, when the
    /// document outgrows [`max_file_bytes`](Self::max_file_bytes), and
    /// when the session ends, before its end is reported.
    pub fn active_identity(
        &self,
        buffer: BufferId,
        server: ServerId,
    ) -> Option<(SessionId, IncarnationId)> {
        self.identities.active(buffer, server)
    }

    /// The table of identities, shared with the servers' threads.
    #[cfg(test)]
    pub(crate) fn identities(&self) -> &Identities {
        &self.identities
    }

    /// The LSP and buffer versions in `buffer`'s ring on its server,
    /// oldest first, once the commands before this one are done.
    #[cfg(test)]
    pub(crate) fn ring(
        &self,
        buffer: BufferId,
    ) -> impl Future<Output = Vec<(i32, Version)>> + use<> {
        let (answer, answered) = oneshot::channel();
        if let Some(&(server, _)) = self.documents.get(&buffer) {
            self.send(server, Command::Ring(buffer, answer));
        }
        answered.map(Result::unwrap_or_default)
    }

    /// Whether `buffer` still queues events for fe-lsp.
    #[cfg(test)]
    pub(crate) fn is_subscribed(&self, buffer: BufferId) -> bool {
        self.documents
            .get(&buffer)
            .is_some_and(|(_, subscribed)| subscribed.is_attached())
    }

    /// Says `buffer` emitted events (edits or a save), so its server's
    /// thread drains them now rather than at the next backstop tick. Cheap:
    /// call it once a frame for each buffer that changed.
    pub fn changed(&self, buffer: BufferId) {
        if let Some(&(server, _)) = self.documents.get(&buffer) {
            self.send(server, Command::Changed(buffer));
        }
    }

    fn send(&self, server: ServerId, command: Command) {
        if let Some(handle) = self.servers.get(&server) {
            // A server that has gone has no documents to sync.
            let _ = handle.commands.unbounded_send(command);
        }
    }

    /// The position encoding `server` chose, once it has initialized.
    pub fn position_encoding(&self, server: ServerId) -> Option<PositionEncoding> {
        self.session(server, |session| session.encoding)
    }

    /// What `server` said it can do, once it has initialized.
    pub fn capabilities(&self, server: ServerId) -> Option<ServerCapabilities> {
        self.session(server, |session| session.capabilities.clone())
    }

    fn session<T>(&self, server: ServerId, read: impl FnOnce(&Session) -> Option<T>) -> Option<T> {
        let handle = self.servers.get(&server)?;
        read(
            &handle
                .session
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }

    /// Whether `server`'s thread has ended.
    #[cfg(test)]
    pub(crate) fn thread_finished(&self, server: ServerId) -> bool {
        self.servers
            .get(&server)
            .is_none_or(|handle| handle.thread.is_finished())
    }
}

impl Drop for LspManager {
    /// Asks every server to shut down, without waiting for them.
    fn drop(&mut self) {
        for (_, handle) in self.servers.drain() {
            let _ = handle.stop.send(());
        }
    }
}
