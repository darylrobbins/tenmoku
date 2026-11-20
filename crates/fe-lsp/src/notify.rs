//! Notifications to every server of a manager that concern no one
//! document (`workspace/didChangeWatchedFiles`, file operations).

use std::borrow::Cow;

use async_lsp::ServerSocket;
use lsp_types::ServerCapabilities;
use serde_json::Value;

use crate::event::ServerId;
use crate::manager::LspManager;
use crate::server::Command;

/// Queues notification `method`, whatever it is, on `socket`: the editor's
/// own and a plugin's (custom.rs) alike.
pub(crate) fn send(socket: &ServerSocket, method: &str, params: Value) {
    // If the loop has gone, `loop_done` says so.
    let _ = socket.notify_raw(method.to_owned(), params);
}

impl LspManager {
    /// Sends `method` to each running server of this manager, in id order,
    /// with the params `params_for` gives it (none: nothing is sent). The
    /// notification keeps its order with the requests and document sync
    /// queued before it. A server that hasn't initialized gets nothing.
    pub(crate) fn notify_sessions(
        &self,
        method: &'static str,
        params_for: impl Fn(ServerId, &ServerCapabilities) -> Option<Value>,
    ) {
        for id in self.server_ids() {
            let Some(capabilities) = self.capabilities(id) else {
                continue;
            };
            let Some(params) = params_for(id, &capabilities) else {
                continue;
            };
            // A server that has gone has nothing to be told.
            self.send(
                id,
                Command::Notify {
                    method: Cow::Borrowed(method),
                    params,
                },
            );
        }
    }
}
