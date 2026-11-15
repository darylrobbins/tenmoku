//! The window's lanes (docs/designs/agent-lanes.md): the headless
//! [`Lanes`] and each lane's undecided work ([`LaneWork`]), in a
//! [`LaneStore`], an entity on the [`Workbench`].
//!
//! Lane 1 is the chat, as it works today (§ Today's chat agent):
//! [`LaneStore::new`] makes it first, `LaneKind::Chat`, agent
//! `fastedit.chat`, named `Chat`, Accepted until the first send. Each
//! `chat.propose_edit` and `chat.review_changes` claims its chat's session
//! in it before proposing, so every chat is lane 1. Its work is:
//!
//! - **Its newest turn**, the one the AI Changes panel shows
//!   (`Workbench::ai_changes`, which the panel is a view of).
//!   [`LaneStore::set_changeset`] gives the panel a newer turn and keeps
//!   the one it replaces in [`LaneWork`] while it has undecided hunks:
//!   still on its editors, still counted. So does a refactor preview
//!   taking the panel, which is no lane's.
//! - **Apply's proposals**, loose ([`LaneStore::adopt`]), each held by its
//!   editor too.
//!
//! The workbench counts a lane's hunks ([`Workbench::sync_lanes`]) whenever
//! the AI Changes panel is listed again and as the open editors'
//! [`ProposalEvent`]s come, and moves its state by the lifecycle table
//! ([`LaneState::next`]): the last hunk decided ends a review, hunks added
//! outside a turn (Apply or Review Changes on a finished reply) start one.
//! A send starts lane 1's turn and the reply finishing, failing or being
//! stopped ends it ([`Workbench::lane_turn`]). A proposal leaves
//! `LaneWork` once its last hunk is decided, an older turn once its
//! changeset is.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::SystemTime,
};

use fe_buffer::{BufferId, HunkStatus, Origin, ProposedChange};
use fe_editor::{
    BufferSet, Changeset, Counts, FileEntry,
    lane::{Backing, Hunks, Lane, LaneCounts, LaneEvent, LaneId, LaneKind, LaneState, Lanes},
};
use fe_gpui::{EditorView, ProposalEvent};
use gpui::{Context, Entity, EntityId, Focusable as _, Subscription, Window};

use crate::{
    Workbench,
    ai_changes_buffers::{Made, with_editors},
    ai_changes_panel::{AiChanges, Returned},
};

/// Lane 1's name.
const CHAT_NAME: &str = "Chat";

/// Lane 1, the chat's.
pub(crate) fn chat_lane() -> LaneId {
    LaneId::from_number(1).expect("1 is a lane number")
}

/// One of a lane's proposals made outside a turn's changeset (Apply's),
/// held by its editor too.
struct Loose {
    buffer: BufferId,
    /// The file it's for; `None` for an untitled buffer.
    path: Option<PathBuf>,
    proposal: Rc<RefCell<ProposedChange>>,
}

/// A lane's undecided work, wherever it shows (§ What a lane owns).
#[derive(Default)]
pub(crate) struct LaneWork {
    /// Each turn's changeset not yet resolved, oldest first, but the one
    /// the AI Changes panel shows.
    turns: Vec<AiChanges>,
    /// Proposals made outside a turn, until their last hunk is decided.
    loose: Vec<Loose>,
    /// Whether accepted hunks left the work, with the proposal or turn
    /// they were in, since a turn last started with nothing pending: with
    /// the accepted hunks still held, what Accepted or Rejected is chosen
    /// by. A revert can't reach them there, so they stay accepted.
    settled: bool,
    /// The files whose accepted hunks left the work with their proposal
    /// or turn: the lane wrote them.
    written: BTreeSet<PathBuf>,
    /// Each file's counts, as last counted: every turn's and loose
    /// proposal's, the shown turn's too.
    files: Vec<(PathBuf, Counts)>,
    /// Stale hunks not yet rejected, as last counted.
    conflicts: usize,
}

impl LaneWork {
    fn is_empty(&self) -> bool {
        self.turns.is_empty() && self.loose.is_empty()
    }
}

