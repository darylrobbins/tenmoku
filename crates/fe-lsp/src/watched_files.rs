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
//!
//! A server's index can change with no edit, after a change on disk, say,
//! that it reads in its own time: each session's static watch tells it of
//! those changes (registration.rs), [`LspEvent::Notified`](crate::LspEvent::Notified)
//! follows each notification, and [`LspManager::repoll_due`] times the
//! asking again that catches what changes later
//! (docs/decisions/q5j03ax2af-lsp-add-on-servers.md §5, "Asking again").

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use lsp_types::notification::{DidChangeWatchedFiles, Notification};
use lsp_types::{DidChangeWatchedFilesParams, FileChangeType, FileEvent, Url};
use serde_json::Value;

use crate::event::ServerId;
use crate::manager::LspManager;
use crate::registration::Registrations;

/// What [`LspManager::files_changed`] queues: the changes, as
/// `DidChangeWatchedFilesParams`, for the actor to filter. Never sent.
const FILES_CHANGED: &str = "fe-lsp/filesChanged";

impl LspManager {
    /// Tells each server of the changes to files on disk that its
    /// `workspace/didChangeWatchedFiles` registrations match, by glob and
    /// kind, or that its spec's static watch takes (under its own root):
    /// in the order given, each path once (with its last type), in one
    /// notification, or none if nothing matches. A server with neither is
    /// told nothing. Each notification sent is followed by an
    /// [`LspEvent::Notified`](crate::LspEvent::Notified). Paths must be
    /// absolute; others are ignored.
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

    /// The latest poll of `server` due by `now`, on the manager's clock: its
    /// session's initialization plus the most whole periods of its spec's
    /// `repoll` that fit before `now`. A host asks the server again for
    /// what it has cached of the documents it shows each time this
    /// changes. `None` before one period has passed, for a server with no
    /// `repoll`, one that hasn't initialized, or one whose session has ended
    /// (shutting down, waiting out a crash's backoff, crashed too often or
    /// gone). Each session (a restart, say) counts from its own
    /// initialization.
    pub fn repoll_due(&self, server: ServerId, now: Instant) -> Option<Instant> {
        let (since, period) = self.session(server, |session| session.repoll)?;
        let periods = now.checked_duration_since(since)?.as_nanos() / period.as_nanos();
        if periods == 0 {
            return None;
        }
        let elapsed = u64::try_from(periods * period.as_nanos()).ok()?;
        since.checked_add(Duration::from_nanos(elapsed))
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
