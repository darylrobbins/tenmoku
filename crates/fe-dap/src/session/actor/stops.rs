// SPDX-License-Identifier: Apache-2.0

//! Stops and resumes (docs/decisions/r1b9dw3g0g-debugger.md §2 items 4-5,
//! §13 F2; card D2b): the stop epoch, the lazy fetch on each stop, the
//! pending resume and its guard, Uncertain and its probe, and Pause.

use std::collections::BTreeSet;
use std::time::Instant;

use futures::channel::oneshot;
use serde_json::Value;

use super::{Actor, Pending, Purpose, Timer, parse};
use crate::protocol::{
    ContinueArguments, ContinueBody, ContinuedBody, Empty, PauseArguments, Request,
    ScopesArguments, ScopesBody, StackTraceArguments, StackTraceBody, StepArguments,
    StepInArguments, StoppedBody, Thread, ThreadsBody, VariablesArguments, VariablesBody, request,
};
use crate::session::{
    DapEvent, Epoch, FRAME_PAGE, RequestError, Resume, SessionState, StopData, StopRef,
    is_frameless_evaluate, is_stop_data, lock, to_value,
};

/// The adapter's short `message` for a request refused because the
/// debuggee isn't stopped: it is running.
const NOT_STOPPED: &str = "notStopped";

/// The session-long state of stops.
#[derive(Default)]
pub(super) struct Stops {
    pub(super) epoch: Epoch,
    /// At most one; a newer one replaces it.
    resume: Option<PendingResume>,
    /// From the latest `stopped` (or proven stop) naming one: Pause's and
    /// a resume's default thread, while a `threads` reply still lists it.
    last_stopped_thread: Option<i64>,
    /// The current stop's data, while Stopped.
    stop: Option<StopData>,
    /// Which threads are suspended (§2 item 4's `allThreadsStopped` and
    /// `allThreadsContinued`).
    suspended: Suspended,
    /// The newest thread update applied, as the `seq` the client had used
    /// by then: a `threads` reply to a request sent before it is older,
    /// and dropped.
    threads_from: i64,
}

impl Stops {
    /// A new run (a new adapter, a restart, a shutdown): nothing is
    /// pending, stopped or remembered.
    pub(super) fn new_run(&mut self) {
        self.resume = None;
        self.last_stopped_thread = None;
        self.suspended = Suspended::None;
    }

    /// A new adapter: its `seq`s start again.
    pub(super) fn new_adapter(&mut self) {
        self.new_run();
        self.threads_from = 0;
    }

    /// The thread list changed (a `thread` event, a restart) once the
    /// client had used `seq`: replies to earlier `threads` are out of date.
    pub(super) fn threads_changed(&mut self, seq: i64) {
        self.threads_from = self.threads_from.max(seq);
    }

    /// A `thread` event said `thread_id` exited.
    pub(super) fn thread_exited(&mut self, thread_id: i64) {
        if self.last_stopped_thread == Some(thread_id) {
            self.last_stopped_thread = None;
        }
        self.suspended.remove(thread_id);
        if let Some(resume) = &mut self.resume {
            resume.suspended_before.remove(thread_id);
        }
    }
}

/// Which threads are suspended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Suspended {
    #[default]
    None,
    /// Every thread, whichever are known.
    All,
    Threads(BTreeSet<i64>),
}

impl Suspended {
    fn any(&self) -> bool {
        *self != Suspended::None
    }

    fn contains(&self, thread_id: i64) -> bool {
        match self {
            Suspended::None => false,
            Suspended::All => true,
            Suspended::Threads(threads) => threads.contains(&thread_id),
        }
    }

    /// A suspended thread, of `known` for [`Suspended::All`].
    fn first(&self, known: &[Thread]) -> Option<i64> {
        match self {
            Suspended::None => None,
            Suspended::All => known.first().map(|thread| thread.id),
            Suspended::Threads(threads) => threads.first().copied(),
        }
    }

    /// A `stopped` event: its thread, or all of them.
    fn stopped(&mut self, body: &StoppedBody) {
        match (body.all_threads_stopped, body.thread_id) {
            (Some(true), _) | (_, None) => *self = Suspended::All,
            (_, Some(thread_id)) => match self {
                Suspended::All => {}
                Suspended::Threads(threads) => {
                    threads.insert(thread_id);
                }
                Suspended::None => *self = Suspended::Threads(BTreeSet::from([thread_id])),
            },
        }
    }