/// A lane as the panels that list lanes read it: the Agents panel, and
/// the Changes panel (a later card).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LaneSummary {
    pub(crate) id: LaneId,
    pub(crate) name: Arc<str>,
    pub(crate) state: LaneState,
    /// Its undecided hunks, pending and stale, as last counted.
    pub(crate) pending: usize,
    /// The files it has proposed to, with their hunks' counts.
    pub(crate) files: Vec<(PathBuf, Counts)>,
    /// Stale hunks not yet rejected.
    pub(crate) conflicts: usize,
}

/// A lane's state and pending count, and its work's files, conflicts and
/// files written, as its readers see them.
type Shown = (
    Option<(LaneState, usize)>,
    Option<(Vec<(PathBuf, Counts)>, usize, BTreeSet<PathBuf>)>,
);

/// What counting a lane's hunks found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    /// Each file's counts, in the order first met.
    pub(crate) files: Vec<(PathBuf, Counts)>,
    /// Undecided hunks: pending, and stale until rejected.
    pub(crate) pending: usize,
    pub(crate) accepted: usize,
    pub(crate) stale: usize,
    /// Whether accepted hunks left the work as it was counted.
    pub(crate) settled: bool,
    /// The files of those, where they have one.
    pub(crate) written: Vec<PathBuf>,
}

impl Tally {
    /// Adds `counts`, for `path` if the hunks have one.
    fn add(&mut self, path: Option<&PathBuf>, counts: Counts) {
        self.pending += counts.pending + counts.stale;
        self.accepted += counts.accepted;
        self.stale += counts.stale;
        let Some(path) = path else {
            return;
        };
        match self.files.iter_mut().find(|(held, _)| held == path) {
            Some((_, held)) => {
                held.hunks += counts.hunks;
                held.pending += counts.pending;
                held.accepted += counts.accepted;
                held.rejected += counts.rejected;
                held.stale += counts.stale;
            }
            None => self.files.push((path.clone(), counts)),
        }
    }

    fn add_entries(&mut self, entries: &[FileEntry]) {
        for entry in entries {
            self.add(Some(&entry.path), entry.counts);
        }
    }

    /// Notes that work with `counts` (`path`'s) left the lane's work,
    /// decided.
    fn settle(&mut self, path: Option<&PathBuf>, counts: Counts) {
        if counts.accepted == 0 {
            return;
        }
        self.settled = true;
        if let Some(path) = path.filter(|path| !self.written.contains(path)) {
            self.written.push(path.clone());
        }
    }
}

/// `proposal`'s hunks recorded undecided: pending, or stale until
/// rejected.
fn undecided(proposal: &ProposedChange) -> usize {
    proposal.status_count(HunkStatus::Pending) + proposal.status_count(HunkStatus::Stale)
}

/// Whether `entries` have nothing left to decide.
fn resolved(entries: &[FileEntry]) -> bool {
    (entries.iter()).all(|entry| entry.counts.pending == 0 && entry.counts.stale == 0)
}

/// `proposal`'s counts: read against `buffer` if it's at hand, so a hunk
/// gone stale counts so, else as last recorded.
fn proposal_counts(proposal: &ProposedChange, buffer: Option<&mut fe_buffer::Buffer>) -> Counts {
    if let Some(buffer) = buffer {
        let mut counts = Counts::default();
        for hunk in proposal.hunks(buffer.current()) {
            counts.hunks += 1;
            match hunk.status {
                HunkStatus::Pending => counts.pending += 1,
                HunkStatus::Accepted => counts.accepted += 1,
                HunkStatus::Rejected => counts.rejected += 1,
                HunkStatus::Stale => counts.stale += 1,
            }
        }
        return counts;
    }
    Counts {
        hunks: proposal.hunk_count(),
        pending: proposal.status_count(HunkStatus::Pending),
        accepted: proposal.status_count(HunkStatus::Accepted),
        rejected: proposal.status_count(HunkStatus::Rejected),
        stale: proposal.status_count(HunkStatus::Stale),
    }
}

/// The window's lanes and their work.
pub(crate) struct LaneStore {
    lanes: Lanes,
    work: BTreeMap<LaneId, LaneWork>,
    /// The open editors whose proposal events count the lanes again.
    followed: HashMap<EntityId, Subscription>,
}

impl Default for LaneStore {
    fn default() -> Self {
        Self::new()
    }
}

