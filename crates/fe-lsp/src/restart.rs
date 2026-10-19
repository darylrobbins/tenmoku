//! Crashes and restarts (docs/decisions/lsp-client.md §3.5).
//!
//! A crash is the process exiting or the transport ending while not
//! shutting down, `initialize` failing or timing out, a main-loop error,
//! or the orphan budget running out (requests.rs). Each joins its server's
//! [`Crashes`], a rolling list of the last [`WINDOW`]; *n* is how many are
//! in it. *n* ≤ [`RESTARTS_MAX`] restarts the server after 1, 2, 4, 8 or
//! 16 s; one more stops it until
//! [`LspManager::restart`](crate::LspManager::restart) clears the list, or
//! its spec changes.
//!
//! A restart is a new session on the same server thread and [`ServerId`]
//! (`crate::ServerId`): the session's documents are kept as openings in a
//! [`Carry`], and while the server waits out its backoff the carry takes
//! the manager's commands. Requests then answer `ServerDown` at once;
//! opens, edits and closes are kept for the next session, which opens
//! every document again at its current text, from LSP version 0.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use fe_buffer::BufferId;
use futures::channel::{mpsc, oneshot};
use futures::future::{BoxFuture, Fuse, FusedFuture};
use futures::{FutureExt, StreamExt, select_biased};

use crate::clock::Clock;
use crate::document::Opening;
use crate::event::LspError;
use crate::identity::Identities;
use crate::launcher::{Flag, ServerSpec};
use crate::server::{Command, Routes, hand_over};

/// How long a crash counts against its server.
pub(crate) const WINDOW: Duration = Duration::from_secs(180);
/// The most crashes in [`WINDOW`] that are restarted; one more stops the
/// server.
pub(crate) const RESTARTS_MAX: u32 = 5;

/// A server's recent crashes, per (name, root), kept by the manager across
/// the instances it starts there.
#[derive(Default)]
pub(crate) struct Crashes {
    times: VecDeque<Instant>,
    /// The spec it last ran with, and whether the manager routes documents
    /// to it, so a restart from `Stopped` knows what to start.
    pub(crate) last: Option<(ServerSpec, bool)>,
    /// It stopped after too many crashes with this spec: documents routed
    /// to it wait, rather than start it again, until a restart or a
    /// different spec.
    pub(crate) given_up: Option<ServerSpec>,
    /// Bumped by each [`clear`](Crashes::clear): an instance started
    /// before then no longer counts its crashes here.
    pub(crate) epoch: u64,
}

pub(crate) type SharedCrashes = Arc<Mutex<Crashes>>;

/// What a crash leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The *n*th crash in the window: restart after `after`.
    Restart { n: u32, after: Duration },
    /// Stop: too many crashes. Routed starts with its spec now wait.
    GiveUp,
    /// The list was cleared since the instance started (a restart replaced
    /// it): its crash isn't counted, and it stops.
    Superseded,
}

impl Crashes {
    /// Counts a crash at `now` of an instance with `spec` started at
    /// `epoch`, dropping those older than [`WINDOW`].
    pub(crate) fn crash(&mut self, now: Instant, epoch: u64, spec: &ServerSpec) -> Verdict {
        if epoch != self.epoch {
            return Verdict::Superseded;
        }
        while self
            .times
            .front()
            .is_some_and(|&at| now.saturating_duration_since(at) > WINDOW)
        {
            self.times.pop_front();
        }
        self.times.push_back(now);
        let n = u32::try_from(self.times.len()).unwrap_or(u32::MAX);
        if n > RESTARTS_MAX {
            self.given_up = Some(spec.clone());
            return Verdict::GiveUp;
        }
        Verdict::Restart {
            n,
            after: Duration::from_secs(1 << (n - 1)),
        }
    }

    /// Forgets every crash, and having given up.
    pub(crate) fn clear(&mut self) {
        self.times.clear();
        self.given_up = None;
        self.epoch += 1;
    }
}

/// What a server thread keeps from one session to the next: the manager's
/// commands and stop, and the documents to open again. Dropped, it hands
/// the documents on as the session's end would ([`hand_over`]).
pub(crate) struct Carry {
    pub(crate) commands: mpsc::UnboundedReceiver<Command>,
    pub(crate) stop: oneshot::Receiver<()>,
    /// The manager's kill switch.
    pub(crate) kill: Kill,
    /// Documents to open in the next session.
    pub(crate) openings: Vec<Opening>,
    /// The spec's settings as they are now.
    pub(crate) settings: serde_json::Value,
    pub(crate) routes: Routes,
    pub(crate) identities: Identities,
}

