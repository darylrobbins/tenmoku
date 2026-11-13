//! The ACP v1 client: the connection, the session methods, and the handler
//! that hands out permission requests (decision record § 4).
//!
//! [`connect`] builds the connection over any transport and returns three
//! things:
//!
//! - an [`AcpClient`], a cheap handle for the requests;
//! - a stream of [`Incoming`] messages: session updates as [`AcpEvent`]s and
//!   [`PermissionRequest`]s, in the order the agent sent them;
//! - the connection future, which the caller drives (the workbench, on
//!   GPUI's background executor). It ends when the transport closes, or when
//!   [`AcpClient::shutdown`] is called or every handle is dropped.
//!
//! **Ordering.** The crate runs incoming handlers one at a time on its
//! dispatch loop, and awaiting a request inside one deadlocks
//! (`concepts/ordering.rs`). So neither handler here awaits anything: an
//! update is pushed onto the [`Incoming`] stream, and a permission request
//! is pushed with its responder held here, to be answered later through
//! [`PermissionRequest::select`]. The same loop routes responses, so when a
//! request such as [`AcpClient::prompt`] returns, every update the agent sent
//! before its response is already on the stream.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::v1::schema::{
    CancelNotification, ClientCapabilities, ContentBlock, FileSystemCapabilities, Implementation,
    InitializeRequest, InitializeResponse, LoadSessionRequest, LoadSessionResponse, Meta,
    NewSessionRequest, NewSessionResponse, PermissionOption, PermissionOptionId, PromptRequest,
    PromptResponse, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionConfigId, SessionConfigOption, SessionConfigOptionValue,
    SessionId, SessionModeId, SessionNotification, SetSessionConfigOptionRequest,
    SetSessionModeRequest, ToolCallUpdate,
};
use agent_client_protocol::v1::{ConnectionTo, Responder};
use agent_client_protocol::{Agent, Client, ConnectTo, Error, JsonRpcRequest};
use futures::channel::{mpsc, oneshot};
use futures::future::{self, FutureExt as _, Shared};

use crate::AcpEvent;

/// What fe-acp's requests return.
pub type Result<T> = std::result::Result<T, Error>;

/// The name the client gives in `initialize`'s `clientInfo`.
pub const CLIENT_NAME: &str = "tenmoku";

/// A message from the agent, in the order it was sent.
#[derive(Debug)]
pub enum Incoming {
    /// A session update the panel uses. Updates it ignores (§ 5) are dropped
    /// with a debug log and never appear here.
    Update {
        /// The session the update is for.
        session_id: SessionId,
        /// The update.
        event: AcpEvent,
    },
    /// The agent asks to run a tool. Answer it with
    /// [`PermissionRequest::select`]; dropping it answers `Cancelled`.
    Permission(PermissionRequest),
}

/// The capabilities fe-acp declares in `initialize` (§ 4): no file system
/// methods, no terminal, and no auth capability, so the adapter offers no
/// login method.
#[must_use]
pub fn client_capabilities() -> ClientCapabilities {
    ClientCapabilities::new()
        .fs(FileSystemCapabilities::new()
            .read_text_file(false)
            .write_text_file(false))
        .terminal(false)
}

