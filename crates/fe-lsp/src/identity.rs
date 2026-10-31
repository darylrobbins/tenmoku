//! The active-identity table (docs/decisions/lsp-client.md §4.1): which
//! `(session, incarnation)` of each open buffer is current on each server.
//!
//! - A **session** is one run of a server; an **incarnation** is one
//!   `didOpen` … `didClose` of a document within a session. Both ids come
//!   from this table and are never reused, so a retired identity never
//!   becomes active again.
//! - Each open buffer has an **open generation**, which
//!   [`LspManager::open`](crate::LspManager::open) and
//!   [`close`](crate::LspManager::close) bump on the calling thread. Every
//!   open the actor performs carries the generation it was issued under,
//!   and activates only while the buffer is still open under it: a close,
//!   or a close and reopen, made meanwhile makes it stale.
//!
//! Only lifecycle changes write the table: an open activating, a close
//! retiring the buffer's identities, the actor's own size-limit close
//! retiring its own, and a session's end retiring that session's.
//!
//! The manager and every server thread share it behind one mutex.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use fe_buffer::BufferId;

use crate::event::ServerId;

/// One run of a server, from launch to its end. Never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(u64);

/// One `didOpen` … `didClose` of a document within a session. Never
/// reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IncarnationId(u64);

/// A buffer's open generation: bumped by every `open` and `close`. Drawn
/// from one counter for all buffers, so a buffer closed and opened again
/// never gets back a generation it had.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Generation(u64);

/// The table, shared by the manager and its servers' actors.
#[derive(Clone, Default)]
pub(crate) struct Identities(Arc<Shared>);

#[derive(Default)]
struct Shared {
    table: Mutex<Table>,
    #[cfg(any(test, feature = "test-support"))]
    pauses: pause::Pauses,
}

#[derive(Default)]
struct Table {
    next_session: u64,
    next_incarnation: u64,
    next_generation: u64,
    /// The buffers open on the manager: a closed buffer has no entry.
    open: HashMap<BufferId, Open>,
}

struct Open {
    generation: Generation,
    /// The active identity on each server that has the buffer open.
    active: HashMap<ServerId, (SessionId, IncarnationId)>,
}

impl Table {
    fn bump(&mut self) -> Generation {
        self.next_generation += 1;
        Generation(self.next_generation)
    }
}

impl Identities {
    fn lock(&self) -> MutexGuard<'_, Table> {
        self.0.table.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A new session's id.
    pub(crate) fn new_session(&self) -> SessionId {
        let mut table = self.lock();
        table.next_session += 1;
        SessionId(table.next_session)
    }

    /// Marks `buffer` open under a new generation, which the actor's open
    /// carries. Called after [`close`](Self::close), so it has no
    /// identities yet.
    pub(crate) fn open(&self, buffer: BufferId) -> Generation {
        let mut table = self.lock();
        let generation = table.bump();
        table.open.insert(
            buffer,
            Open {
                generation,
                active: HashMap::new(),
            },
        );
        generation
    }

    /// Marks `buffer` closed, bumping its generation and retiring its
    /// identities in one critical section.
    pub(crate) fn close(&self, buffer: BufferId) {
        let mut table = self.lock();
        table.bump();
        table.open.remove(&buffer);
    }

    /// Whether `buffer` is still open under `generation`.
    pub(crate) fn is_current(&self, buffer: BufferId, generation: Generation) -> bool {
        self.lock()
            .open
            .get(&buffer)
            .is_some_and(|open| open.generation == generation)
    }

    /// Activates a new incarnation of `buffer` on `server`, if the buffer
    /// is still open under `generation`. `None` means the open is stale: it
    /// must insert nothing and send nothing.
    pub(crate) fn activate(
        &self,
        buffer: BufferId,
        generation: Generation,
        server: ServerId,
        session: SessionId,
    ) -> Option<IncarnationId> {
        let mut table = self.lock();
        let table = &mut *table;
        let open = table
            .open
            .get_mut(&buffer)
            .filter(|open| open.generation == generation)?;
        table.next_incarnation += 1;
        let incarnation = IncarnationId(table.next_incarnation);
        open.active.insert(server, (session, incarnation));
        Some(incarnation)
    }

