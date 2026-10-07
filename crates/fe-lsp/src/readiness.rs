//! Readiness (docs/decisions/lsp-client.md §3.4): when a session is
//! `Ready`, which only decides routing, and whether an empty answer from it
//! is final, which is a separate rule.
//!
//! The spec's [`ReadinessMode`] decides before startup:
//!
//! - **`server-status`**: `Ready` at the first `experimental/serverStatus`
//!   with `quiescent: true`; progress never makes the server ready. With
//!   no status at all [`STATUS_WAIT`] after `initialize` answered, the
//!   session falls back to the progress rule for routing, for good.
//! - **`progress`**: `Ready` once `initialize` has answered and no counted
//!   work-done token has been open for [`SETTLE`]. A token matching one of
//!   the spec's `ready_ignores_progress` prefixes doesn't count, and begins
//!   and ends are matched per occurrence, since servers reuse tokens. A
//!   token the server created (`window/workDoneProgress/create`) is open
//!   from then until its begin takes over, or an end.
//!
//! `Ready` latches until the session ends. An empty answer is final only in
//! `server-status` mode, with no fallback, while the latest status says
//! `quiescent: true`, and only if it stayed so from the request's send to
//! its reply ([`Readiness::final_mark`]). A non-empty answer is always
//! final; no timer ever makes an empty one final.
//!
//! The tracker is a plain state machine on times the caller passes in, so
//! its timers run on the injected clock. Every input first applies what
//! time has changed by then ([`Readiness::tick`]), so a status or progress
//! read after a deadline passed, but before the actor's timer fired, finds
//! the deadline's effect already there: the outcome depends on the clock,
//! never on which of the two the thread got to first.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use lsp_types::{NumberOrString, WorkDoneProgress};

use crate::event::ServerStatus;
use crate::launcher::{ReadinessMode, ServerSpec};

/// How long no counted progress may be open before the progress rule
/// makes a session `Ready`.
pub(crate) const SETTLE: Duration = Duration::from_millis(500);
/// How long `server-status` mode waits after `initialize` for a first
/// status before it falls back to the progress rule.
pub(crate) const STATUS_WAIT: Duration = Duration::from_secs(10);
/// The most distinct tokens tracked at once. Begins of further tokens are
/// ignored (and logged): a server that never ends its progress can't grow
/// the table, and an ignored token only risks an early `Ready`, which
/// costs routing, never an answer's finality.
const TOKENS_MAX: usize = 256;

/// One session's readiness.
pub(crate) struct Readiness {
    mode: ReadinessMode,
    ignores: Vec<String>,
    /// When `initialize` answered.
    initialized: Option<Instant>,
    /// Open occurrences of each token.
    open: HashMap<NumberOrString, Open>,
    /// Tokens created and not yet begun, and whether each counts.
    created: HashMap<NumberOrString, bool>,
    /// Since when no counted token has been open, once initialized.
    quiet_since: Option<Instant>,
    /// The latest status's `quiescent`, once one has come.
    quiescent: Option<bool>,
    /// `server-status` mode found no status in time and routes by progress.
    fallback: bool,
    ready: bool,
    /// The fallback happened and the actor hasn't logged it yet.
    unlogged_fallback: bool,
    /// Bumped each time empty answers become final.
    epoch: u64,
    /// Bumped on every begin, to find the latest open title.
    begins: u64,
}

struct Open {
    count: u32,
    counted: bool,
    title: String,
    /// When its latest begin came, in [`Readiness::begins`] order.
    begun: u64,
}

impl Readiness {
    pub(crate) fn new(spec: &ServerSpec) -> Self {
        Self {
            mode: spec.readiness,
            ignores: spec.ready_ignores_progress.clone(),
            initialized: None,
            open: HashMap::new(),
            created: HashMap::new(),
            quiet_since: None,
            quiescent: None,
            fallback: false,
            ready: false,
            unlogged_fallback: false,
            epoch: 0,
            begins: 0,
        }
    }

    /// `initialize` answered at `now`.
    pub(crate) fn initialized(&mut self, now: Instant) {
        self.initialized = Some(now);
        if self.counted_open() == 0 {
            self.quiet_since = Some(now);
        }
    }

    /// The server created `token` (`window/workDoneProgress/create`) at
    /// `now`.
    pub(crate) fn created(&mut self, token: &NumberOrString, now: Instant) {
        self.tick(now);
        if !self.created.contains_key(token) && self.full() {
            log::warn!("more than {TOKENS_MAX} progress tokens open; ignoring {token:?}");
            return;
        }
        let counted = self.counts(token);
        self.created.insert(token.clone(), counted);
        if counted {
            self.quiet_since = None;
        }
    }

    fn full(&self) -> bool {
        self.open.len() + self.created.len() >= TOKENS_MAX
    }