impl LaneStore {
    /// A store with lane 1, the chat, made before anything else.
    pub(crate) fn new() -> Self {
        Self::restore(Vec::new(), Some(1))
    }

    /// The store of a saved record and its next number (`None`: every
    /// number spent), every lane reconciled before anything reads it. Lane 1 is always the chat: a record without it, or with
    /// another lane numbered 1, gets the chat there, and no other lane ever
    /// takes 1.
    pub(crate) fn restore(record: Vec<Lane>, next: Option<u32>) -> Self {
        let chat = chat_lane();
        let mut record: Vec<Lane> = (record.into_iter())
            .filter(|lane| lane.id != chat || lane.kind == LaneKind::Chat)
            .collect();
        if record.iter().all(|lane| lane.id != chat) {
            record.insert(0, Self::chat(SystemTime::now()));
        }
        // Nothing is pending until the lanes' work is counted; a lane saved
        // as Accepted had one accepted.
        let lanes = Lanes::restore(record, next, |lane| {
            let hunks = Hunks {
                pending: 0,
                any_accepted: lane.state == LaneState::Accepted,
            };
            (hunks, None)
        });
        Self {
            lanes,
            work: BTreeMap::new(),
            followed: HashMap::new(),
        }
    }

    /// Lane 1 as made: Accepted, nothing pending, until the first send.
    fn chat(started: SystemTime) -> Lane {
        Lane {
            id: chat_lane(),
            name: CHAT_NAME.into(),
            agent: crate::chat_panel::AGENT.into(),
            sessions: Vec::new(),
            kind: LaneKind::Chat,
            backing: Backing::Shared,
            state: LaneState::Accepted,
            started,
            revision: None,
            pending: 0,
        }
    }

    #[allow(dead_code)] // The title bar (a later card) reads it.
    pub(crate) fn lanes(&self) -> &Lanes {
        &self.lanes
    }

    pub(crate) fn lane(&self, id: LaneId) -> Option<&Lane> {
        self.lanes.get(id)
    }

    /// The lane an edit of `origin` belongs to, if one claims it.
    pub(crate) fn lane_of(&self, origin: &Origin) -> Option<LaneId> {
        match origin {
            Origin::Ai { agent, session } => Some(self.lanes.lane_for(agent, session)?.id),
            _ => None,
        }
    }

    /// Adds `session` to lane `id`'s, as each chat does before it proposes.
    pub(crate) fn claim(&mut self, id: LaneId, session: Arc<str>) -> bool {
        self.lanes.claim(id, session)
    }

    /// Registers `proposal`, made outside a turn on `buffer` (`path`'s), as
    /// one of lane `id`'s, so it counts as pending until decided.
    pub(crate) fn adopt(
        &mut self,
        id: LaneId,
        buffer: BufferId,
        path: Option<PathBuf>,
        proposal: Rc<RefCell<ProposedChange>>,
    ) {
        if self.lanes.get(id).is_none() {
            return;
        }
        let work = self.work.entry(id).or_default();
        if (work.loose.iter()).any(|loose| Rc::ptr_eq(&loose.proposal, &proposal)) {
            return;
        }
        work.loose.push(Loose {
            buffer,
            path,
            proposal,
        });
    }

    /// Keeps `turn`, which the panel no longer shows, among its lane's
    /// work, after its other turns.
    fn keep_turn(&mut self, id: LaneId, turn: AiChanges) {
        if self.lanes.get(id).is_some() {
            self.work.entry(id).or_default().turns.push(turn);
        }
    }

    /// Keeps what `tally`, of work that left lane `id` decided or
    /// replaced, accepted: the lane wrote it.
    fn settle(&mut self, id: LaneId, tally: Tally) {
        if !tally.settled {
            return;
        }
        let work = self.work.entry(id).or_default();
        work.settled = true;
        work.written.extend(tally.written);
    }

    /// Whether any lane holds work outside the panel.
    pub(crate) fn has_work(&self) -> bool {
        self.work.values().any(|work| !work.is_empty())
    }

