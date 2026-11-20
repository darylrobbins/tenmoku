//! A plugin's way to the language servers' sessions: fe-plugin's [`LspSessions`] over the
//! workbench's windows, each with its fe-lsp manager.
//!
//! - **Which session.** A buffer's: the open buffer with the id the plugin names, in the first
//!   window whose manager has it placed on a server, then the instance of the server it is
//!   placed on ([`LspManager::custom_request_for`]). A root's: the manager of the window rooted
//!   there, if the folder is trusted ([`LspManager::custom_request_at`]). No window, buffer or
//!   running instance there is `not-found`, and nothing is started.
//! - **Waiting.** The request is made on the main thread, which hands the plugin's thread its
//!   future; the plugin's thread waits for it until its call's deadline ([`CallDeadline`]) or the
//!   plugin unloads, and no longer. The future is dropped then, which cancels the request.
//! - **Any method.** fe-lsp sends whatever method the plugin names (`crates/fe-lsp/src/custom.rs`);
//!   which ones a plugin may send is fe-plugin's `is_routed` to judge.

use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, Thread},
    time::{Duration, Instant},
};

use fe_buffer::BufferId;
use fe_lsp::{LspError, Unavailable};
use fe_plugin::lsp::{LspSessions, SessionCall, SessionTarget};
use futures::future::BoxFuture;
use gpui::App;
use kiln_l10n::verbatim;
use kiln_plugin::bindings::Error;
use serde_json::Value;

use super::{Bridge, CallDeadline, open_editors};
use crate::Workbench;

impl Bridge {
    /// The language-server sessions, for a plugin whose calls end at `deadline`, or `fallback`
    /// from the wait when no call has set one.
    pub(crate) fn lsp(
        &self,
        deadline: CallDeadline,
        fallback: Duration,
        extension: PathBuf,
        load: Arc<AtomicBool>,
    ) -> Box<dyn LspSessions> {
        Box::new(Sessions {
            bridge: self.clone(),
            deadline,
            fallback,
            extension,
            load,
        })
    }
}

/// Whether the instance of `server` fe-lsp runs is the one of the extension in `dir`: `running`
/// is each server handed to the managers, by extension directory and name, so a same-named server
/// a running one shadows, or one not consented to, isn't in it.
pub(crate) fn runs_for<'a>(
    running: impl IntoIterator<Item = (&'a Path, &'a str)>,
    server: &str,
    dir: &Path,
) -> bool {
    let same = |theirs: &Path| {
        theirs == dir
            || matches!((theirs.canonicalize(), dir.canonicalize()), (Ok(a), Ok(b)) if a == b)
    };
    running
        .into_iter()
        .any(|(theirs, name)| name == server && same(theirs))
}

/// One plugin's way to the sessions.
struct Sessions {
    bridge: Bridge,
    deadline: CallDeadline,
    fallback: Duration,
    /// Its extension's folder.
    extension: PathBuf,
    /// Set when the load this plugin is of is cancelled, even before it has run its `init`.
    load: Arc<AtomicBool>,
}

/// Why a call stops: its plugin's load was cancelled, or its request was cancelled (unload).
#[derive(Clone)]
struct Stop {
    call: Arc<AtomicBool>,
    load: Arc<AtomicBool>,
}

impl Stop {
    fn stopped(&self) -> bool {
        self.call.load(Ordering::Acquire) || self.load.load(Ordering::Acquire)
    }
}

/// What the main thread made of a call.
type Pending = Result<BoxFuture<'static, Result<Value, LspError>>, Error>;

impl Sessions {
    fn deadline(&self) -> Instant {
        (self.deadline.get()).unwrap_or_else(|| Instant::now() + self.fallback)
    }

    fn params(call: &SessionCall<'_>) -> Result<Value, Error> {
        serde_json::from_str(call.params).map_err(|error| {
            Error::InvalidArgument(format!(verbatim!("params are not JSON: {}"), error))
        })
    }
}

impl LspSessions for Sessions {
    fn is_builtin(&self, server: &str) -> bool {
        fe_lsp::is_builtin_server(server)
    }

    fn request(&mut self, call: SessionCall<'_>) -> Result<String, Error> {
        let params = Self::params(&call)?;
        let (server, method) = (call.server.to_owned(), call.method.to_owned());
        let target = call.target.clone();
        let deadline = self.deadline();
        let extension = self.extension.clone();
        let stop = Stop {
            call: call.cancelled.clone(),
            load: self.load.clone(),
        };
        let stopped = stop.clone();
        let pending = self.bridge.on_main_unless(
            deadline,
            Some(&|| stop.stopped()),
            move |_, cx| -> Pending {
                authorize(&server, &extension, &stopped, cx)?;
                find(&target, cx, |manager, buffer| match (&target, buffer) {
                    (SessionTarget::Root(root), _) => Box::pin(manager.custom_request_at(
                        &server,
                        Path::new(root),
                        &method,
                        params,
                    ))
                        as BoxFuture<'static, _>,
                    (SessionTarget::Buffer(_), Some(buffer)) => {
                        Box::pin(manager.custom_request_for(buffer, &server, &method, params))
                            as BoxFuture<'static, _>
                    }
                    (SessionTarget::Buffer(_), None) => {
                        unreachable!("a buffer target has a buffer")
                    }
                })
            },
        )??;
        match wait(pending, &stop, deadline)? {
            Ok(result) => Ok(result.to_string()),
            Err(error) => Err(failed(&error)),
        }
    }

