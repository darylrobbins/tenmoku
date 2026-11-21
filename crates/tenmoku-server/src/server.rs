//! The sessions (D § 3): one per client window, made at the handshake. A
//! session whose link drops stays for the grace, then is torn down: its
//! process groups get TERM, and KILL [`TERM_WAIT`] later.
//!
//! One mutex guards everything, and one condvar says it changed: the reaper
//! waits on it for the next deadline, and [`Snapshot`] readers for what they
//! want to see. The clock wakes it too when a test moves it.

use crate::Config;
use crate::clock::Clock;
use fe_remote_proto::{
    Accept, ControlOp, HandshakeError, Hello, Incoming, IoError, IoKind, Link, Mux, OpenTarget,
    PathBytes, SessionToken, Status, chan,
};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read as _};
use std::os::unix::process::CommandExt as _;
use std::process::{Child, Command, ExitStatus};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Between a session's TERM and its KILL (D § 3).
pub const TERM_WAIT: Duration = Duration::from_secs(2);
/// How many ended sessions [`Snapshot::torn_down`] remembers.
const TORN_DOWN_KEPT: usize = 16;
/// The longest [`Shared::wait_for`] waits, in real time: far past anything
/// a test waits for, so a broken one fails instead of hanging.
const WAIT_LIMIT: Duration = Duration::from_secs(60);

/// What a daemon holds now, for its log and for tests.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// The live sessions, oldest first.
    pub sessions: Vec<SessionInfo>,
    /// Sessions whose grace has ended, between their TERM and their KILL.
    pub ending: Vec<SessionToken>,
    /// Sessions whose groups have had their KILL, with leaders not yet
    /// reaped.
    pub reaping: Vec<SessionToken>,
    /// The last sessions to end, oldest first.
    pub torn_down: Vec<TornDown>,
    /// The daemon has stopped serving and let go of its lock.
    pub exited: bool,
}

impl Snapshot {
    pub fn session(&self, token: SessionToken) -> Option<&SessionInfo> {
        self.sessions.iter().find(|s| s.token == token)
    }
}

#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub token: SessionToken,
    /// `Hello.location`: the destination as the client typed it, and the
    /// project's path on this host.
    pub destination: String,
    pub path: PathBytes,
    /// Whether a link is serving it now. A detached session waits out its
    /// grace for a client to come back.
    pub attached: bool,
    /// The leaders' pids of its process groups.
    pub groups: Vec<u32>,
}

/// A session that has ended, and how each of its process groups' leaders
/// exited.
#[derive(Clone, Debug)]
pub struct TornDown {
    pub session: SessionToken,
    pub exits: Vec<(u32, ExitStatus)>,
}

