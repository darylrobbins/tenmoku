//! The client side's router: what fe-lsp answers when a server asks, and
//! what it does with the server's notifications (§3.6).

use std::future::ready;
use std::ops::ControlFlow;
use std::rc::Rc;

use async_lsp::ServerSocket;
use async_lsp::router::Router;
use lsp_types::request::{
    ApplyWorkspaceEdit, CodeLensRefresh, InlayHintRefreshRequest, SemanticTokensRefresh,
    ShowDocument, ShowMessageRequest, WorkDoneProgressCreate, WorkspaceConfiguration,
    WorkspaceDiagnosticRefresh, WorkspaceFoldersRequest,
};
use lsp_types::{ApplyWorkspaceEditResponse, ShowDocumentResult, WorkspaceFolder};
use serde_json::Value;

use crate::server::{Queue, Taken};

/// What the router's handlers know about their server.
pub(crate) struct ClientState {
    /// The server's name, for logs.
    pub(crate) name: String,
    /// The single workspace folder.
    pub(crate) folder: WorkspaceFolder,
    /// The spec's `settings`, answering `workspace/configuration`.
    pub(crate) settings: Value,
    /// Where the loop counts the actor's requests it has taken.
    pub(crate) queue: Rc<Queue>,
    /// Keeps the main loop's event channel open for as long as the loop
    /// lives: async-lsp panics if every sender is gone.
    pub(crate) _socket: ServerSocket,
}

pub(crate) fn client_router(state: ClientState) -> Router<ClientState> {
    let mut router = Router::new(state);
    router
        .request::<WorkDoneProgressCreate, _>(|_, _| ready(Ok(())))
        .request::<WorkspaceFoldersRequest, _>(|st, ()| ready(Ok(Some(vec![st.folder.clone()]))))
        .request::<WorkspaceConfiguration, _>(|st, params| {
            let answers = params
                .items
                .iter()
                .map(|item| section(&st.settings, item.section.as_deref()))
                .collect();
            ready(Ok(answers))
        })
        .request::<ApplyWorkspaceEdit, _>(|_, _| {
            ready(Ok(ApplyWorkspaceEditResponse {
                applied: false,
                failure_reason: Some("not supported".into()),
                failed_change: None,
            }))
        })
        .request::<ShowMessageRequest, _>(|st, params| {
            log::info!("{}: {:?}: {}", st.name, params.typ, params.message);
            ready(Ok(None))
        })
        .request::<ShowDocument, _>(|_, _| ready(Ok(ShowDocumentResult { success: false })))
        .request::<WorkspaceDiagnosticRefresh, _>(|_, ()| ready(Ok(())))
        .request::<SemanticTokensRefresh, _>(|_, ()| ready(Ok(())))
        .request::<InlayHintRefreshRequest, _>(|_, ()| ready(Ok(())))
        .request::<CodeLensRefresh, _>(|_, ()| ready(Ok(())))
        .event::<Taken>(|st, Taken| {
            st.queue.took();
            ControlFlow::Continue(())
        })
        // async-lsp's default stops the main loop on any notification it
        // has no handler for, unless its method starts with `$/`.
        .unhandled_notification(|st, notif| {
            log::info!("{}: ignored notification {}", st.name, notif.method);
            ControlFlow::Continue(())
        });
    // Requests left unhandled get async-lsp's `MethodNotFound`.
    router
}

/// The value at a dotted `section` of `settings` ("a.b" is `settings.a.b`),
/// the whole of it for no section, or `null`.
fn section(settings: &Value, section: Option<&str>) -> Value {
    let Some(section) = section.filter(|s| !s.is_empty()) else {
        return settings.clone();
    };
    section
        .split('.')
        .try_fold(settings, |value, key| value.get(key))
        .cloned()
        .unwrap_or(Value::Null)
}