/// Connects to an agent over `transport`.
///
/// Returns the client handle, the stream of [`Incoming`] messages, and the
/// connection future, which must be driven for anything to happen. The
/// future ends with `Ok` when the transport closes or the client shuts down,
/// and the stream ends after it.
///
/// No request is sent: call [`AcpClient::initialize`] first.
pub fn connect(
    transport: impl ConnectTo<Client> + 'static,
) -> (
    AcpClient,
    mpsc::UnboundedReceiver<Incoming>,
    impl Future<Output = Result<()>> + Send + 'static,
) {
    let (incoming_tx, incoming_rx) = mpsc::unbounded();
    let (connection_tx, connection_rx) = oneshot::channel();
    let (close_tx, close_rx) = oneshot::channel::<()>();
    let pending = Pending::default();

    let updates_tx = incoming_tx.clone();
    let permissions_tx = incoming_tx;
    let permissions = pending.clone();
    let driver = Client
        .v1()
        .name("fe-acp")
        .on_receive_notification(
            async move |notification: SessionNotification, _connection| {
                let session_id = notification.session_id;
                match AcpEvent::from(notification.update) {
                    AcpEvent::Ignored(kind) => {
                        log::debug!("fe-acp: ignoring the {kind} update for session {session_id}");
                    }
                    event => {
                        // The receiver is gone only when nobody listens any
                        // more; the update is then of no use.
                        let _ = updates_tx.unbounded_send(Incoming::Update { session_id, event });
                    }
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest,
                        responder: Responder<RequestPermissionResponse>,
                        _connection| {
                permissions.receive(request, responder, &permissions_tx);
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, async move |connection: ConnectionTo<Agent>| {
            let _ = connection_tx.send(connection.clone());
            let closed = connection.incoming_closed();
            futures::pin_mut!(closed);
            // Either the client shuts down (or every handle is gone, which
            // drops the sender), or the agent's side closes.
            future::select(close_rx, closed).await;
            Ok(())
        });

    let client = AcpClient {
        connection: connection_rx.shared(),
        pending,
        close: Arc::new(Mutex::new(Some(close_tx))),
    };
    (client, incoming_rx, driver)
}

/// A handle to a connected agent. Clones share the connection.
///
/// Each request waits for its response. Call them from a task, never from
/// inside a handler on the connection's dispatch loop.
#[derive(Clone)]
pub struct AcpClient {
    connection: Shared<oneshot::Receiver<ConnectionTo<Agent>>>,
    pending: Pending,
    close: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

impl std::fmt::Debug for AcpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpClient").finish_non_exhaustive()
    }
}

impl AcpClient {
    /// Sends `initialize` for protocol v1 with [`client_capabilities`], and
    /// returns the agent's answer: its info (the version floor is checked by
    /// the caller), capabilities and auth methods.
    ///
    /// Fails if the agent answers with any protocol version but v1.
    pub async fn initialize(&self) -> Result<InitializeResponse> {
        let request = InitializeRequest::new(ProtocolVersion::V1)
            .client_capabilities(client_capabilities())
            .client_info(Implementation::new(CLIENT_NAME, env!("CARGO_PKG_VERSION")));
        let response = self.request(request).await?;
        if response.protocol_version != ProtocolVersion::V1 {
            return Err(Error::internal_error().data(format!(
                "the agent answered protocol version {}; fe-acp speaks only version 1",
                response.protocol_version
            )));
        }
        Ok(response)
    }

    /// Sends `session/new` for `cwd`, with no MCP servers and the given
    /// `_meta` (Claude Agent's security profile goes there).
    pub async fn new_session(
        &self,
        cwd: impl Into<PathBuf>,
        meta: Option<Meta>,
    ) -> Result<NewSessionResponse> {
        self.request(NewSessionRequest::new(cwd).meta(meta)).await
    }

    /// Sends `session/load` for `session_id` in `cwd`, with no MCP servers
    /// and the given `_meta`. The agent replays the session's history as
    /// updates, all on the [`Incoming`] stream by the time this returns.
    pub async fn load_session(
        &self,
        session_id: impl Into<SessionId>,
        cwd: impl Into<PathBuf>,
        meta: Option<Meta>,
    ) -> Result<LoadSessionResponse> {
        self.request(LoadSessionRequest::new(session_id, cwd).meta(meta))
            .await
    }

    /// Sends `session/prompt` and waits for the turn to end: the response's
    /// stop reason ends it. The turn's updates and permission requests
    /// arrive on the [`Incoming`] stream meanwhile.
    ///
    /// The turn runs from the first poll until the response arrives, fails,
    /// or the future is dropped.
    pub async fn prompt(
        &self,
        session_id: impl Into<SessionId>,
        prompt: Vec<ContentBlock>,
    ) -> Result<PromptResponse> {
        let session_id = session_id.into();
        let _turn = self.pending.start_turn(&session_id);
        self.request(PromptRequest::new(session_id, prompt)).await
    }

    /// Stops the session's running turns: sends `session/cancel`, then
    /// answers every pending permission request for the session
    /// `Cancelled`, as the protocol requires.
    ///
    /// Until every turn running at the cancel has ended (its prompt's
    /// response arrived, with the `cancelled` stop reason), further requests
    /// for the session are answered `Cancelled` at once and never reach the
    /// stream, even while a later prompt runs: the agent queues prompts, and
    /// its requests don't say which turn they belong to.
    ///
    /// The notification and the answers are queued under the lock that every
    /// other answer takes, so on the wire an allow is either before the
    /// `session/cancel` or not sent at all, and an automatic `Cancelled` is
    /// always after it.
    pub async fn cancel(&self, session_id: impl Into<SessionId>) -> Result<()> {
        let session_id = session_id.into();
        let connection = self.connection().await?;
        self.pending.cancel_session(&session_id, || {
            connection.send_notification(CancelNotification::new(session_id.clone()))
        })
    }

    /// Sends `session/set_config_option` and returns the session's
    /// configuration options as they now are.
    pub async fn set_config_option(
        &self,
        session_id: impl Into<SessionId>,
        config_id: impl Into<SessionConfigId>,
        value: impl Into<SessionConfigOptionValue>,
    ) -> Result<Vec<SessionConfigOption>> {
        let request = SetSessionConfigOptionRequest::new(session_id, config_id, value);
        Ok(self.request(request).await?.config_options)
    }

    /// Sends `session/set_mode`.
    pub async fn set_mode(
        &self,
        session_id: impl Into<SessionId>,
        mode_id: impl Into<SessionModeId>,
    ) -> Result<()> {
        self.request(SetSessionModeRequest::new(session_id, mode_id))
            .await?;
        Ok(())
    }

    /// Ends the connection: the connection future finishes, requests still
    /// waiting fail, and the [`Incoming`] stream ends. Dropping every handle
    /// does the same.
    pub fn shutdown(&self) {
        if let Some(close) = lock(&self.close).take() {
            let _ = close.send(());
        }
    }

    async fn connection(&self) -> Result<ConnectionTo<Agent>> {
        self.connection.clone().await.map_err(|_| {
            Error::internal_error().data("the connection to the agent ended before it started")
        })
    }

    async fn request<R: JsonRpcRequest>(&self, request: R) -> Result<R::Response> {
        let connection = self.connection().await?;
        connection.send_request(request).block_task().await
    }
}

/// A `session/request_permission` from the agent, waiting for an answer.
///
/// It is answered exactly once: by [`select`](Self::select), by
/// [`cancel`](Self::cancel), by [`AcpClient::cancel`] for its session, or
/// `Cancelled` when it is dropped unanswered.
#[derive(Debug)]
pub struct PermissionRequest {
    id: u64,
    request: RequestPermissionRequest,
    pending: Pending,
}

impl PermissionRequest {
    /// The session asking.
    #[must_use]
    pub fn session_id(&self) -> &SessionId {
        &self.request.session_id
    }

    /// The tool call it asks about: its id, and whatever fields the agent
    /// filled in (title, kind, raw input, content).
    #[must_use]
    pub fn tool_call(&self) -> &ToolCallUpdate {
        &self.request.tool_call
    }

    /// The options offered, such as allow once, allow always and reject.
    #[must_use]
    pub fn options(&self) -> &[PermissionOption] {
        &self.request.options
    }

    /// The request as received.
    #[must_use]
    pub fn request(&self) -> &RequestPermissionRequest {
        &self.request
    }

    /// Whether it still waits for an answer: false once its session's turn
    /// was cancelled.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.pending.is_open(self.id)
    }

    /// Answers with the offered option `option_id`. Returns whether that
    /// answer was sent.
    ///
    /// It isn't when the request was already answered `Cancelled` by a
    /// cancel of its session. And an id the request didn't offer is never
    /// sent: the request is answered `Cancelled` instead, so an unknown id
    /// can never read as an allow.
    pub fn select(self, option_id: impl Into<PermissionOptionId>) -> bool {
        let option_id = option_id.into();
        if !self.options().iter().any(|o| o.option_id == option_id) {
            log::warn!(
                "fe-acp: {option_id} isn't an option of this permission request; cancelling it"
            );
            return false;
        }
        let outcome = RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id));
        self.pending.answer(self.id, outcome)
    }

    /// Answers `Cancelled`.
    pub fn cancel(self) {
        // Drop answers it.
    }
}

