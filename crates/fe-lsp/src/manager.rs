//! [`LspManager`]: starts and stops servers, and carries requests to them.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use futures::channel::{mpsc, oneshot};
use futures::future::FutureExt;
use lsp_types::ServerCapabilities;
use lsp_types::request::Request;

use crate::clock::{Clock, SystemClock};
use crate::event::{LspError, LspEvent, PositionEncoding, ServerId};
use crate::launcher::{ServerLauncher, ServerSpec};
use crate::server::{Command, Issue, Server, Session, Sink, send_request};

/// Runs language servers, one thread each, and reports on them through an
/// event sink.
pub struct LspManager {
    launcher: Arc<dyn ServerLauncher>,
    clock: Arc<dyn Clock>,
    sink: Sink,
    servers: HashMap<ServerId, Handle>,
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
            next_id: 0,
        }
    }

    /// Starts `spec`'s server for the workspace at `root`, an absolute
    /// path, on a thread of its own, unless one with the same name is
    /// already running there. The launcher runs on that thread, and the
    /// sink hears how the start went. Errs only if the thread can't be
    /// made.
    pub fn start(&mut self, spec: ServerSpec, root: &Path) -> io::Result<ServerId> {
        self.servers
            .retain(|_, handle| !handle.commands.is_closed() && !handle.thread.is_finished());
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
