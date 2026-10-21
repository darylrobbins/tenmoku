//! `workspace/didChangeWatchedFiles`: file changes the host saw on disk,
//! sent to the servers that registered globs matching them
//! (registration.rs).
//!
//! [`LspManager::files_changed`] queues the changes for every session
//! (notify.rs) under [`FILES_CHANGED`], and each session's actor keeps
//! those its registrations want ([`for_session`]) as it sends them in a
//! `workspace/didChangeWatchedFiles`: the registrations live with the
//! session, and one the router answered before the call is applied. A
//! `workspace/didChangeWatchedFiles` queued as such is sent as it is.

use std::collections::HashMap;
use std::path::PathBuf;

use lsp_types::notification::{DidChangeWatchedFiles, Notification};
use lsp_types::{DidChangeWatchedFilesParams, FileChangeType, FileEvent, Url};
use serde_json::Value;

use crate::manager::LspManager;
use crate::registration::Registrations;

/// What [`LspManager::files_changed`] queues: the changes, as
/// `DidChangeWatchedFilesParams`, for the actor to filter. Never sent.
const FILES_CHANGED: &str = "fe-lsp/filesChanged";

impl LspManager {
    /// Tells each server of the changes to files on disk that its
    /// `workspace/didChangeWatchedFiles` registrations match, by glob and
    /// kind: in the order given, each path once (with its last type), in
    /// one notification, or none if nothing matches. A server with no such
    /// registration is told nothing. Paths must be absolute; others are
    /// ignored.
    pub fn files_changed(&self, changes: Vec<(PathBuf, FileChangeType)>) {
        let mut events: Vec<FileEvent> = Vec::new();
        let mut at: HashMap<PathBuf, usize> = HashMap::new();
        for (path, typ) in changes {
            if let Some(&index) = at.get(&path) {
                events[index].typ = typ;
                continue;
            }
            let Ok(uri) = Url::from_file_path(&path) else {
                log::warn!("files_changed: {} isn't an absolute path", path.display());
                continue;
            };
            at.insert(path, events.len());
            events.push(FileEvent { uri, typ });
        }
        if events.is_empty() {
            return;
        }
        let Ok(params) = serde_json::to_value(DidChangeWatchedFilesParams { changes: events })
        else {
            return;
        };
        self.notify_sessions(FILES_CHANGED, |_, _| Some(params.clone()));
    }
}

/// What a session sends for a queued `method`: for [`FILES_CHANGED`], a
/// `workspace/didChangeWatchedFiles` with only the changes `registrations`
/// want (none: nothing is sent); any other method as it is.
pub(crate) fn for_session(
    method: &'static str,
    params: Value,
    registrations: &Registrations,
) -> Option<(&'static str, Value)> {
    if method != FILES_CHANGED {
        return Some((method, params));
    }
    let mut params: DidChangeWatchedFilesParams = serde_json::from_value(params).ok()?;
    params.changes.retain(|event| {
        event
            .uri
            .to_file_path()
            .is_ok_and(|path| registrations.watches(&path, event.typ))
    });
    if params.changes.is_empty() {
        return None;
    }
    let params = serde_json::to_value(params).ok()?;
    Some((DidChangeWatchedFiles::METHOD, params))
}
