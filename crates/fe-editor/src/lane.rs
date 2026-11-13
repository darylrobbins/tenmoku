//! The headless lane model (docs/designs/agent-lanes.md § Identity,
//! § Lifecycle): which agent wrote which hunks, and where its work stands.
//!
//! No I/O and no GPUI. The model never counts hunks or reads a worktree
//! itself: every transition takes the lane's pending-hunk count
//! ([`Hunks`]) and, for worktree lanes, the tree id of the live worktree
//! ([`TreeId`]) as inputs.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::SystemTime;

/// A lane's number: "lane 2". Given in order of creation, never reused
/// while the workspace's lane record lasts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LaneId(NonZeroU32);

impl LaneId {
    /// The number as shown, starting at 1.
    pub fn get(self) -> u32 {
        self.0.get()
    }

    pub fn from_number(number: u32) -> Option<LaneId> {
        NonZeroU32::new(number).map(LaneId)
    }
}

/// The id of a worktree's tree, compared for equality only.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TreeId(pub Arc<str>);

/// What kind of agent owns the lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaneKind {
    /// The chat (lane 1): persistent, never closes.
    Chat,
    /// An agent outside the editor, reached through `host` ("acp", a plugin
    /// host); it signals its turns itself (`lane.finish`).
    External { host: Arc<str> },
    /// A terminal agent working in its own worktree; its turns end when the
    /// worktree is frozen.
    Worktree,
}

/// Where the lane's edits land (§ Isolation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backing {
    Shared,
    Worktree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LaneState {
    /// The agent is producing; it may already have pending hunks.
    Working,
    /// Its turn is over and it has undecided hunks.
    PendingReview,
    /// Every hunk decided, at least one accepted.
    Accepted,
    /// Every hunk decided, none accepted.
    Rejected,
    /// Ended (closed, disconnected, exited) with hunks undecided.
    Abandoned,
}

/// A lane's hunks, as the caller counts them: every undecided hunk it owns
/// (`Pending`, and `Stale` until rejected), and whether any was accepted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hunks {
    pub pending: usize,
    pub any_accepted: bool,
}

impl Hunks {
    /// The state of a lane whose hunks are all decided.
    fn decided(self) -> LaneState {
        if self.any_accepted {
            LaneState::Accepted
        } else {
            LaneState::Rejected
        }
    }

    /// PendingReview with hunks undecided, else Accepted or Rejected.
    fn settled(self) -> LaneState {
        if self.pending > 0 {
            LaneState::PendingReview
        } else {
            self.decided()
        }
    }
}

/// What happens to a lane. Each is a row (or the rows) of the lifecycle
/// table; [`LaneState::next`] is the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaneEvent {
    /// The turn ends: `lane.finish`, the reply finishing, failing or being
    /// stopped, a worktree frozen.
    TurnEnded,
    /// A new turn starts: request changes or a review comment sent as the
    /// next instruction, the chat's next send, the agent starting by itself,
    /// a persistent lane or reconnected agent starting another turn.
    TurnStarted,
    /// Hunks added outside a turn (Apply or Review Changes on a finished
    /// reply, a worktree lane frozen again).
    HunksAdded,
    /// A hunk was decided (accepted or rejected).
    Decided,
    /// The lane ended: closed, disconnected, exited.
    Ended,
    /// The same agent reopens the same session (`lane.open` or ACP
    /// `session/load`) before the lane is cleared.
    Reopened,
}

impl LaneState {
    /// The table of § Lifecycle: the state `event` leads to from `self`, or
    /// `None` when the row does not apply (the lane stays as it is).
    pub fn next(self, event: &LaneEvent, hunks: Hunks) -> Option<LaneState> {
        use LaneState::*;
        match (self, event) {
            (Working, LaneEvent::TurnEnded) => Some(hunks.settled()),
            (PendingReview | Accepted | Rejected, LaneEvent::TurnStarted) => Some(Working),
            (Accepted | Rejected, LaneEvent::HunksAdded) if hunks.pending > 0 => {
                Some(PendingReview)
            }
            (PendingReview | Abandoned, LaneEvent::Decided) if hunks.pending == 0 => {
                Some(hunks.decided())
            }
            (Working | PendingReview, LaneEvent::Ended) => Some(if hunks.pending > 0 {
                Abandoned
            } else {
                hunks.decided()
            }),
            (Abandoned, LaneEvent::Reopened) => Some(Working),
            _ => None,
        }
    }

