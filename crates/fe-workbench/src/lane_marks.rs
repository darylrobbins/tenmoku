//! Editor lane marks (docs/designs/agent-lanes.md § Editor, card 11): in
//! every open editor, the rows an agent wrote ([`AgentRows`]) as the change
//! bar's dotted agent lane, a pending hunk's old rows on the agent's faint
//! row band too, and each lane's proposals with their lane chip
//! (`lane 2 · pending`, [`EditorView::set_proposal_badges`]).
//!
//! - **Rows.** An agent's accepted text, by the buffer's provenance (a row
//!   stays the agent's while any of its text is; one the user rewrote whole
//!   is theirs), and the rows each pending or stale hunk of the proposals
//!   the editor shows replaces. An agent no lane claims, as Inline Edit,
//!   gets the lane but no chip.
//! - **Provider.** Each editor has its own [`DecorationSet`], installed with
//!   [`EditorModel::add_decoration_provider`](fe_editor::EditorModel::add_decoration_provider),
//!   in place of any copy a split made of another's. It emits
//!   [`GutterMark::Dotted`](fe_editor::decorations::GutterMark::Dotted) over
//!   every run and [`Tint::Agent`](fe_editor::decorations::Tint::Agent) row
//!   backgrounds over a pending one's.
//! - **Following.** Made afresh [`EDIT_REST`] after the last edit, as the
//!   provenance column is, the marks following the text meanwhile; at once
//!   as the editor's proposals change, are decided ([`ProposalEvent`]) or
//!   the lanes do.
//! - **Chip.** Per lane, over the proposals of it the editor shows:
//!   `lane 2 · pending`, `lane 2 · 1 conflict` while some are stale, and
//!   `lane 2 · stale` once all are. Text in `l10n/en-US/editor.ftl`, keys
//!   `workbench-editor-lane-…`.

use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Duration};

use fe_buffer::{HunkStatus, ProposedChange, Version};
use fe_editor::{
    AgentRows,
    decorations::{DecorationSet, ProviderId},
    lane::LaneId,
};
use fe_gpui::{EditorView, ProposalBadge, ProposalEvent};
use gpui::{App, Context, Entity, EntityId, SharedString, Subscription, Task, WeakEntity};
use kiln_l10n::tr;

use crate::{Workbench, lanes::LaneStore};

/// How long after the last edit the marks are made afresh: the provenance
/// column's rest.
const EDIT_REST: Duration = Duration::from_millis(300);

/// What an editor's marks were made of: its buffer's version and, for each
/// proposal it shows, its address and how many hunks are pending and stale.
type Made = (Version, Vec<(usize, usize, usize)>);

/// An editor with marks.
struct Slot {
    editor: WeakEntity<EditorView>,
    marks: Rc<RefCell<DecorationSet>>,
    /// What the marks shown were made of.
    made: Option<Made>,
    /// The rebuild waiting out an edit's rest, for the version asked for.
    resting: Option<(Version, Task<()>)>,
    _subscriptions: [Subscription; 2],
}

/// A window's marked editors, and every provider given out, which a split
/// copies.
#[derive(Default)]
struct State {
    slots: HashMap<EntityId, Slot>,
    issued: Vec<ProviderId>,
}

type Shared = Rc<RefCell<State>>;

pub(crate) fn init(cx: &mut App) {
    cx.observe_new(|workbench: &mut Workbench, window, cx| {
        if window.is_none() {
            return;
        }
        let state = Shared::default();
        cx.observe_self({
            let state = state.clone();
            move |workbench, cx| adopt(workbench, &state, cx)
        })
        .detach();
        let lanes = workbench.lanes.clone();
        cx.observe(&lanes, {
            let state = state.clone();
            move |workbench, _, cx| rebuild_all(workbench, &state, cx)
        })
        .detach();
        adopt(workbench, &state, cx);
    })
    .detach();
}