    /// Whether `token` holds back readiness: one whose text starts with an
    /// ignored prefix doesn't.
    fn counts(&self, token: &NumberOrString) -> bool {
        let text = match token {
            NumberOrString::Number(n) => n.to_string(),
            NumberOrString::String(s) => s.clone(),
        };
        !self.ignores.iter().any(|prefix| text.starts_with(prefix))
    }

    /// A `$/progress` for `token` came at `now`.
    pub(crate) fn progress(
        &mut self,
        token: &NumberOrString,
        value: &WorkDoneProgress,
        now: Instant,
    ) {
        self.tick(now);
        match value {
            WorkDoneProgress::Begin(begin) => {
                let known = self.open.contains_key(token) || self.created.remove(token).is_some();
                if !known && self.full() {
                    log::warn!("more than {TOKENS_MAX} progress tokens open; ignoring {token:?}");
                    return;
                }
                self.begins += 1;
                let counted = self.counts(token);
                let open = self.open.entry(token.clone()).or_insert(Open {
                    count: 0,
                    counted,
                    title: String::new(),
                    begun: 0,
                });
                open.count += 1;
                open.title.clone_from(&begin.title);
                open.begun = self.begins;
                if counted {
                    self.quiet_since = None;
                }
            }
            WorkDoneProgress::Report(_) => {}
            WorkDoneProgress::End(_) => {
                // An occurrence begun ends first; else one only created. An
                // end with neither is ignored.
                if let Some(open) = self.open.get_mut(token) {
                    open.count -= 1;
                    if open.count == 0 {
                        self.open.remove(token);
                    }
                } else if self.created.remove(token).is_none() {
                    return;
                }
                if self.initialized.is_some()
                    && self.quiet_since.is_none()
                    && self.counted_open() == 0
                {
                    self.quiet_since = Some(now);
                }
            }
        }
    }

    /// An `experimental/serverStatus` came at `now`. Whether it applies:
    /// only in `server-status` mode.
    pub(crate) fn status(&mut self, quiescent: bool, now: Instant) -> bool {
        if self.mode != ReadinessMode::ServerStatus {
            return false;
        }
        self.tick(now);
        if quiescent && self.quiescent != Some(true) {
            self.epoch += 1;
        }
        self.quiescent = Some(quiescent);
        if quiescent && !self.fallback {
            self.ready = true;
        }
        true
    }

    /// Applies what time alone changes, as of `now`.
    pub(crate) fn tick(&mut self, now: Instant) {
        if let Some(due) = self.fallback_due()
            && now >= due
        {
            self.fallback = true;
            self.unlogged_fallback = true;
        }
        if let Some(due) = self.settle_due()
            && now >= due
        {
            self.ready = true;
        }
    }

    /// Whether the session fell back to the progress rule since this was
    /// last asked.
    pub(crate) fn take_fallback(&mut self) -> bool {
        std::mem::take(&mut self.unlogged_fallback)
    }

    /// When [`tick`](Self::tick) next has something to do.
    pub(crate) fn next_due(&self) -> Option<Instant> {
        match (self.fallback_due(), self.settle_due()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn fallback_due(&self) -> Option<Instant> {
        let pending =
            self.mode == ReadinessMode::ServerStatus && !self.fallback && self.quiescent.is_none();
        pending.then(|| self.initialized.map(|at| at + STATUS_WAIT))?
    }

    /// When the progress rule makes the session `Ready`, if it applies.
    fn settle_due(&self) -> Option<Instant> {
        let by_progress = self.mode == ReadinessMode::Progress || self.fallback;
        if self.ready || !by_progress {
            return None;
        }
        self.quiet_since.map(|since| since + SETTLE)
    }

    fn counted_open(&self) -> usize {
        let created = self.created.values().filter(|&&counted| counted).count();
        self.open.values().filter(|open| open.counted).count() + created
    }

    /// What the status shows: `Ready` once latched; before that,
    /// `Indexing` while counted progress is open, with the title of the
    /// latest begun, else `Starting`.
    pub(crate) fn view(&self) -> ServerStatus {
        if self.ready {
            return ServerStatus::Ready;
        }
        self.open
            .values()
            .filter(|open| open.counted)
            .max_by_key(|open| open.begun)
            .map_or(ServerStatus::Starting, |open| ServerStatus::Indexing {
                title: open.title.clone(),
            })
    }

    /// A mark to check a reply against, if an empty answer to a request
    /// sent now would be final: `server-status` mode, no fallback, and
    /// the latest status `quiescent: true`.
    pub(crate) fn final_mark(&self) -> Option<u64> {
        let quiescent = self.mode == ReadinessMode::ServerStatus
            && !self.fallback
            && self.quiescent == Some(true);
        quiescent.then_some(self.epoch)
    }

    /// Whether an empty answer is final, for a request sent with `mark`:
    /// it still would be, and the server hasn't been non-quiescent since.
    pub(crate) fn is_final(&self, mark: Option<u64>) -> bool {
        mark.is_some() && self.final_mark() == mark
    }
}