    /// `thread_id` resumed (or, `all`, every thread did). With every
    /// thread suspended, the ones left are the `known` others.
    fn resumed(&mut self, thread_id: Option<i64>, all: bool, known: &[Thread]) {
        let Some(thread_id) = thread_id.filter(|_| !all) else {
            *self = Suspended::None;
            return;
        };
        if *self == Suspended::All {
            *self = Suspended::Threads(known.iter().map(|thread| thread.id).collect());
        }
        self.remove(thread_id);
    }

    fn remove(&mut self, thread_id: i64) {
        if let Suspended::Threads(threads) = self {
            threads.remove(&thread_id);
            if threads.is_empty() {
                *self = Suspended::None;
            }
        }
    }

    /// The program listed `threads`: the others are gone.
    fn listed(&mut self, threads: &[Thread]) {
        if let Suspended::Threads(suspended) = self {
            suspended.retain(|id| threads.iter().any(|thread| thread.id == *id));
            if suspended.is_empty() {
                *self = Suspended::None;
            }
        }
    }
}

/// The pending resume: its `seq` and the epoch it created.
struct PendingResume {
    seq: i64,
    command: String,
    epoch: Epoch,
    /// The thread it resumed, for Uncertain's probe and a single-thread
    /// `continue`.
    thread_id: Option<i64>,
    /// The threads suspended when it was sent: those a single-thread
    /// `continue` leaves stopped.
    suspended_before: Suspended,
    /// It went unanswered: the session is Uncertain because of it, and its
    /// late reply still counts.
    timed_out: bool,
    sent_at: Instant,
}

/// A step of the fetch on a stop (§2 item 5).
#[derive(Debug)]
pub(super) enum Fetch {
    Threads,
    Frames { thread_id: i64 },
    Scopes { frame_id: i64 },
    Variables { reference: i64 },
}

/// A step of Uncertain's probe: `threads`, then `stackTrace` on the
/// resumed thread (or the first listed).
#[derive(Debug)]
pub(super) enum Probe {
    Threads { thread_id: Option<i64> },
    Frames { thread_id: i64 },
}

/// Whether `command` resumes the debuggee (§2 item 4's list, and the
/// reverse ones).
pub(super) fn is_resume(command: &str) -> bool {
    matches!(
        command,
        "continue"
            | "next"
            | "stepIn"
            | "stepOut"
            | "goto"
            | "restartFrame"
            | "stepBack"
            | "reverseContinue"
    )
}

/// Whether a resume may go from Uncertain: the ones that need no stopped
/// frame.
fn resumes_from_uncertain(command: &str) -> bool {
    matches!(command, "continue" | "reverseContinue")
}

fn is_not_stopped(err: &RequestError) -> bool {
    err.message() == Some(NOT_STOPPED)
}

impl Actor {
    // The epoch.

    /// Moves to a new epoch: the stop's data is retired, and no resume or
    /// probe is pending.
    pub(super) fn advance_epoch(&mut self) {
        self.s.epoch = self.s.epoch.next();
        self.s.resume = None;
        self.s.stop = None;
        self.publish_stop();
        // The host's requests about the old stop are answered now, and
        // their replies, when they come, dropped.
        let stale: Vec<i64> = self
            .g
            .pending
            .iter()
            .filter(|(_, pending)| {
                matches!(pending.purpose, Purpose::External { epoch: Some(_), .. })
            })
            .map(|(seq, _)| *seq)
            .collect();
        for seq in stale {
            let Some(pending) = self.g.pending.remove(&seq) else {
                continue;
            };
            self.g.timers.cancel(Timer::Request(seq));
            if let Purpose::External { token, reply, .. } = pending.purpose {
                self.g.tokens.remove(&token);
                let _ = reply.send(Err(RequestError::Stale {
                    command: pending.command,
                }));
            }
        }
    }

    fn publish_stop(&self) {
        let mut view = lock(&self.shared.view);
        view.epoch = self.s.epoch;
        view.stop = self.s.stop.clone();
    }