    /// A restored lane's state, reconciled before anything reads it
    /// (§ Lifecycle › Restart); `self` is the state saved.
    pub fn reconciled(self, kind: &LaneKind, hunks: Hunks) -> LaneState {
        use LaneState::*;
        match kind {
            LaneKind::Chat => hunks.settled(),
            LaneKind::External { .. } => match self {
                // Its agent is gone, whatever is pending: Abandoned until
                // its session is reopened.
                Working | PendingReview => Abandoned,
                other => other,
            },
            // The terminal isn't restored: frozen once, then PendingReview
            // or as decided.
            LaneKind::Worktree => match self {
                Working | PendingReview | Accepted | Rejected => hunks.settled(),
                Abandoned => {
                    if hunks.pending > 0 {
                        Abandoned
                    } else {
                        hunks.decided()
                    }
                }
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lane {
    pub id: LaneId,
    /// What the agent calls itself: "Chat", "claude-code", a task's title.
    pub name: Arc<str>,
    /// `Origin::Ai`'s agent: "fastedit.chat", a plugin id.
    pub agent: Arc<str>,
    /// `Origin::Ai` sessions it owns.
    pub sessions: Vec<Arc<str>>,
    pub kind: LaneKind,
    pub backing: Backing,
    pub state: LaneState,
    pub started: SystemTime,
    /// A worktree lane's newest revision: the tree it was last frozen at.
    pub revision: Option<TreeId>,
    /// Pending hunks as of the last event or restore; only an Abandoned
    /// lane's count in the rail reads it.
    pub pending: usize,
}

impl Lane {
    /// Apply `event`, returning the new state if it changed.
    pub fn apply(&mut self, event: &LaneEvent, hunks: Hunks) -> Option<LaneState> {
        self.pending = hunks.pending;
        let next = self.state.next(event, hunks)?;
        self.state = next;
        Some(next)
    }

    /// A worktree lane is frozen at `tree`: a running turn ends, a decided
    /// lane with new hunks returns to review. The tree is its newest
    /// revision.
    pub fn freeze(&mut self, tree: TreeId, hunks: Hunks) -> Option<LaneState> {
        self.revision = Some(tree);
        self.pending = hunks.pending;
        let event = if self.state == LaneState::Working {
            LaneEvent::TurnEnded
        } else {
            LaneEvent::HunksAdded
        };
        self.apply(&event, hunks)
    }

    /// The worktree watcher's debounced event: the live tree is `live`. If
    /// it no longer equals the newest revision's, the agent wrote again.
    pub fn observe_tree(&mut self, live: &TreeId) -> Option<LaneState> {
        use LaneState::*;
        if self.backing != Backing::Worktree
            || !matches!(self.state, PendingReview | Accepted | Rejected)
            || self.revision.as_ref() == Some(live)
        {
            return None;
        }
        self.state = Working;
        Some(Working)
    }
}

/// The counts the title bar and the rail show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LaneCounts {
    /// Lanes in `Working` ("Agents working").
    pub working: usize,
    /// Lanes with pending hunks (PendingReview, Abandoned not yet decided)
    /// plus those `Working`.
    pub pending: usize,
}

/// A workspace's lanes, by number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lanes {
    lanes: BTreeMap<LaneId, Lane>,
    /// The next number; `None` once every number is spent.
    next: Option<NonZeroU32>,
}

impl Default for Lanes {
    fn default() -> Self {
        Lanes {
            lanes: BTreeMap::new(),
            next: Some(NonZeroU32::MIN),
        }
    }
}

impl Lanes {
    pub fn new() -> Lanes {
        Lanes::default()
    }

    /// Rebuild from a saved record, reconciling every lane before anything
    /// reads it. `next` is the saved next number (`None`: exhausted), raised
    /// past any lane present. `facts` reads a lane's hunks and, for a
    /// worktree lane, the tree its worktree is frozen at now: it becomes the
    /// lane's newest revision, so observing it starts no turn.
    pub fn restore(
        lanes: Vec<Lane>,
        next: Option<u32>,
        mut facts: impl FnMut(&Lane) -> (Hunks, Option<TreeId>),
    ) -> Lanes {
        let mut restored = Lanes::new();
        let mut next = next.and_then(NonZeroU32::new);
        for mut lane in lanes {
            let (hunks, frozen) = facts(&lane);
            lane.state = lane.state.reconciled(&lane.kind, hunks);
            lane.pending = hunks.pending;
            if lane.backing == Backing::Worktree && frozen.is_some() {
                lane.revision = frozen;
            }
            if next.is_some_and(|n| n <= lane.id.0) {
                next = lane.id.0.checked_add(1);
            }
            restored.lanes.insert(lane.id, lane);
        }
        restored.next = next;
        restored
    }

    /// The number the next lane gets (to save with the record); `None` once
    /// every number is spent.
    pub fn next_number(&self) -> Option<u32> {
        self.next.map(NonZeroU32::get)
    }

    /// Start a lane in `Working`, numbered in order of creation. Its
    /// `session`, if any, is claimed. `None`, creating nothing, if another
    /// lane of the agent already holds the session or numbers are spent.
    pub fn open(
        &mut self,
        name: Arc<str>,
        agent: Arc<str>,
        session: Option<Arc<str>>,
        kind: LaneKind,
        backing: Backing,
        started: SystemTime,
    ) -> Option<LaneId> {
        let number = self.next?;
        if session
            .as_ref()
            .is_some_and(|s| self.lane_for(&agent, s).is_some())
        {
            return None;
        }
        let id = LaneId(number);
        self.next = number.checked_add(1);
        self.lanes.insert(
            id,
            Lane {
                id,
                name,
                agent,
                sessions: session.into_iter().collect(),
                kind,
                backing,
                state: LaneState::Working,
                started,
                revision: None,
                pending: 0,
            },
        );
        Some(id)
    }

    pub fn get(&self, id: LaneId) -> Option<&Lane> {
        self.lanes.get(&id)
    }

    pub fn get_mut(&mut self, id: LaneId) -> Option<&mut Lane> {
        self.lanes.get_mut(&id)
    }

    /// Lanes in order of number.
    pub fn iter(&self) -> impl Iterator<Item = &Lane> {
        self.lanes.values()
    }

    pub fn len(&self) -> usize {
        self.lanes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }

    /// The lane an edit's `Origin::Ai { agent, session }` belongs to: the
    /// one whose agent matches and whose sessions hold the session. An
    /// origin no lane claims has no lane.
    pub fn lane_for(&self, agent: &str, session: &str) -> Option<&Lane> {
        self.lanes
            .values()
            .find(|lane| &*lane.agent == agent && lane.sessions.iter().any(|s| &**s == session))
    }

    /// Add `session` to lane `id`'s sessions (the chat lane does it as each
    /// chat proposes). False if the lane is absent or another lane of the
    /// same agent already holds the session.
    pub fn claim(&mut self, id: LaneId, session: Arc<str>) -> bool {
        let Some(lane) = self.lanes.get(&id) else {
            return false;
        };
        match self.lane_for(&lane.agent, &session) {
            Some(owner) if owner.id != id => return false,
            Some(_) => return true,
            None => {}
        }
        if let Some(lane) = self.lanes.get_mut(&id) {
            lane.sessions.push(session);
        }
        true
    }

    /// Apply `event` to lane `id`, returning its new state if it changed.
    pub fn apply(&mut self, id: LaneId, event: &LaneEvent, hunks: Hunks) -> Option<LaneState> {
        self.lanes.get_mut(&id)?.apply(event, hunks)
    }

    /// The same agent reopens the same session: an Abandoned lane goes back
    /// to Working. Returns the lane reopened.
    pub fn reopen(&mut self, agent: &str, session: &str) -> Option<LaneId> {
        let id = self.lane_for(agent, session)?.id;
        let lane = self.lanes.get_mut(&id)?;
        lane.state = lane.state.next(&LaneEvent::Reopened, Hunks::default())?;
        Some(id)
    }

    /// Clear a lane: only one that has ended (Accepted, Rejected or
    /// Abandoned), and never the chat. The caller asks first and rejects the
    /// hunks of an Abandoned lane, and removes its worktree. Its number stays
    /// spent.
    pub fn clear(&mut self, id: LaneId) -> Option<Lane> {
        let lane = self.lanes.get(&id)?;
        if lane.kind == LaneKind::Chat
            || matches!(lane.state, LaneState::Working | LaneState::PendingReview)
        {
            return None;
        }
        self.lanes.remove(&id)
    }

    pub fn counts(&self) -> LaneCounts {
        let mut counts = LaneCounts::default();
        for lane in self.lanes.values() {
            if lane.state == LaneState::Working {
                counts.working += 1;
            }
            let in_rail = match lane.state {
                LaneState::Working | LaneState::PendingReview => true,
                LaneState::Abandoned => lane.pending > 0,
                LaneState::Accepted | LaneState::Rejected => false,
            };
            if in_rail {
                counts.pending += 1;
            }
        }
        counts
    }
}