/// How waiting out a backoff ended.
pub(crate) enum Waited {
    Elapsed,
    /// Asked to stop, killed, or every sender has gone: the documents have
    /// been handed on.
    Stopped,
}

/// Resolves once the manager's kill switch is thrown
/// ([`KillSwitch`](crate::KillSwitch)): the server thread then kills its
/// process at once, and launches and restarts nothing more.
pub(crate) struct Kill {
    switch: Arc<Flag>,
    wait: Fuse<BoxFuture<'static, ()>>,
}

impl Kill {
    pub(crate) fn new(switch: Arc<Flag>) -> Self {
        let wait = Flag::wait(&switch).boxed().fuse();
        Self { switch, wait }
    }

    /// Whether the switch has been thrown, whether or not this has been
    /// polled since.
    pub(crate) fn thrown(&self) -> bool {
        self.switch.fired()
    }

    /// Another wait on the same switch.
    pub(crate) fn again(&self) -> Self {
        Self::new(self.switch.clone())
    }
}

impl Future for Kill {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.wait.poll_unpin(cx)
    }
}

impl FusedFuture for Kill {
    fn is_terminated(&self) -> bool {
        self.wait.is_terminated()
    }
}

impl Carry {
    /// Whether the manager has asked for a stop (or gone), or thrown its
    /// kill switch.
    pub(crate) fn stopping(&mut self) -> bool {
        !matches!(self.stop.try_recv(), Ok(None)) || self.kill.thrown()
    }

    /// Waits `after` on `clock`, taking commands meanwhile ([`hold`]).
    ///
    /// [`hold`]: Self::hold
    pub(crate) async fn wait_out(&mut self, clock: &dyn Clock, after: Duration) -> Waited {
        let mut timer = clock.sleep(after).fuse();
        loop {
            select_biased! {
                () = &mut self.kill => break,
                _ = &mut self.stop => break,
                () = timer => return Waited::Elapsed,
                command = self.commands.next() => match command {
                    Some(command) => self.hold(command),
                    None => break,
                },
            }
        }
        self.hand_over();
        Waited::Stopped
    }

    /// Carries out a command while no session runs: a request answers
    /// `ServerDown` (its answer dropped), and documents are kept for the
    /// next session.
    pub(crate) fn hold(&mut self, command: Command) {
        match command {
            // Dropped, they answer `ServerDown`.
            Command::Request(_) | Command::Documents(_) => {}
            Command::Admit { admit, .. } => {
                // It answers the caller itself, so issues nothing.
                let _ = admit(Err(LspError::ServerDown));
            }
            Command::WithDocuments(admit) => {
                let _ = admit(Err(LspError::ServerDown));
            }
            Command::Open(opening) => {
                if self
                    .identities
                    .is_current(opening.buffer, opening.generation)
                {
                    self.openings.retain(|held| held.buffer != opening.buffer);
                    self.openings.push(*opening);
                }
            }
            Command::Changed(buffer) => {
                if let Some(opening) = self.opening(buffer) {
                    opening.catch_up();
                }
            }
            Command::Close(buffer) => self.openings.retain(|held| held.buffer != buffer),
            Command::Configure(settings) => self.settings = settings,
            Command::Notify { .. } => {}
            Command::Drain => {}
            #[cfg(test)]
            Command::Ring(_, answer) => {
                let _ = answer.send(Vec::new());
            }
        }
    }

    fn opening(&mut self, buffer: BufferId) -> Option<&mut Opening> {
        self.openings.iter_mut().find(|held| held.buffer == buffer)
    }

    /// The documents to open in the next session, brought up to their
    /// buffers' latest text, oldest first; a buffer closed meanwhile is
    /// dropped.
    pub(crate) fn reopen(&mut self) -> VecDeque<Command> {
        std::mem::take(&mut self.openings)
            .into_iter()
            .filter(|opening| {
                self.identities
                    .is_current(opening.buffer, opening.generation)
            })
            .map(|mut opening| {
                opening.catch_up();
                Command::Open(Box::new(opening))
            })
            .collect()
    }

    /// Closes the commands and forwards every document to wherever the
    /// manager now places it.
    pub(crate) fn hand_over(&mut self) {
        let openings = std::mem::take(&mut self.openings);
        hand_over(&mut self.commands, openings, &self.routes, &self.identities);
    }
}

impl Drop for Carry {
    fn drop(&mut self) {
        self.hand_over();
    }
}