    /// A reply to `threads` request `seq`, sent at `epoch`: the program's
    /// own list, unless the epoch has moved since (a stop, a resume, a new
    /// run) or a newer update came first. Whether it was applied.
    pub(super) fn threads_answered(
        &mut self,
        seq: i64,
        epoch: Epoch,
        threads: Vec<Thread>,
    ) -> bool {
        if epoch != self.s.epoch || seq <= self.s.threads_from {
            log::debug!("dropped a threads reply older than the list ({seq})");
            return false;
        }
        self.s.threads_from = seq;
        // Thread events are optional: a thread the list leaves out is gone.
        if let Some(last) = self.s.last_stopped_thread
            && !threads.iter().any(|thread| thread.id == last)
        {
            self.s.last_stopped_thread = None;
        }
        self.s.suspended.listed(&threads);
        if let Some(resume) = &mut self.s.resume {
            resume.suspended_before.listed(&threads);
        }
        lock(&self.shared.view).threads_answered = true;
        self.set_threads(threads);
        true
    }

    /// The thread a request goes to when none is named: the one that last
    /// stopped while it is suspended, another suspended one, the one that
    /// last stopped, or the first known.
    fn default_thread(&self) -> Option<i64> {
        let suspended = &self.s.suspended;
        let last = self.s.last_stopped_thread;
        last.filter(|thread_id| suspended.contains(*thread_id))
            .or_else(|| suspended.first(&self.threads))
            .or(last)
            .or_else(|| self.threads.first().map(|thread| thread.id))
    }

    /// The stop data a request carries ids of, which is the current stop's
    /// (`epoch` is the caller's, if any): stop data is asked for only while
    /// Stopped, of a suspended thread, apart from a frameless `evaluate`
    /// outside Resuming and Uncertain. `Err` when it may not go.
    pub(super) fn stop_data_epoch(
        &self,
        command: &str,
        arguments: &Value,
        epoch: Option<Epoch>,
    ) -> Result<Option<Epoch>, ()> {
        if !is_stop_data(command) {
            return Ok(epoch);
        }
        match self.state {
            SessionState::Stopped => {
                let thread = arguments.get("threadId").and_then(Value::as_i64);
                if command == request::StackTrace::COMMAND
                    && thread.is_some_and(|thread_id| !self.s.suspended.contains(thread_id))
                {
                    return Err(());
                }
                Ok(Some(epoch.unwrap_or(self.s.epoch)))
            }
            SessionState::Resuming | SessionState::Uncertain => Err(()),
            // A frameless evaluation while running, which some adapters
            // support: about no stop.
            _ if is_frameless_evaluate(command, arguments) => Ok(None),
            _ => Err(()),
        }
    }

    /// A suspended thread to show a stop of: the one that last stopped, if
    /// it still is, else another.
    fn suspended_thread(&self) -> Option<i64> {
        let suspended = &self.s.suspended;
        self.s
            .last_stopped_thread
            .filter(|thread_id| suspended.contains(*thread_id))
            .or_else(|| suspended.first(&self.threads))
    }

    /// Some threads are still suspended after a resume or `continued`: a
    /// stop at the current epoch, on one of them, fetched afresh.
    fn retained_stop(&mut self) {
        let thread_id = self.suspended_thread();
        self.begin_stop(None, thread_id);
        self.set_state(SessionState::Stopped);
        self.fetch();
    }

    /// No thread is suspended any more: the stop is over.
    fn end_stop(&mut self) {
        self.advance_epoch();
        self.set_state(SessionState::Running);
    }

    /// A `thread` event said `thread_id` exited. A stop of it moves to
    /// another suspended thread, at a new epoch; with none, it is over.
    pub(super) fn thread_gone(&mut self, thread_id: i64) {
        if self.state != SessionState::Stopped || self.g.shutdown.is_some() {
            return;
        }
        if !self.s.suspended.any() {
            return self.end_stop();
        }
        let shown = self.s.stop.as_ref().and_then(|stop| stop.thread_id);
        if shown == Some(thread_id) {
            self.advance_epoch();
            self.retained_stop();
        }
    }

    // Events.

    /// `stopped`: a new epoch, any pending resume cleared, and the fetch.
    pub(super) fn stopped(&mut self, body: StoppedBody) {
        self.advance_epoch();
        self.s.suspended.stopped(&body);
        if let Some(thread_id) = body.thread_id {
            self.s.last_stopped_thread = Some(thread_id);
        }
        let thread_id = body.thread_id;
        self.begin_stop(Some(body.clone()), thread_id);
        self.set_state(SessionState::Stopped);
        self.emit(DapEvent::Stopped(body));
        self.fetch();
    }