pub(crate) struct Shared {
    pub(crate) config: Config,
    state: Mutex<State>,
    changed: Condvar,
    /// Moves of a manual clock wake the reaper through this.
    _wake: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
struct State {
    sessions: Vec<Session>,
    ending: Vec<Ending>,
    /// Sessions whose groups have had their KILL, with leaders not yet
    /// reaped.
    reaping: Vec<SessionToken>,
    torn_down: VecDeque<TornDown>,
    /// When the last session ended, or the daemon started.
    idle_since: Option<Instant>,
    /// The reaper has decided to exit: no more sessions are made.
    exiting: bool,
    exited: bool,
    /// The clock's time at the reaper's last finished pass.
    seen: Option<Instant>,
    next_conn: u64,
}

struct Session {
    token: SessionToken,
    destination: String,
    path: PathBytes,
    link: Attachment,
    /// Its process groups' leaders. A leader is reaped only once its group
    /// has had its KILL: while it's unreaped, its pid, and so the group's
    /// id, can't be given to another process, so the signals can't land on
    /// a stranger.
    groups: Vec<Child>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Attachment {
    /// Served by the link the daemon numbered so.
    Attached(u64),
    /// Its link dropped then.
    Detached(Instant),
}

/// A session past its grace, waiting for its KILL.
struct Ending {
    session: SessionToken,
    groups: Vec<Child>,
    kill_at: Instant,
}

impl Shared {
    pub(crate) fn new(config: Config) -> Arc<Shared> {
        let now = config.clock.now();
        Arc::new_cyclic(|weak: &std::sync::Weak<Shared>| {
            let weak = weak.clone();
            let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                if let Some(shared) = weak.upgrade() {
                    // Taking the lock orders this after a waiter's look at
                    // the time.
                    let _st = shared.lock();
                    shared.changed.notify_all();
                }
            });
            config.clock.subscribe(&wake);
            Shared {
                config,
                state: Mutex::new(State {
                    idle_since: Some(now),
                    ..State::default()
                }),
                changed: Condvar::new(),
                _wake: wake,
            }
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn clock(&self) -> &Clock {
        &self.config.clock
    }

    /// Runs the handshake on `link` and serves the session it makes until
    /// the link is lost, when the session is left to its grace. `on_accept`
    /// runs once a `Hello` is taken, before the `Welcome` goes.
    pub(crate) fn serve_link(
        &self,
        link: Link,
        on_accept: impl FnOnce(),
    ) -> Result<SessionToken, HandshakeError> {
        let conn = {
            let mut st = self.lock();
            st.next_conn += 1;
            st.next_conn
        };
        let mut made = None;
        let handshake = Mux::server(link, self.config.mux.clone(), |hello| {
            let accept = self.accept(hello, conn)?;
            made = Some(accept.session);
            on_accept();
            Ok(accept)
        });
        let mux = match handshake {
            Ok((mux, _)) => mux,
            Err(e) => {
                // A session made for a link that then failed to start
                // waits for its client like any other.
                if let Some(token) = made {
                    self.detach(token, conn);
                }
                log!("a handshake failed: {e}");
                return Err(e);
            }
        };
        let token = made.expect("an accepted handshake made a session");
        while let Some(incoming) = mux.accept() {
            answer(incoming);
        }
        let lost = mux.wait_lost();
        log!("session {} lost its link: {lost}", hex(token));
        self.detach(token, conn);
        Ok(token)
    }

    /// The session for `hello`: the one it resumes, if that one is waiting
    /// for its client, else a new one. Resuming the streams themselves is
    /// phase 9's (T1991): there are none yet.
    fn accept(&self, hello: &Hello, conn: u64) -> Result<Accept, String> {
        let mut st = self.lock();
        // A session whose grace is over is over, whether or not the reaper
        // has got to it yet.
        self.expire(&mut st, self.clock().now());
        self.changed.notify_all();
        if st.exiting {
            return Err("this tenmoku-server daemon is shutting down; connect again".into());
        }
        let resumed = hello.resume.as_ref().and_then(|r| {
            st.sessions
                .iter_mut()
                .find(|s| s.token == r.session && matches!(s.link, Attachment::Detached(_)))
        });
        let token = if let Some(session) = resumed {
            session.link = Attachment::Attached(conn);
            log!("session {} re-attached", hex(session.token));
            session.token
        } else {
            let token = loop {
                let token = SessionToken(random().map_err(|e| format!("no session token: {e}"))?);
                if st.sessions.iter().all(|s| s.token != token) {
                    break token;
                }
            };
            if let Some(r) = &hello.resume {
                log!(
                    "no session {} to resume: starting a new one",
                    hex(r.session)
                );
            }
            log!(
                "session {} for {} {}",
                hex(token),
                hello.location.destination,
                String::from_utf8_lossy(&hello.location.path.0)
            );
            st.sessions.push(Session {
                token,
                destination: hello.location.destination.clone(),
                path: hello.location.path.clone(),
                link: Attachment::Attached(conn),
                groups: Vec::new(),
            });
            token
        };
        st.idle_since = None;
        self.changed.notify_all();
        Ok(Accept {
            session: token,
            streams: Vec::new(),
        })
    }

    /// Starts `session`'s grace, unless another link has taken it since.
    fn detach(&self, token: SessionToken, conn: u64) {
        let now = self.clock().now();
        let mut st = self.lock();
        if let Some(s) = st.sessions.iter_mut().find(|s| s.token == token)
            && s.link == Attachment::Attached(conn)
        {
            s.link = Attachment::Detached(now);
            self.changed.notify_all();
        }
    }

    /// Spawns `command` as the leader of a new process group that belongs
    /// to `session`, and is torn down with it.
    pub(crate) fn spawn_group(
        &self,
        token: SessionToken,
        command: &mut Command,
    ) -> io::Result<u32> {
        let mut st = self.lock();
        self.expire(&mut st, self.clock().now());
        self.changed.notify_all();
        let session = st
            .sessions
            .iter_mut()
            .find(|s| s.token == token)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such session"))?;
        let child = command.process_group(0).spawn()?;
        let pid = child.id();
        session.groups.push(child);
        self.changed.notify_all();
        Ok(pid)
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        snapshot(&self.lock())
    }

    /// Waits until `seen` holds of the state, for up to a minute of real
    /// time. False if it never did.
    pub(crate) fn wait_for(&self, mut seen: impl FnMut(&Snapshot) -> bool) -> bool {
        let deadline = Instant::now() + WAIT_LIMIT;
        let mut st = self.lock();
        loop {
            if seen(&snapshot(&st)) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            st = (self.changed)
                .wait_timeout(st, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Waits for the reaper to have acted on the clock's time now. With a
    /// manual clock, what it hasn't done by then it won't do until the
    /// clock moves again.
    pub(crate) fn settle(&self) -> bool {
        let target = self.clock().now();
        let mut st = self.lock();
        let deadline = Instant::now() + WAIT_LIMIT;
        while !st.exited && st.seen.is_none_or(|seen| seen < target) {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            st = (self.changed)
                .wait_timeout(st, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        true
    }

    /// Whether the reaper has decided to exit, after which no session is
    /// made.
    pub(crate) fn exiting(&self) -> bool {
        self.lock().exiting
    }

    pub(crate) fn set_exited(&self) {
        self.lock().exited = true;
        self.changed.notify_all();
    }

    /// Ends sessions as their graces run out, until there are none and,
    /// with `linger`, none has come for that long since the last ended.
    /// Then no new session is made, and it returns.
    pub(crate) fn reap(self: &Arc<Self>, linger: Option<Duration>) {
        let mut st = self.lock();
        loop {
            let now = self.clock().now();
            self.expire(&mut st, now);
            let (due, waiting): (Vec<Ending>, Vec<Ending>) = std::mem::take(&mut st.ending)
                .into_iter()
                .partition(|e| now >= e.kill_at);
            st.ending = waiting;
            // Every due group has its KILL before any leader is waited for,
            // and the waits are off this loop: a leader stuck in the kernel
            // holds up only its own reaping.
            for ending in due {
                for leader in &ending.groups {
                    signal_group(leader, libc::SIGKILL);
                }
                self.reap_on_a_thread(&mut st, ending);
            }
            let quiet = st.sessions.is_empty() && st.ending.is_empty();
            let lingered = match linger {
                None => true,
                Some(linger) => st
                    .idle_since
                    .is_some_and(|idle| after(idle, linger).is_some_and(|end| now >= end)),
            };
            st.seen = Some(now);
            if quiet && lingered {
                st.exiting = true;
                self.changed.notify_all();
                return;
            }
            self.changed.notify_all();
            let next = self.next_deadline(&st, linger);
            st = match (self.clock(), next) {
                (Clock::System, Some(next)) => {
                    (self.changed)
                        .wait_timeout(st, next.saturating_duration_since(now))
                        .unwrap_or_else(|e| e.into_inner())
                        .0
                }
                // A manual clock wakes us when it moves.
                _ => self.changed.wait(st).unwrap_or_else(|e| e.into_inner()),
            };
        }
    }

    /// Waits on a thread of its own for the leaders of `ending`'s groups,
    /// which have had their KILL, and records how they exited. `st` is the
    /// caller's guard: the thread takes the lock only once it's released.
    fn reap_on_a_thread(self: &Arc<Self>, st: &mut State, ending: Ending) {
        let session = ending.session;
        let groups: Vec<u32> = ending.groups.iter().map(Child::id).collect();
        let shared = self.clone();
        st.reaping.push(session);
        let builder = std::thread::Builder::new().name("session-reaper".into());
        #[cfg(feature = "test-support")]
        let start = crate::testing::injected_reaper_failure();
        #[cfg(not(feature = "test-support"))]
        let start: io::Result<()> = Ok(());
        // `ending` moves in only once the thread has started. If it can't,
        // the leaders are dropped unreaped, and stay zombies until the
        // daemon exits: their groups have had their KILL all the same.
        let spawned = start.and_then(|()| {
            builder.spawn(move || {
                let done = reaped(ending);
                let mut st = shared.lock();
                st.reaping.retain(|t| *t != done.session);
                record(&mut st, done);
                shared.changed.notify_all();
            })
        });
        if let Err(e) = spawned {
            log!(
                "can't reap session {}'s process groups {groups:?}: {e}",
                hex(session)
            );
            st.reaping.retain(|t| *t != session);
            record(
                st,
                TornDown {
                    session,
                    exits: Vec::new(),
                },
            );
        }
    }

    /// Ends the detached sessions whose grace is over at `now`: TERM to
    /// each of their process groups, and KILL due [`TERM_WAIT`] after that
    /// TERM, however late it went. The linger counts from when the grace
    /// ended.
    fn expire(&self, st: &mut State, now: Instant) {
        let grace = self.config.grace;
        let end = |s: &Session| match s.link {
            Attachment::Detached(since) => after(since, grace).filter(|end| now >= *end),
            Attachment::Attached(_) => None,
        };
        let (ended, live): (Vec<Session>, Vec<Session>) =
            (std::mem::take(&mut st.sessions).into_iter()).partition(|s| end(s).is_some());
        st.sessions = live;
        let mut last_end = None;
        for session in ended {
            let ended_at = end(&session).expect("an ended session's grace has an end");
            last_end = last_end.max(Some(ended_at));
            log!(
                "session {} ended: no client came back within {:?}",
                hex(session.token),
                grace
            );
            for group in &session.groups {
                signal_group(group, libc::SIGTERM);
            }
            if session.groups.is_empty() {
                record(
                    st,
                    TornDown {
                        session: session.token,
                        exits: Vec::new(),
                    },
                );
            } else {
                st.ending.push(Ending {
                    session: session.token,
                    groups: session.groups,
                    kill_at: now + TERM_WAIT,
                });
            }
        }
        if st.sessions.is_empty() && last_end.is_some() {
            st.idle_since = last_end;
        }
    }

    fn next_deadline(&self, st: &State, linger: Option<Duration>) -> Option<Instant> {
        let graces = st.sessions.iter().filter_map(|s| match s.link {
            Attachment::Detached(since) => after(since, self.config.grace),
            Attachment::Attached(_) => None,
        });
        let kills = st.ending.iter().map(|e| e.kill_at);
        let exit = match (linger, st.idle_since) {
            (Some(linger), Some(idle)) if st.sessions.is_empty() && st.ending.is_empty() => {
                after(idle, linger)
            }
            _ => None,
        };
        graces.chain(kills).chain(exit).min()
    }
}

fn after(at: Instant, wait: Duration) -> Option<Instant> {
    at.checked_add(wait)
}

fn snapshot(st: &State) -> Snapshot {
    Snapshot {
        sessions: (st.sessions.iter())
            .map(|s| SessionInfo {
                token: s.token,
                destination: s.destination.clone(),
                path: s.path.clone(),
                attached: matches!(s.link, Attachment::Attached(_)),
                groups: s.groups.iter().map(Child::id).collect(),
            })
            .collect(),
        ending: st.ending.iter().map(|e| e.session).collect(),
        reaping: st.reaping.clone(),
        torn_down: st.torn_down.iter().cloned().collect(),
        exited: st.exited,
    }
}

fn record(st: &mut State, done: TornDown) {
    if st.torn_down.len() == TORN_DOWN_KEPT {
        st.torn_down.pop_front();
    }
    st.torn_down.push_back(done);
}

/// Reaps the leaders of `ending`'s groups, which have had their KILL.
fn reaped(ending: Ending) -> TornDown {
    let exits = (ending.groups.into_iter())
        .filter_map(|mut leader| {
            let pid = leader.id();
            match leader.wait() {
                Ok(status) => Some((pid, status)),
                Err(e) => {
                    log!("can't reap process group {pid}: {e}");
                    None
                }
            }
        })
        .collect();
    log!("session {} torn down", hex(ending.session));
    TornDown {
        session: ending.session,
        exits,
    }
}

/// `signal` to the group `leader` leads. One that has gone already is no
/// error: what's left of it is reaped with its leader.
fn signal_group(leader: &Child, signal: libc::c_int) {
    let Ok(pgid) = libc::pid_t::try_from(leader.id()) else {
        return;
    };
    // SAFETY: kill has no memory preconditions. The group's id is the
    // leader's pid, which stays this process's unreaped child.
    unsafe {
        libc::kill(-pgid, signal);
    }
}

/// Answers a request on a channel with no service yet (D § 3: control only
/// in this phase), with the reason as its status.
fn answer(incoming: Incoming) {
    let unsupported = |message: String| {
        Status::Error(IoError {
            kind: IoKind::Unsupported,
            message,
        })
    };
    let status = match incoming.header.chan {
        chan::CONTROL => match incoming.op::<ControlOp>() {
            Ok(ControlOp::Open(OpenTarget::Tcp { host, port })) => unsupported(format!(
                "this tenmoku-server can't open a connection to {host}:{port} yet"
            )),
            Err(e) => Status::Error(IoError {
                kind: IoKind::InvalidData,
                message: e.to_string(),
            }),
        },
        c => {
            let service = match c {
                chan::FS => "file",
                chan::WATCH => "watch",
                chan::SEARCH => "search",
                chan::GIT => "git",
                chan::PROCESS => "process",
                _ => "such",
            };
            unsupported(format!("this tenmoku-server has no {service} service yet"))
        }
    };
    // A request cancelled or a link lost meanwhile needs no answer.
    let _ = incoming.stream.end(status);
}

/// 16 bytes from the system's random source.
fn random() -> io::Result<[u8; 16]> {
    let mut bytes = [0; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// A session token, for the log.
pub(crate) fn hex(token: SessionToken) -> String {
    token.0[..6].iter().map(|b| format!("{b:02x}")).collect()
}