    /// Lane `id`'s count `tally`, moving its state: its last hunk decided,
    /// or hunks added once it was decided. Whether anything its readers
    /// see changed: its state, its pending count, its files, their counts,
    /// its conflicts or what it wrote.
    pub(crate) fn recount(&mut self, id: LaneId, tally: Tally) -> bool {
        let before = self.shown(id);
        let accepted = tally.accepted > 0;
        let work = self.work.entry(id).or_default();
        work.settled |= tally.settled;
        work.written.extend(tally.written);
        work.conflicts = tally.stale;
        work.files = tally.files;
        let hunks = Hunks {
            pending: tally.pending,
            any_accepted: accepted || work.settled,
        };
        if let Some(lane) = self.lanes.get_mut(id)
            && lane.apply(&LaneEvent::Decided, hunks).is_none()
            && hunks.pending > 0
        {
            lane.apply(&LaneEvent::HunksAdded, hunks);
        }
        self.shown(id) != before
    }

    /// What readers of lane `id` see.
    fn shown(&self, id: LaneId) -> Shown {
        let lane = self.lanes.get(id).map(|lane| (lane.state, lane.pending));
        let work = (self.work.get(&id))
            .map(|work| (work.files.clone(), work.conflicts, work.written.clone()));
        (lane, work)
    }

    /// Lane `id`'s turn starts or ends (`event`), its hunks counted as
    /// `tally`. A turn starting with nothing pending starts the choice
    /// between Accepted and Rejected afresh.
    pub(crate) fn turn(&mut self, id: LaneId, event: LaneEvent, tally: Tally) {
        let (pending, accepted) = (tally.pending, tally.accepted > 0);
        self.recount(id, tally);
        let work = self.work.entry(id).or_default();
        if event == LaneEvent::TurnStarted && pending == 0 {
            work.settled = false;
        }
        let hunks = Hunks {
            pending,
            any_accepted: accepted || work.settled,
        };
        self.lanes.apply(id, &event, hunks);
    }

    /// The title bar's and the rail's counts. An Abandoned lane counts in
    /// the rail only while it has hunks pending.
    pub(crate) fn counts(&self) -> LaneCounts {
        self.lanes.counts()
    }

    /// The rail's count: lanes with hunks pending, those Working among
    /// them ([`LaneCounts::pending`] holds both).
    pub(crate) fn rail_count(&self) -> usize {
        self.counts().pending
    }

    /// Every lane, in order of number, with its files as last counted.
    pub(crate) fn summaries(&self) -> Vec<LaneSummary> {
        (self.lanes.iter())
            .map(|lane| {
                let work = self.work.get(&lane.id);
                LaneSummary {
                    id: lane.id,
                    name: lane.name.clone(),
                    state: lane.state,
                    pending: lane.pending,
                    files: work.map(|work| work.files.clone()).unwrap_or_default(),
                    conflicts: work.map_or(0, |work| work.conflicts),
                }
            })
            .collect()
    }

    /// Removes lane `id` and its work: only one that has ended (Accepted,
    /// Rejected or Abandoned), never lane 1 ([`Lanes::clear`]). The caller
    /// rejects an Abandoned lane's hunks first, having asked
    /// ([`Workbench::reject_lane_work`]). Its number stays spent.
    pub(crate) fn clear(&mut self, id: LaneId) -> Option<Lane> {
        let lane = self.lanes.clear(id)?;
        self.work.remove(&id);
        Some(lane)
    }

    /// Each file a lane wrote, with the lanes that did: accepted hunks in
    /// its work as last counted, and those that left it decided.
    #[allow(dead_code)] // The Agents panel, title bar and rail (later cards) read it.
    pub(crate) fn written_files(&self) -> HashMap<PathBuf, Vec<LaneId>> {
        let mut written: HashMap<PathBuf, Vec<LaneId>> = HashMap::new();
        for (id, work) in &self.work {
            let held = (work.files.iter())
                .filter(|(_, counts)| counts.accepted > 0)
                .map(|(path, _)| path);
            let paths: BTreeSet<&PathBuf> = held.chain(&work.written).collect();
            for path in paths {
                written.entry(path.clone()).or_default().push(*id);
            }
        }
        written
    }
}