    /// `continued`: running, at a new epoch, any pending resume cleared;
    /// or, if it names one thread and others stay suspended, a stop on
    /// those at the new epoch.
    pub(super) fn continued(&mut self, body: ContinuedBody) {
        // Before a pending resume's answer nothing is known to be
        // suspended: the threads a `continue` left are its snapshot's (a
        // step runs them all).
        let mut suspended = match self.s.resume.take() {
            Some(resume) if resume.command == request::Continue::COMMAND => resume.suspended_before,
            Some(_) => Suspended::None,
            None => std::mem::take(&mut self.s.suspended),
        };
        self.advance_epoch();
        let all = body.all_threads_continued != Some(false);
        suspended.resumed(Some(body.thread_id), all, &self.threads);
        self.s.suspended = suspended;
        if self.s.suspended.any() {
            self.retained_stop();
        } else {
            self.set_state(SessionState::Running);
        }
        self.emit(DapEvent::Continued(body));
    }

    // Resumes.

    /// The user's resume, asked at `epoch`. It goes only if that stop (or
    /// Uncertain) is still the current one.
    pub(super) fn resume_command(&mut self, resume: Resume, thread_id: Option<i64>, epoch: Epoch) {
        let command = resume.command();
        let allowed = self.g.shutdown.is_none()
            && epoch == self.s.epoch
            && match self.state {
                SessionState::Stopped => true,
                SessionState::Uncertain => resumes_from_uncertain(command),
                _ => false,
            };
        let thread_id = thread_id.or_else(|| self.default_thread());
        match (allowed, thread_id) {
            (true, Some(thread_id)) => {
                let arguments = match resume {
                    Resume::Continue => to_value(&ContinueArguments {
                        thread_id,
                        ..Default::default()
                    }),
                    Resume::Next | Resume::StepOut => to_value(&StepArguments {
                        thread_id,
                        ..Default::default()
                    }),
                    Resume::StepIn => to_value(&StepInArguments {
                        thread_id,
                        ..Default::default()
                    }),
                };
                self.begin_resume(
                    command,
                    arguments,
                    Some(thread_id),
                    Purpose::Resume {
                        token: None,
                        reply: None,
                    },
                );
            }
            (true, None) => log::info!("{command} ignored: no thread is known"),
            (false, _) => log::info!("{command} ignored: the stop it was asked at is over"),
        }
        lock(&self.shared.view).resume_requested = false;
    }

    /// A resume the host sent as a raw request: the pending resume like
    /// the user's, its reply the host's too.
    pub(super) fn raw_resume(
        &mut self,
        token: u64,
        command: String,
        arguments: Value,
        reply: oneshot::Sender<Result<Value, RequestError>>,
    ) {
        let allowed = match self.state {
            SessionState::Stopped => true,
            SessionState::Uncertain => resumes_from_uncertain(&command),
            _ => false,
        };
        if !allowed {
            let _ = reply.send(Err(RequestError::NotStopped { command }));
            return;
        }
        let thread_id = arguments.get("threadId").and_then(Value::as_i64);
        self.begin_resume(
            &command,
            arguments,
            thread_id,
            Purpose::Resume {
                token: Some(token),
                reply: Some(reply),
            },
        );
    }

    /// Sends a resume: the epoch moves on at once, so the stop's data is
    /// retired before any reply, and it is the pending resume.
    fn begin_resume(
        &mut self,
        command: &str,
        arguments: Value,
        thread_id: Option<i64>,
        purpose: Purpose,
    ) {
        // Nothing is known to be suspended until its answer.
        let suspended_before = std::mem::take(&mut self.s.suspended);
        self.advance_epoch();
        let seq = self.send(command, arguments, purpose);
        self.s.resume = Some(PendingResume {
            seq,
            command: command.to_owned(),
            epoch: self.s.epoch,
            thread_id,
            suspended_before,
            timed_out: false,
            sent_at: self.clock.now(),
        });
        self.set_state(SessionState::Resuming);
    }