    /// The actor's own close (a document that outgrew the size limit):
    /// retires `buffer`'s entry on `server` only if the buffer is still open
    /// under `generation` and the entry is still `identity`. Otherwise the
    /// buffer was closed, or closed and reopened, since, and that close
    /// already retired it. It never bumps the generation.
    pub(crate) fn retire(
        &self,
        buffer: BufferId,
        generation: Generation,
        server: ServerId,
        identity: (SessionId, IncarnationId),
    ) {
        let mut table = self.lock();
        if let Some(open) = table.open.get_mut(&buffer)
            && open.generation == generation
            && open.active.get(&server) == Some(&identity)
        {
            open.active.remove(&server);
        }
    }

    /// Retires every identity of `session`, and no others.
    pub(crate) fn end_session(&self, session: SessionId) {
        for open in self.lock().open.values_mut() {
            open.active.retain(|_, (of, _)| *of != session);
        }
    }

    /// The active identity of `buffer` on `server`.
    pub(crate) fn active(
        &self,
        buffer: BufferId,
        server: ServerId,
    ) -> Option<(SessionId, IncarnationId)> {
        self.lock().open.get(&buffer)?.active.get(&server).copied()
    }

    /// `buffer`'s current generation, while it's open.
    #[cfg(test)]
    pub(crate) fn generation(&self, buffer: BufferId) -> Option<Generation> {
        self.lock().open.get(&buffer).map(|open| open.generation)
    }

    /// Where an actor may be held by a test, outside the table's lock.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn pauses(&self) -> &pause::Pauses {
        &self.0.pauses
    }

    /// Holds the calling thread at `point` if a test armed it.
    #[inline]
    pub(crate) fn pause(&self, point: Point) {
        #[cfg(any(test, feature = "test-support"))]
        self.0.pauses.reach(point);
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = point;
    }
}

/// Points in an actor's document lifecycle a test can hold it at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Point {
    /// An open, before it activates.
    Activate,
    /// A size-limit close, before it retires its identity.
    Retire,
}

/// Held by a session's actor: when the actor goes, however it goes (a
/// panic included), the session's identities go with it.
pub(crate) struct EndSession(pub(crate) Identities, pub(crate) SessionId);

impl Drop for EndSession {
    fn drop(&mut self) {
        self.0.end_session(self.1);
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) mod pause {
    use std::sync::{Condvar, Mutex, PoisonError};
    use std::time::{Duration, Instant};

    use super::Point;

    const WAIT: Duration = Duration::from_secs(10);

    /// A test's holds on actors: [`arm`](Self::arm) a point, and the next
    /// thread to reach it blocks there until [`resume`](Self::resume).
    #[derive(Default)]
    pub(crate) struct Pauses {
        state: Mutex<State>,
        changed: Condvar,
    }

    #[derive(Default)]
    struct State {
        armed: Vec<Point>,
        held: Option<Point>,
    }

    impl Pauses {
        pub(crate) fn arm(&self, point: Point) {
            self.lock().armed.push(point);
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, State> {
            self.state.lock().unwrap_or_else(PoisonError::into_inner)
        }

        fn wait(&self, what: &str, done: impl Fn(&State) -> bool) {
            let deadline = Instant::now() + WAIT;
            let mut state = self.lock();
            while !done(&state) {
                let now = Instant::now();
                assert!(now < deadline, "timed out waiting for {what}");
                state = self
                    .changed
                    .wait_timeout(state, deadline - now)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        }

        pub(super) fn reach(&self, point: Point) {
            {
                let mut state = self.lock();
                let Some(at) = state.armed.iter().position(|armed| *armed == point) else {
                    return;
                };
                state.armed.remove(at);
                state.held = Some(point);
            }
            self.changed.notify_all();
            self.wait("a resume", |state| state.held != Some(point));
        }

        /// Blocks until a thread is held at `point`.
        pub(crate) fn wait_held(&self, point: Point) {
            self.wait(&format!("an actor held at {point:?}"), |state| {
                state.held == Some(point)
            });
        }

        pub(crate) fn resume(&self) {
            self.lock().held = None;
            self.changed.notify_all();
        }
    }
}
