//! Dynamic registration (docs/decisions/lsp-client.md §3.6).
//!
//! A successful reply to `client/registerCapability` means the
//! registration is installed, so it accepts only methods whose effect
//! fe-lsp implements:
//!
//! - `workspace/didChangeConfiguration`, which is sent whenever settings
//!   change, whatever is registered (its `dynamicRegistration` is false,
//!   but servers register it anyway);
//! - `workspace/didChangeWatchedFiles`, whose `registerOptions`
//!   (`DidChangeWatchedFilesRegistrationOptions`) name the globs and kinds
//!   of the file changes the server wants (watched_files.rs). Its options
//!   must be present and parse, and each watcher's glob must build
//!   ([`LspGlob`]); a watcher's kind defaults to create | change | delete.
//!
//! Any other method, or a watched-files registration breaking that rule,
//! refuses the whole request (registration is all or nothing) with
//! `RequestFailed`, naming the methods, and records nothing.
//! `client/unregisterCapability` removes the ids it names, and their
//! watchers; an unknown id is a no-op.

use std::collections::BTreeMap;
use std::path::Path;

use async_lsp::{ErrorCode, ResponseError};
use lsp_types::notification::{DidChangeConfiguration, DidChangeWatchedFiles, Notification};
use lsp_types::{
    DidChangeWatchedFilesRegistrationOptions, FileChangeType, Registration, RegistrationParams,
    UnregistrationParams, WatchKind,
};

use crate::glob::LspGlob;

/// The methods a server may register.
const SUPPORTED: [&str; 2] = [
    DidChangeConfiguration::METHOD,
    DidChangeWatchedFiles::METHOD,
];

/// An accepted registration's method, with what it registered.
enum Registered {
    Configuration,
    /// Each watcher's glob and the kinds of change it wants.
    WatchedFiles(Vec<(LspGlob, WatchKind)>),
}

/// A session's accepted registrations, by registration id.
#[derive(Default)]
pub(crate) struct Registrations(BTreeMap<String, Registered>);

impl Registrations {
    pub(crate) fn register(&mut self, params: RegistrationParams) -> Result<(), ResponseError> {
        let mut refused = Vec::new();
        let mut accepted = Vec::new();
        for registration in params.registrations {
            if !SUPPORTED.contains(&registration.method.as_str()) {
                refused.push(registration.method);
                continue;
            }
            match registered(&registration) {
                Ok(registered) => accepted.push((registration.id, registered)),
                Err(why) => refused.push(format!("{} ({why})", registration.method)),
            }
        }
        if !refused.is_empty() {
            return Err(ResponseError::new(
                ErrorCode::REQUEST_FAILED,
                format!("Tenmoku can't register {}", refused.join(", ")),
            ));
        }
        self.0.extend(accepted);
        Ok(())
    }

    pub(crate) fn unregister(&mut self, params: UnregistrationParams) {
        for unregistration in params.unregisterations {
            self.0.remove(&unregistration.id);
        }
    }

    /// Whether a watched-files registration wants a change of `typ` to
    /// `path`.
    pub(crate) fn watches(&self, path: &Path, typ: FileChangeType) -> bool {
        let kind = match typ {
            FileChangeType::CREATED => WatchKind::Create,
            FileChangeType::CHANGED => WatchKind::Change,
            FileChangeType::DELETED => WatchKind::Delete,
            _ => return false,
        };
        self.0
            .values()
            .flat_map(|registered| match registered {
                Registered::Configuration => &[][..],
                Registered::WatchedFiles(watchers) => watchers,
            })
            .any(|(glob, wanted)| wanted.contains(kind) && glob.matches(path))
    }

    /// The registration ids, in order.
    #[cfg(test)]
    pub(crate) fn ids(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }
}

/// What a registration of a supported method registers.
fn registered(registration: &Registration) -> Result<Registered, String> {
    if registration.method == DidChangeConfiguration::METHOD {
        return Ok(Registered::Configuration);
    }
    let options = registration
        .register_options
        .clone()
        .ok_or("no registerOptions")?;
    let options: DidChangeWatchedFilesRegistrationOptions =
        serde_json::from_value(options).map_err(|err| format!("unreadable options: {err}"))?;
    let watchers = options
        .watchers
        .iter()
        .map(|watcher| {
            let glob = LspGlob::new(&watcher.glob_pattern)?;
            Ok((glob, watcher.kind.unwrap_or(WatchKind::all())))
        })
        .collect::<Result<_, String>>()?;
    Ok(Registered::WatchedFiles(watchers))
}