    /// The resume guard: a completion applies only to the pending resume,
    /// at the epoch it created, while Resuming, or Uncertain by its own
    /// timeout, and never once shutting down.
    fn is_current_resume(&self, seq: i64) -> bool {
        let Some(resume) = &self.s.resume else {
            return false;
        };
        self.g.shutdown.is_none()
            && resume.seq == seq
            && resume.epoch == self.s.epoch
            && match self.state {
                SessionState::Resuming => true,
                SessionState::Uncertain => resume.timed_out,
                _ => false,
            }
    }

    /// A resume's success, failure or timeout.
    pub(super) fn resume_answered(&mut self, seq: i64, result: Result<Value, RequestError>) {
        if !self.is_current_resume(seq) {
            log::debug!("ignored a stale resume completion ({seq})");
            return;
        }
        match result {
            // Running on the response: `continued` comes only for resumes
            // the client didn't ask for. A `continue` answered
            // `allThreadsContinued: false` ran only its thread; every other
            // resume runs them all (v1 never sends `singleThread`).
            Ok(body) => {
                let Some(resume) = self.s.resume.take() else {
                    return;
                };
                let all = resume.command != request::Continue::COMMAND
                    || parse::<ContinueBody>(body).and_then(|body| body.all_threads_continued)
                        != Some(false);
                let mut left = resume.suspended_before;
                left.resumed(resume.thread_id, all, &self.threads);
                self.s.suspended = left;
                if self.s.suspended.any() {
                    self.retained_stop();
                } else {
                    self.set_state(SessionState::Running);
                }
            }
            // The program may be running or still stopped. The request
            // stays pending: its late reply is the expected way out.
            Err(err @ RequestError::Timeout { .. }) => {
                if let Some(resume) = &mut self.s.resume {
                    resume.timed_out = true;
                }
                self.cancel_request(seq);
                self.emit(DapEvent::Failed(err));
                self.set_state(SessionState::Uncertain);
            }
            // `notStopped`: the program is running.
            Err(err) if is_not_stopped(&err) => {
                self.s.resume = None;
                self.emit(DapEvent::Failed(err));
                self.set_state(SessionState::Running);
            }
            // §13 F2: a failure, `cancelled` included, isn't proof the
            // program stayed stopped. Uncertain until a stop is proven.
            Err(err) => {
                let thread_id = self.s.resume.take().and_then(|resume| resume.thread_id);
                self.emit(DapEvent::Failed(err));
                self.set_state(SessionState::Uncertain);
                self.probe(thread_id);
            }
        }
    }

    /// The pending resume, if `seq` is it and it had timed out: its late
    /// reply is still its completion.
    pub(super) fn late_resume(&self, seq: i64, command: &str) -> Option<Pending> {
        let resume = self.s.resume.as_ref()?;
        (resume.seq == seq && resume.timed_out).then(|| Pending {
            command: command.to_owned(),
            purpose: Purpose::Resume {
                token: None,
                reply: None,
            },
            sent_at: resume.sent_at,
            timeout: None,
        })
    }

    // Uncertain's probe (§13 F2).

    fn probe(&mut self, thread_id: Option<i64>) {
        let epoch = self.s.epoch;
        self.send(
            request::Threads::COMMAND,
            to_value(&Empty::default()),
            Purpose::Probe {
                epoch,
                what: Probe::Threads { thread_id },
            },
        );
    }