impl LaneStore {
    /// Shows `changeset`, a turn of lane `id`'s, in the AI Changes panel in
    /// place of the turn shown before, which stays in its lane's work while
    /// it has hunks to decide. Only lane 1's turns show there; another
    /// lane's is kept in its work.
    pub(crate) fn set_changeset(
        workbench: &mut Workbench,
        id: LaneId,
        changeset: Changeset,
        returned: Option<Returned>,
        window: &mut Window,
        cx: &mut Context<Workbench>,
    ) {
        let mut turn = AiChanges::new(changeset);
        turn.lane = Some(id);
        turn.returned = returned;
        if id != chat_lane() {
            workbench
                .lanes
                .update(cx, |store, _| store.keep_turn(id, turn));
            return workbench.sync_lanes(cx);
        }
        // The turn it replaces leaves no review tabs behind.
        workbench.discard_ai_reviews(window, cx);
        workbench.keep_shown_turn(cx);
        workbench.ai_changes = Some(turn);
        workbench.ai_changes_panel(cx);
        workbench.sync_ai_changes(cx);
        workbench.run_dock_op(crate::dock::DockOp::ShowAiChanges, window, cx);
    }
}

impl Workbench {
    /// Moves the turn the AI Changes panel shows, if it's a lane's, into its
    /// lane's work, for something else to take the panel.
    pub(crate) fn keep_shown_turn(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.ai_changes.as_ref().and_then(|state| state.lane) else {
            return;
        };
        let Some(mut turn) = self.ai_changes.take() else {
            return;
        };
        // A turn its chat asked back (the API chat's, whose proposals keep
        // its decisions) goes back now, as the panel lets go of it, and
        // isn't kept in the lane's work.
        if turn.returned.is_some() {
            return;
        }
        turn.set_aside();
        self.lanes.update(cx, |store, _| store.keep_turn(id, turn));
    }

    /// Forgets lane turn `turn_id`, which a Review of it again replaces:
    /// its kept copy goes, and its accepted hunks, kept or shown, are
    /// settled first, so what it wrote stays its lane's. The shown turn
    /// is the caller's to take off the panel.
    pub(crate) fn drop_lane_turn(&mut self, turn_id: &str, cx: &mut Context<Self>) {
        let editors = self.open_editors(cx);
        let shown = (self.ai_changes.as_mut())
            .filter(|state| state.changeset.turn_id() == turn_id)
            .and_then(|state| {
                let id = state.lane?;
                let entries = with_editors(&editors, &mut state.made, cx, |set| {
                    state.changeset.files(set)
                });
                Some((id, entries))
            });
        self.lanes.update(cx, |store, cx| {
            let mut dropped: Vec<(LaneId, Vec<FileEntry>)> = shown.into_iter().collect();
            for (id, work) in &mut store.work {
                work.turns.retain_mut(|turn| {
                    if turn.changeset.turn_id() != turn_id {
                        return true;
                    }
                    let entries = with_editors(&editors, &mut turn.made, cx, |set| {
                        turn.changeset.files(set)
                    });
                    dropped.push((*id, entries));
                    false
                });
            }
            for (id, entries) in dropped {
                let mut tally = Tally::default();
                for entry in &entries {
                    tally.settle(Some(&entry.path), entry.counts);
                }
                store.settle(id, tally);
            }
        });
    }

    /// Registers `proposal`, an Apply's on `buffer`, with the lane its
    /// origin belongs to, if any, and counts that lane again.
    pub(crate) fn adopt_proposal(
        &mut self,
        buffer: BufferId,
        path: Option<PathBuf>,
        proposal: Rc<RefCell<ProposedChange>>,
        cx: &mut Context<Self>,
    ) {
        let origin = proposal.borrow().origin().clone();
        let Some(id) = self.lanes.read(cx).lane_of(&origin) else {
            return;
        };
        self.lanes
            .update(cx, |store, _| store.adopt(id, buffer, path, proposal));
        self.sync_lanes(cx);
    }