/// Gives each open editor without marks its own, and drops those of
/// editors no longer open.
fn adopt(workbench: &mut Workbench, state: &Shared, cx: &mut Context<Workbench>) {
    let editors = workbench.open_editors(cx);
    let new: Vec<Entity<EditorView>> = {
        let Ok(mut shared) = state.try_borrow_mut() else {
            return;
        };
        shared.slots.retain(|id, slot| {
            slot.editor.upgrade().is_some() && editors.iter().any(|e| e.entity_id() == *id)
        });
        (editors.into_iter())
            .filter(|editor| !shared.slots.contains_key(&editor.entity_id()))
            .collect()
    };
    for editor in new {
        attach(workbench, editor, state, cx);
    }
}

/// Gives `editor` its marks, in place of any of ours a split copied, and
/// makes them.
fn attach(
    workbench: &Workbench,
    editor: Entity<EditorView>,
    state: &Shared,
    cx: &mut Context<Workbench>,
) {
    let id = editor.entity_id();
    let marks = Rc::new(RefCell::new(DecorationSet::new()));
    let copied = state.borrow().issued.clone();
    let provider = editor.update(cx, |view, _| {
        for provider in copied {
            view.model_mut().remove_decoration_provider(provider);
        }
        view.model_mut().add_decoration_provider(marks.clone())
    });
    let subscriptions = [
        cx.observe(&editor, {
            let state = state.clone();
            move |workbench, _, cx| follow(workbench, id, &state, cx)
        }),
        cx.subscribe(&editor, {
            let state = state.clone();
            move |workbench, _, _: &ProposalEvent, cx| rebuild(workbench, id, &state, cx)
        }),
    ];
    {
        let mut shared = state.borrow_mut();
        shared.issued.push(provider);
        shared.slots.insert(
            id,
            Slot {
                editor: editor.downgrade(),
                marks,
                made: None,
                resting: None,
                _subscriptions: subscriptions,
            },
        );
    }
    rebuild(workbench, id, state, cx);
}

/// What `editor`'s marks would be made of now.
fn made(editor: &EditorView) -> Made {
    let proposals = (editor.proposals().iter())
        .map(|proposal| {
            let address = Rc::as_ptr(proposal) as *const u8 as usize;
            let proposal = proposal.borrow();
            let pending = proposal.status_count(HunkStatus::Pending);
            (address, pending, proposal.status_count(HunkStatus::Stale))
        })
        .collect();
    (editor.model().buffer().version(), proposals)
}

/// As editor `id` changes: its marks are made again at once for new
/// proposals or decisions, once its edits have rested for new text.
fn follow(workbench: &mut Workbench, id: EntityId, state: &Shared, cx: &mut Context<Workbench>) {
    let Some(editor) =
        (state.try_borrow().ok()).and_then(|shared| shared.slots.get(&id)?.editor.upgrade())
    else {
        return;
    };
    let now = made(editor.read(cx));
    let (version, waiting) = {
        let shared = state.borrow();
        let Some(slot) = shared.slots.get(&id) else {
            return;
        };
        if slot.made.as_ref() == Some(&now) {
            return;
        }
        let edited = slot
            .made
            .as_ref()
            .is_none_or(|(version, _)| *version != now.0);
        if !edited {
            drop(shared);
            return rebuild(workbench, id, state, cx);
        }
        let waiting = (slot.resting.as_ref()).is_some_and(|(asked, _)| *asked == now.0);
        (now.0, waiting)
    };
    if waiting {
        return;
    }
    let task = cx.spawn({
        let state = state.clone();
        async move |workbench, cx| {
            cx.background_executor().timer(EDIT_REST).await;
            workbench
                .update(cx, |workbench, cx| {
                    if let Some(slot) = state.borrow_mut().slots.get_mut(&id) {
                        slot.resting = None;
                    }
                    rebuild(workbench, id, &state, cx);
                })
                .ok();
        }
    });
    if let Some(slot) = state.borrow_mut().slots.get_mut(&id) {
        slot.resting = Some((version, task));
    }
}