    pub(super) fn probe_answered(
        &mut self,
        seq: i64,
        epoch: Epoch,
        what: Probe,
        result: Result<Value, RequestError>,
    ) {
        if epoch != self.s.epoch
            || self.state != SessionState::Uncertain
            || self.g.shutdown.is_some()
        {
            log::debug!("ignored a probe reply from before a change");
            return;
        }
        let body = match result {
            Ok(body) => body,
            Err(err) if is_not_stopped(&err) => {
                return self.set_state(SessionState::Running);
            }
            Err(_) => {
                log::info!("the probe for a stop failed: the session stays uncertain");
                return;
            }
        };
        match what {
            Probe::Threads { thread_id } => {
                let threads = parse::<ThreadsBody>(body)
                    .map(|body| body.threads)
                    .unwrap_or_default();
                self.threads_answered(seq, epoch, threads);
                let threads = &self.threads;
                let target = thread_id
                    .filter(|wanted| threads.iter().any(|thread| thread.id == *wanted))
                    .or_else(|| threads.first().map(|thread| thread.id));
                let Some(thread_id) = target else {
                    log::info!("the probe found no thread: the session stays uncertain");
                    return;
                };
                let arguments = StackTraceArguments {
                    thread_id,
                    levels: Some(1),
                    ..Default::default()
                };
                self.send(
                    request::StackTrace::COMMAND,
                    to_value(&arguments),
                    Purpose::Probe {
                        epoch,
                        what: Probe::Frames { thread_id },
                    },
                );
            }
            // A stack proves a stop only from an adapter that gives one
            // for suspended threads alone, and only a stack with frames.
            // Then fetch afresh under a new epoch; the probe's frame isn't
            // kept.
            Probe::Frames { thread_id } => {
                let frames = parse::<StackTraceBody>(body)
                    .map(|body| body.stack_frames)
                    .unwrap_or_default();
                if frames.is_empty() {
                    log::info!("the probe got no stack: the session stays uncertain");
                    return;
                }
                if !self.options.stack_proves_stop {
                    log::info!(
                        "debug adapter {}'s stack doesn't prove a stop: uncertain until one is reported",
                        self.options.adapter.name
                    );
                    return;
                }
                self.advance_epoch();
                self.s.suspended = Suspended::Threads(BTreeSet::from([thread_id]));
                self.s.last_stopped_thread = Some(thread_id);
                self.begin_stop(None, Some(thread_id));
                self.set_state(SessionState::Stopped);
                self.fetch();
            }
        }
    }

    // The fetch on a stop (§2 item 5).

    /// The new stop's (empty) data, published before the state says
    /// Stopped.
    fn begin_stop(&mut self, stopped: Option<StoppedBody>, thread_id: Option<i64>) {
        self.s.stop = Some(StopData {
            epoch: self.s.epoch,
            stopped,
            thread_id,
            ..Default::default()
        });
        self.publish_stop();
    }

    /// The fetch's first step: `threads`, each stop.
    fn fetch(&mut self) {
        self.send_fetch(
            request::Threads::COMMAND,
            to_value(&Empty::default()),
            Fetch::Threads,
        );
    }

    fn send_fetch(&mut self, command: &str, arguments: Value, what: Fetch) {
        let epoch = self.s.epoch;
        self.send(command, arguments, Purpose::Fetch { epoch, what });
    }