    /// Rejects every undecided hunk of lane `id`'s work, kept turns and
    /// loose proposals, as clearing an Abandoned lane does once the user
    /// says so; no text is touched. A hunk whose buffer an editor holds is
    /// rejected through it, so its counts follow; one whose buffer none
    /// holds any more is rejected as recorded
    /// ([`ProposedChange::reject_undecided`]). The open editors redraw and
    /// the lane is counted again. Returns how many hunks are still
    /// undecided after it: none, unless a rejection failed.
    pub(crate) fn reject_lane_work(&mut self, id: LaneId, cx: &mut Context<Self>) -> usize {
        let editors = self.open_editors(cx);
        let remaining = self.lanes.update(cx, |store, cx| {
            let Some(work) = store.work.get_mut(&id) else {
                return 0;
            };
            let mut remaining = 0;
            for turn in &mut work.turns {
                let entries = with_editors(&editors, &mut turn.made, cx, |set| {
                    // A file whose buffer is gone reports a conflict here,
                    // and is rejected below.
                    let _ = turn.changeset.reject_all(set);
                    turn.changeset.files(set)
                });
                for entry in &entries {
                    match turn.changeset.proposal(&entry.path) {
                        Some((_, proposal)) => {
                            let mut proposal = proposal.borrow_mut();
                            proposal.reject_undecided();
                            remaining += undecided(&proposal);
                        }
                        // A new file, rejected without a buffer.
                        None => remaining += entry.counts.pending,
                    }
                }
            }
            for loose in &work.loose {
                let mut proposal = loose.proposal.borrow_mut();
                proposal.reject_undecided();
                remaining += undecided(&proposal);
            }
            remaining
        });
        for editor in &editors {
            editor.update(cx, |_, cx| cx.notify());
        }
        self.sync_lanes(cx);
        remaining
    }

    /// Shows lane `id`'s undecided work for review: the AI Changes panel,
    /// if it shows the lane's turn with hunks undecided; else the first of
    /// its kept turns' files and its loose proposals (Apply's) with one,
    /// in its editor, brought forward and focused, the caret on its first
    /// undecided hunk. Whether it showed any. Only lane 1's work is
    /// reviewed here; another lane's is `agent.review_lane`'s (card 19).
    /// A refactor preview on the panel stays: the lane's turn under it is
    /// kept work, reached through its editors.
    pub(crate) fn review_lane(
        &mut self,
        id: LaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if id != chat_lane() {
            return false;
        }
        let editors = self.open_editors(cx);
        // A refactor preview on the panel is no lane's turn.
        if let Some(state) = (self.ai_changes.as_mut()).filter(|state| state.lane == Some(id)) {
            let entries = with_editors(&editors, &mut state.made, cx, |set| {
                state.changeset.files(set)
            });
            if (entries.iter()).any(|entry| entry.counts.pending + entry.counts.stale > 0) {
                self.ai_changes_panel(cx);
                self.run_dock_op(crate::dock::DockOp::ShowAiChanges, window, cx);
                return true;
            }
        }
        let target = self.lanes.update(cx, |store, cx| {
            let work = store.work.get_mut(&id)?;
            for turn in &mut work.turns {
                let entries = with_editors(&editors, &mut turn.made, cx, |set| {
                    turn.changeset.files(set)
                });
                let found = (entries.iter())
                    .filter(|entry| entry.counts.pending + entry.counts.stale > 0)
                    .find_map(|entry| turn.changeset.proposal(&entry.path));
                if found.is_some() {
                    return found;
                }
            }
            (work.loose.iter())
                .find(|loose| undecided(&loose.proposal.borrow()) > 0)
                .map(|loose| (loose.buffer, loose.proposal.clone()))
        });
        let Some((buffer, proposal)) = target else {
            return false;
        };
        let shown = (self.pane_group(cx).panes().into_iter()).find_map(|pane| {
            let at = (pane.read(cx).tabs().iter()).position(|tab| {
                (tab.editor()).is_some_and(|editor| editor.read(cx).model().buffer().id() == buffer)
            })?;
            let editor = pane.read(cx).tabs()[at].editor().cloned()?;
            Some((pane.clone(), at, editor))
        });
        let Some((pane, at, editor)) = shown else {
            return false;
        };
        self.activate(pane.clone(), window, cx);
        pane.update(cx, |pane, cx| pane.activate(at, window, cx));
        editor.update(cx, |editor, cx| {
            let hunks = proposal.borrow().hunks(editor.model().buffer().current());
            let first = (hunks.iter())
                .filter(|hunk| matches!(hunk.status, HunkStatus::Pending | HunkStatus::Stale))
                .map(|hunk| hunk.range.start)
                .min();
            if let Some(at) = first {
                editor.select_and_reveal(at..at, cx);
            }
        });
        window.focus(&editor.focus_handle(cx), cx);
        true
    }