    fn notify(&mut self, call: SessionCall<'_>) -> Result<(), Error> {
        let params = Self::params(&call)?;
        let (server, method) = (call.server.to_owned(), call.method.to_owned());
        let target = call.target.clone();
        let deadline = self.deadline();
        let extension = self.extension.clone();
        let stop = Stop {
            call: call.cancelled.clone(),
            load: self.load.clone(),
        };
        let stopped = stop.clone();
        self.bridge
            .on_main_unless(deadline, Some(&|| stop.stopped()), move |_, cx| {
                authorize(&server, &extension, &stopped, cx)?;
                find(&target, cx, |manager, buffer| match (&target, buffer) {
                    (SessionTarget::Root(root), _) => {
                        manager.custom_notify_at(&server, Path::new(root), &method, params)
                    }
                    (SessionTarget::Buffer(_), Some(buffer)) => {
                        manager.custom_notify_for(buffer, &server, &method, params)
                    }
                    (SessionTarget::Buffer(_), None) => {
                        unreachable!("a buffer target has a buffer")
                    }
                })?
                .map_err(|error| failed(&error))
            })?
    }
}

/// Checks, on the main thread just before a call is made, that its plugin is still loaded and
/// that `server` is a built-in one or the one the plugin's extension in `extension` is running
/// now, by the consent in force at this moment.
fn authorize(server: &str, extension: &Path, stop: &Stop, cx: &App) -> Result<(), Error> {
    if stop.stopped() {
        return Err(Error::Denied(verbatim!("the plugin was unloaded").into()));
    }
    if fe_lsp::is_builtin_server(server) {
        return Ok(());
    }
    let running = crate::lsp_consent::running_extension_servers(cx);
    let theirs = (running.iter()).map(|(dir, name)| (dir.as_path(), name.as_str()));
    if runs_for(theirs, server, extension) {
        Ok(())
    } else {
        Err(Error::Denied(format!(
            verbatim!("{} is not a server this plugin's extension is running"),
            server
        )))
    }
}

/// Runs `with` on the manager `target` names, given the buffer it names. `not-found` with none.
fn find<R>(
    target: &SessionTarget,
    cx: &mut App,
    with: impl FnOnce(&fe_lsp::LspManager, Option<BufferId>) -> R,
) -> Result<R, Error> {
    let windows: Vec<_> = (cx.windows().into_iter())
        .filter_map(|window| window.downcast::<Workbench>())
        .collect();
    match target {
        SessionTarget::Root(root) => {
            for window in windows {
                let manager = window.read(cx).ok().and_then(|workbench| {
                    let trusted = workbench.root_trusted();
                    trusted
                        .then(|| workbench.lsp_manager_at(Path::new(root)))
                        .flatten()
                });
                if let Some(manager) = manager {
                    let manager = manager.lock().unwrap_or_else(|e| e.into_inner());
                    return Ok(with(&manager, None));
                }
            }
            Err(Error::NotFound(format!(
                verbatim!("no trusted open root {}"),
                root
            )))
        }
        SessionTarget::Buffer(id) => {
            let buffer = (open_editors(cx).into_iter())
                .find(|(open, ..)| open == id)
                .map(|(_, _, editor)| editor.read(cx).model().buffer().id())
                .ok_or_else(|| {
                    Error::NotFound(format!(verbatim!("no open buffer has id {}"), id))
                })?;
            for window in windows {
                let manager = window
                    .read(cx)
                    .ok()
                    .and_then(|workbench| workbench.lsp.manager());
                let Some(manager) = manager else { continue };
                let manager = manager.lock().unwrap_or_else(|e| e.into_inner());
                if !manager.servers_of(buffer).is_empty() {
                    return Ok(with(&manager, Some(buffer)));
                }
            }
            Err(Error::NotFound(format!(
                verbatim!("buffer {} is not on a language server"),
                id
            )))
        }
    }
}

/// How fe-lsp's `error` reads to the plugin: no session is `not-found`.
fn failed(error: &LspError) -> Error {
    match error {
        LspError::ServerDown | LspError::Unavailable(Unavailable::NotOpen) => {
            Error::NotFound(format!(verbatim!("no running session there: {}"), error))
        }
        other => Error::Failed(other.to_string()),
    }
}

/// Wakes the thread that waits.
struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// How often a waiting thread looks to see whether its plugin unloaded.
const CHECK: Duration = Duration::from_millis(25);

/// `future`'s output, waiting on this thread until `deadline`, or until `stop` says to. The
/// future is dropped then, which cancels its request.
fn wait<T>(future: impl Future<Output = T>, stop: &Stop, deadline: Instant) -> Result<T, Error> {
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return Ok(output);
        }
        if stop.stopped() {
            return Err(Error::Denied(verbatim!("the plugin was unloaded").into()));
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Error::Failed(
                verbatim!("the language server did not answer before the call's deadline").into(),
            ));
        }
        thread::park_timeout(left.min(CHECK));
    }
}

#[cfg(test)]
#[path = "plugin_lsp_tests.rs"]
mod plugin_lsp_tests;

#[cfg(test)]
#[path = "plugin_lsp_raw_tests.rs"]
mod plugin_lsp_raw_tests;