impl Drop for PermissionRequest {
    fn drop(&mut self) {
        self.pending
            .answer(self.id, RequestPermissionOutcome::Cancelled);
    }
}

/// Queues `outcome` as the answer. Callers hold the [`Pending`] lock, so
/// answers and `session/cancel` reach the outgoing queue in one order.
fn respond(
    responder: Responder<RequestPermissionResponse>,
    outcome: RequestPermissionOutcome,
) -> bool {
    match responder.respond(RequestPermissionResponse::new(outcome)) {
        Ok(()) => true,
        Err(err) => {
            log::debug!("fe-acp: answering a permission request failed: {err}");
            false
        }
    }
}

/// The permission requests waiting for an answer and the turns running,
/// shared by the handler, the client and the requests themselves.
///
/// Every answer and every `session/cancel` is queued while holding the lock
/// (queueing only pushes onto the connection's unbounded outgoing channel,
/// which keeps their order on the wire).
#[derive(Debug, Clone, Default)]
struct Pending(Arc<Mutex<PendingState>>);

#[derive(Default)]
struct PendingState {
    next_id: u64,
    open: Vec<Open>,
    turns: HashMap<SessionId, Turns>,
}

/// A session's turns, by id.
#[derive(Default)]
struct Turns {
    running: Vec<u64>,
    /// Turns that were running at a cancel and haven't ended. While any is
    /// left, the session's permission requests are refused.
    cancelled: Vec<u64>,
}