/// Makes every marked editor's marks again: the lanes changed.
fn rebuild_all(workbench: &Workbench, state: &Shared, cx: &mut Context<Workbench>) {
    let ids: Vec<EntityId> = match state.try_borrow() {
        Ok(shared) => shared.slots.keys().copied().collect(),
        Err(_) => return,
    };
    for id in ids {
        rebuild(workbench, id, state, cx);
    }
}

/// Makes editor `id`'s marks and chips afresh.
fn rebuild(workbench: &Workbench, id: EntityId, state: &Shared, cx: &mut Context<Workbench>) {
    let Some((editor, marks)) = (state.try_borrow().ok()).and_then(|shared| {
        let slot = shared.slots.get(&id)?;
        Some((slot.editor.upgrade()?, slot.marks.clone()))
    }) else {
        return;
    };
    let store = workbench.lanes.read(cx);
    let view = editor.read(cx);
    let proposals: Vec<Rc<RefCell<ProposedChange>>> = view.proposals().to_vec();
    let badges = {
        let buffer = view.model().buffer();
        let borrowed: Vec<_> = proposals.iter().map(|proposal| proposal.borrow()).collect();
        let rows = AgentRows::new(&buffer, borrowed.iter().map(|p| &**p), store.lanes());
        let snapshot = buffer.current();
        let items = rows.decorations(snapshot);
        marks
            .borrow_mut()
            .apply(snapshot, snapshot.version(), items);
        badges(&proposals, store, &buffer)
    };
    let made = made(view);
    if let Some(slot) = state.borrow_mut().slots.get_mut(&id) {
        slot.made = Some(made);
    }
    editor.update(cx, |editor, cx| {
        editor.set_proposal_badges(badges, cx);
        cx.notify();
    });
}

/// Each lane's proposals of `proposals`, with the chip they share: what
/// their hunks in `buffer` are, together.
fn badges(
    proposals: &[Rc<RefCell<ProposedChange>>],
    store: &LaneStore,
    buffer: &fe_buffer::Buffer,
) -> Vec<(Rc<RefCell<ProposedChange>>, ProposalBadge)> {
    let snapshot = buffer.current();
    let mut lanes: Vec<(LaneId, Rc<RefCell<ProposedChange>>)> = Vec::new();
    // Each lane's pending and stale hunks.
    let mut counts: HashMap<LaneId, (usize, usize)> = HashMap::new();
    for proposal in proposals {
        let shown = proposal.borrow();
        if shown.buffer() != snapshot.id() || shown.version() > snapshot.version() {
            continue;
        }
        let Some(lane) = store.lane_of(shown.origin()) else {
            continue;
        };
        let count = counts.entry(lane).or_default();
        for hunk in shown.hunks(snapshot) {
            match hunk.status {
                HunkStatus::Pending => count.0 += 1,
                HunkStatus::Stale => count.1 += 1,
                HunkStatus::Accepted | HunkStatus::Rejected => {}
            }
        }
        lanes.push((lane, proposal.clone()));
    }
    (lanes.into_iter())
        .filter_map(|(lane, proposal)| {
            let (pending, stale) = counts[&lane];
            let number = lane.get();
            let text: SharedString = match (pending, stale) {
                (0, 0) => return None,
                (_, 0) => tr!("workbench-editor-lane-pending", lane = number),
                (0, _) => tr!("workbench-editor-lane-stale", lane = number),
                (_, count) => tr!(
                    "workbench-editor-lane-conflicts",
                    lane = number,
                    count = count
                ),
            };
            let badge = ProposalBadge {
                lane,
                text,
                stale: stale > 0,
            };
            Some((proposal, badge))
        })
        .collect()
}

#[cfg(test)]
#[path = "lane_marks_tests.rs"]
mod tests;