    pub(super) fn fetch_answered(
        &mut self,
        seq: i64,
        epoch: Epoch,
        what: Fetch,
        result: Result<Value, RequestError>,
    ) {
        // A reply from another stop never fills this one's data.
        if epoch != self.s.epoch || self.state != SessionState::Stopped || self.g.shutdown.is_some()
        {
            log::debug!("dropped a stop-data reply from another epoch: {what:?}");
            return;
        }
        let body = match result {
            Ok(body) => body,
            Err(_) => {
                // The payload log has the reply itself, redacted.
                log::warn!("fetching the stop's data failed at {what:?}");
                return;
            }
        };
        match what {
            Fetch::Threads => {
                let threads = parse::<ThreadsBody>(body)
                    .map(|body| body.threads)
                    .unwrap_or_default();
                self.threads_answered(seq, epoch, threads);
                // The stop's thread, while suspended and listed (an empty
                // list leaves it be); else another suspended one; with
                // none left, the stop is over.
                let listed = |thread_id: i64| {
                    self.threads.is_empty()
                        || self.threads.iter().any(|thread| thread.id == thread_id)
                };
                let stopped =
                    self.s
                        .stop
                        .as_ref()
                        .and_then(|stop| stop.thread_id)
                        .filter(|thread_id| {
                            self.s.suspended.contains(*thread_id) && listed(*thread_id)
                        });
                if !self.s.suspended.any() {
                    log::info!("the stopped threads are gone: the stop is over");
                    return self.end_stop();
                }
                let Some(thread_id) = stopped.or_else(|| self.suspended_thread()) else {
                    log::info!("the stop names no thread and the program listed none");
                    return;
                };
                if let Some(stop) = &mut self.s.stop {
                    stop.thread_id = Some(thread_id);
                }
                self.publish_stop();
                let arguments = StackTraceArguments {
                    thread_id,
                    start_frame: Some(0),
                    levels: Some(FRAME_PAGE),
                    ..Default::default()
                };
                self.send_fetch(
                    request::StackTrace::COMMAND,
                    to_value(&arguments),
                    Fetch::Frames { thread_id },
                );
            }
            Fetch::Frames { thread_id } => {
                let body = parse::<StackTraceBody>(body).unwrap_or_default();
                let top = body.stack_frames.first().map(|frame| frame.id);
                if let Some(stop) = &mut self.s.stop {
                    stop.frames = body.stack_frames.clone();
                    stop.total_frames = body.total_frames;
                }
                self.publish_stop();
                self.emit(DapEvent::Frames {
                    epoch,
                    thread_id,
                    frames: body.stack_frames,
                    total_frames: body.total_frames,
                });
                if let Some(frame_id) = top {
                    let arguments = ScopesArguments {
                        frame_id,
                        ..Default::default()
                    };
                    self.send_fetch(
                        request::Scopes::COMMAND,
                        to_value(&arguments),
                        Fetch::Scopes { frame_id },
                    );
                }
            }
            Fetch::Scopes { frame_id } => {
                let scopes = parse::<ScopesBody>(body)
                    .map(|body| body.scopes)
                    .unwrap_or_default();
                if let Some(stop) = &mut self.s.stop {
                    stop.scopes = scopes.clone();
                }
                self.publish_stop();
                self.emit(DapEvent::Scopes {
                    frame: StopRef::new(epoch, frame_id),
                    scopes: scopes.clone(),
                });
                // Expensive scopes, and everything deeper, wait for the UI.
                for scope in scopes {
                    if scope.expensive || scope.variables_reference <= 0 {
                        continue;
                    }
                    let arguments = VariablesArguments {
                        variables_reference: scope.variables_reference,
                        ..Default::default()
                    };
                    self.send_fetch(
                        request::Variables::COMMAND,
                        to_value(&arguments),
                        Fetch::Variables {
                            reference: scope.variables_reference,
                        },
                    );
                }
            }
            Fetch::Variables { reference } => {
                let variables = parse::<VariablesBody>(body)
                    .map(|body| body.variables)
                    .unwrap_or_default();
                if let Some(stop) = &mut self.s.stop {
                    stop.variables.insert(reference, variables.clone());
                }
                self.publish_stop();
                self.emit(DapEvent::Variables {
                    reference: StopRef::new(epoch, reference),
                    variables,
                });
            }
        }
    }

    // Pause.

    /// The user's Pause. It changes no state: the `stopped` that follows
    /// does.
    pub(super) fn pause_command(&mut self, thread_id: Option<i64>) {
        let may_run = matches!(
            self.state,
            SessionState::Running | SessionState::Resuming | SessionState::Uncertain
        );
        if self.g.shutdown.is_some() || !may_run {
            log::info!("pause ignored: the program isn't running");
            return;
        }
        match thread_id.or_else(|| self.default_thread()) {
            Some(thread_id) => self.send_pause(thread_id),
            // No thread known: ask for them first.
            None => {
                let epoch = self.s.epoch;
                self.send(
                    request::Threads::COMMAND,
                    to_value(&Empty::default()),
                    Purpose::PauseThreads { epoch },
                );
            }
        }
    }

    fn send_pause(&mut self, thread_id: i64) {
        let arguments = PauseArguments {
            thread_id,
            ..Default::default()
        };
        self.send(
            request::Pause::COMMAND,
            to_value(&arguments),
            Purpose::Pause,
        );
    }

    /// The `threads` a Pause sent first. Its list counts if it is the
    /// newest; the pause goes only if nothing changed since it was asked.
    pub(super) fn pause_threads_answered(
        &mut self,
        seq: i64,
        epoch: Epoch,
        result: Result<Value, RequestError>,
    ) {
        let current = epoch == self.s.epoch
            && self.g.shutdown.is_none()
            && matches!(
                self.state,
                SessionState::Running | SessionState::Resuming | SessionState::Uncertain
            );
        if !current {
            log::info!("pause given up: the session changed while finding a thread");
            return;
        }
        match result {
            Ok(body) => {
                let threads = parse::<ThreadsBody>(body)
                    .map(|body| body.threads)
                    .unwrap_or_default();
                self.threads_answered(seq, epoch, threads);
                match self.default_thread() {
                    Some(thread_id) => self.send_pause(thread_id),
                    // The controls now say why Pause is off.
                    None => log::info!("pause: the program reported no threads"),
                }
            }
            Err(err) => self.emit(DapEvent::Failed(err)),
        }
    }
}