impl std::fmt::Debug for PendingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingState")
            .field("open", &self.open.len())
            .field("sessions", &self.turns.len())
            .finish()
    }
}

struct Open {
    id: u64,
    session_id: SessionId,
    responder: Responder<RequestPermissionResponse>,
}

/// A running turn; dropping it ends the turn.
struct Turn {
    id: u64,
    session_id: SessionId,
    pending: Pending,
}

impl Drop for Turn {
    fn drop(&mut self) {
        let mut state = lock(&self.pending.0);
        if let Some(turns) = state.turns.get_mut(&self.session_id) {
            turns.running.retain(|&id| id != self.id);
            turns.cancelled.retain(|&id| id != self.id);
            if turns.running.is_empty() && turns.cancelled.is_empty() {
                state.turns.remove(&self.session_id);
            }
        }
    }
}

impl Pending {
    /// The handler's half: holds the responder and hands the request out,
    /// or answers `Cancelled` at once while a cancelled turn runs.
    fn receive(
        &self,
        request: RequestPermissionRequest,
        responder: Responder<RequestPermissionResponse>,
        incoming: &mpsc::UnboundedSender<Incoming>,
    ) {
        let mut state = lock(&self.0);
        let refused = state
            .turns
            .get(&request.session_id)
            .is_some_and(|turns| !turns.cancelled.is_empty());
        if refused {
            respond(responder, RequestPermissionOutcome::Cancelled);
            return;
        }
        let id = state.next_id;
        state.next_id += 1;
        state.open.push(Open {
            id,
            session_id: request.session_id.clone(),
            responder,
        });
        drop(state);
        let request = PermissionRequest {
            id,
            request,
            pending: self.clone(),
        };
        // With nobody listening the request comes back, and dropping it
        // answers `Cancelled`.
        let _ = incoming.unbounded_send(Incoming::Permission(request));
    }

    /// Answers the open request `id` with `outcome`. False when it was
    /// already answered, or the answer couldn't be queued.
    fn answer(&self, id: u64, outcome: RequestPermissionOutcome) -> bool {
        let mut state = lock(&self.0);
        let Some(index) = state.open.iter().position(|open| open.id == id) else {
            return false;
        };
        let open = state.open.remove(index);
        respond(open.responder, outcome)
    }

    fn is_open(&self, id: u64) -> bool {
        lock(&self.0).open.iter().any(|open| open.id == id)
    }

    fn start_turn(&self, session_id: &SessionId) -> Turn {
        let mut state = lock(&self.0);
        let id = state.next_id;
        state.next_id += 1;
        state
            .turns
            .entry(session_id.clone())
            .or_default()
            .running
            .push(id);
        Turn {
            id,
            session_id: session_id.clone(),
            pending: self.clone(),
        }
    }

    /// Under the lock: queues the cancel notification with `notify`, marks
    /// the session's running turns cancelled, and answers its open requests
    /// `Cancelled`. If `notify` fails, nothing changes.
    fn cancel_session(
        &self,
        session_id: &SessionId,
        notify: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let mut state = lock(&self.0);
        notify()?;
        if let Some(turns) = state.turns.get_mut(session_id) {
            turns.cancelled = turns.running.clone();
        }
        let (taken, kept): (Vec<Open>, Vec<Open>) = std::mem::take(&mut state.open)
            .into_iter()
            .partition(|open| &open.session_id == session_id);
        state.open = kept;
        for open in taken {
            respond(open.responder, RequestPermissionOutcome::Cancelled);
        }
        Ok(())
    }
}

/// Locks, ignoring poisoning: the state stays consistent across a panic,
/// since every update to it is a single push, removal or insert.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