    /// Lane `id`'s turn starts or ends (`event`): the chat's send, its
    /// reply finishing, failing or being stopped.
    pub(crate) fn lane_turn(&mut self, id: LaneId, event: LaneEvent, cx: &mut Context<Self>) {
        let editors = self.open_editors(cx);
        self.follow_lane_editors(&editors, cx);
        let tally = self.tally(id, &editors, cx);
        self.lanes.update(cx, |store, cx| {
            store.turn(id, event, tally);
            cx.notify();
        });
    }

    /// Counts every lane's hunks again, following the open editors'
    /// decisions, and moves each lane's state by what it finds.
    pub(crate) fn sync_lanes(&mut self, cx: &mut Context<Self>) {
        let editors = self.open_editors(cx);
        self.follow_lane_editors(&editors, cx);
        let ids: Vec<LaneId> = (self.lanes.read(cx).lanes.iter())
            .map(|lane| lane.id)
            .collect();
        for id in ids {
            let tally = self.tally(id, &editors, cx);
            self.lanes.update(cx, |store, cx| {
                if store.recount(id, tally) {
                    cx.notify();
                }
            });
        }
    }

    /// Follows the proposal events of `editors`, the open ones, and no
    /// others: a hunk decided in any counts its lane again.
    fn follow_lane_editors(&mut self, editors: &[Entity<EditorView>], cx: &mut Context<Self>) {
        let live: HashSet<EntityId> = editors.iter().map(Entity::entity_id).collect();
        let followed = &self.lanes.read(cx).followed;
        let new: Vec<Entity<EditorView>> = (editors.iter())
            .filter(|editor| !followed.contains_key(&editor.entity_id()))
            .cloned()
            .collect();
        let subscriptions: Vec<(EntityId, Subscription)> = (new.iter())
            .map(|editor| {
                let subscription = cx.subscribe(editor, |workbench, _, _: &ProposalEvent, cx| {
                    workbench.sync_lanes(cx)
                });
                (editor.entity_id(), subscription)
            })
            .collect();
        self.lanes.update(cx, |store, _| {
            store.followed.retain(|id, _| live.contains(id));
            store.followed.extend(subscriptions);
        });
    }

    /// Lane `id`'s hunks: its kept turns', the shown turn's if it's the
    /// lane's, and its loose proposals'. Kept turns resolved and proposals
    /// decided leave its work, settled.
    fn tally(
        &mut self,
        id: LaneId,
        editors: &[Entity<EditorView>],
        cx: &mut Context<Self>,
    ) -> Tally {
        let mut tally = Tally::default();
        // In turn order: the kept turns, the shown one, then the loose
        // proposals.
        self.lanes.update(cx, |store, cx| {
            let Some(work) = store.work.get_mut(&id) else {
                return;
            };
            work.turns.retain_mut(|turn| {
                let entries =
                    with_editors(editors, &mut turn.made, cx, |set| turn.changeset.files(set));
                tally.add_entries(&entries);
                let resolved = resolved(&entries);
                if resolved {
                    for entry in &entries {
                        tally.settle(Some(&entry.path), entry.counts);
                    }
                }
                !resolved
            });
        });
        if let Some(state) = (self.ai_changes.as_mut()).filter(|state| state.lane == Some(id)) {
            let entries = with_editors(editors, &mut state.made, cx, |set| {
                state.changeset.files(set)
            });
            tally.add_entries(&entries);
        }
        self.lanes.update(cx, |store, cx| {
            let Some(work) = store.work.get_mut(&id) else {
                return;
            };
            let mut made = Made::default();
            with_editors(editors, &mut made, cx, |set| {
                work.loose.retain(|loose| {
                    let proposal = loose.proposal.borrow();
                    let counts = proposal_counts(&proposal, set.buffer_mut(loose.buffer));
                    tally.add(loose.path.as_ref(), counts);
                    let decided = counts.pending + counts.stale == 0;
                    if decided {
                        tally.settle(loose.path.as_ref(), counts);
                    }
                    !decided
                });
            });
        });
        tally
    }
}

#[cfg(test)]
#[path = "lanes_tests.rs"]
mod lanes_tests;
