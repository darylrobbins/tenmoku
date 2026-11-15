//! A lane's file as a side-by-side diff (docs/designs/agent-lanes.md §
//! Diff, card 15): the left header `Before`, the right `After · lane 2` in
//! the agent's silver-blue ([`DiffSide::agent`]).
//!
//! - **Right side.** The buffer with the lane's pending hunks applied,
//!   [`ProposedChange::dry_run`]'s text, over a fixed text as the git
//!   diffs' sides are; the left is the buffer as it is. Nothing is written.
//! - **`agent.diff_lane_file`** opens it for the active editor's file and
//!   the first lane with pending hunks in it. The Agents panel, the Changes
//!   group's rows and the AI Changes panel open it with
//!   [`Workbench::open_lane_diff`].

use std::path::PathBuf;

use fe_buffer::{Buffer, HunkStatus};
use fe_editor::{EditorModel, lane::LaneId};
use fe_gpui::{DiffSide, EditorView};
use gpui::{App, Context, Entity, SharedString, Window};
use kiln::{Cx, platform::command_actions::dispatch_window};
use kiln_l10n::tr;

use crate::{
    Workbench,
    workbench::toast::{self, Toast},
};

#[cfg(test)]
#[path = "lane_diff_tests.rs"]
mod tests;

/// The command's declaration: see [`init`].
#[allow(dead_code)]
struct LaneDiffCommands;

#[kiln::command(context = "workbench")]
#[allow(dead_code)]
impl LaneDiffCommands {
    /// Show Lane Changes to Active File
    #[command(id = "agent.diff_lane_file")]
    fn diff_lane_file(&mut self, _: &mut Cx<Self>) {}
}

/// Handles `agent.diff_lane_file` on the window's workbench.
pub(crate) fn init(cx: &mut App) {
    cx.on_action(|_: &DiffLaneFile, cx| {
        let Some(handle) = dispatch_window(cx).or_else(|| cx.active_window()) else {
            return;
        };
        cx.defer(move |cx| {
            handle
                .update(cx, |_, window, cx| {
                    if let Some(Some(workbench)) = window.root::<Workbench>() {
                        workbench.update(cx, |workbench, cx| workbench.diff_lane_file(window, cx));
                    }
                })
                .ok();
        });
    });
}

/// A read-only side titled `title` over `text`.
fn fixed_side(title: impl Into<SharedString>, text: &str) -> DiffSide {
    let buffer = Buffer::from_file_contents(text);
    DiffSide::new(title, EditorModel::new(buffer, None)).read_only()
}

impl Workbench {
    /// `agent.diff_lane_file`: opens the active editor's file against the
    /// first lane's pending work in it; a toast says so if no lane has any.
    pub(crate) fn diff_lane_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.active_editor(cx).cloned();
        let lane = editor.as_ref().and_then(|editor| {
            let store = self.lanes.read(cx);
            pending_lanes(editor, store, cx).into_iter().next()
        });
        match (lane, editor) {
            (Some(lane), Some(editor)) if self.open_lane_diff(lane, &editor, window, cx) => {}
            _ => toast::show(cx, Toast::info(tr!("workbench-lane-diff-none"))),
        }
    }

    /// Opens `editor`'s file as a diff of `Before` against `After · lane N`,
    /// the right side `editor`'s buffer with `lane`'s pending hunks
    /// applied, in the active pane and focused. Whether the lane had any.
    pub(crate) fn open_lane_diff(
        &mut self,
        lane: LaneId,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((before, after)) = lane_texts(lane, editor, self.lanes.read(cx), cx) else {
            return false;
        };
        let path = (editor.read(cx).model().path())
            .map(|path| path.to_owned())
            .unwrap_or_else(|| PathBuf::from("untitled"));
        let left = fixed_side(tr!("workbench-lane-diff-before"), &before);
        let right =
            fixed_side(tr!("workbench-lane-diff-after", lane = lane.get()), &after).agent(true);
        self.open_side_compare(left, right, &path, window, cx);
        true
    }
}

/// The lanes with pending hunks in `editor`'s proposals, in lane order.
fn pending_lanes(
    editor: &Entity<EditorView>,
    store: &crate::lanes::LaneStore,
    cx: &App,
) -> Vec<LaneId> {
    let view = editor.read(cx);
    let buffer = view.model().buffer();
    let snapshot = buffer.current();
    let mut lanes: Vec<LaneId> = (view.proposals().iter())
        .filter_map(|proposal| {
            let shown = proposal.borrow();
            if shown.buffer() != snapshot.id() || shown.version() > snapshot.version() {
                return None;
            }
            let lane = store.lane_of(shown.origin())?;
            (shown.status_count(HunkStatus::Pending) > 0).then_some(lane)
        })
        .collect();
    lanes.sort();
    lanes.dedup();
    lanes
}

/// `editor`'s buffer as it is, and with `lane`'s pending hunks applied: of
/// one proposal, its [`fe_buffer::DryRun::text`]; of several, each one's
/// clean hunks together, a hunk overlapping an earlier one left out. `None`
/// if the lane has none pending there.
fn lane_texts(
    lane: LaneId,
    editor: &Entity<EditorView>,
    store: &crate::lanes::LaneStore,
    cx: &App,
) -> Option<(String, String)> {
    let view = editor.read(cx);
    let buffer = view.model().buffer();
    let snapshot = buffer.current();
    let mut runs = Vec::new();
    for proposal in view.proposals() {
        let shown = proposal.borrow();
        if shown.buffer() != snapshot.id()
            || shown.version() > snapshot.version()
            || store.lane_of(shown.origin()) != Some(lane)
            || shown.status_count(HunkStatus::Pending) == 0
        {
            continue;
        }
        let dry = shown.dry_run(snapshot);
        let hunks = shown.hunks(snapshot);
        runs.push((dry, hunks));
    }
    let before = snapshot.text().to_string();
    let after = match runs.as_slice() {
        [] => return None,
        [(dry, _)] => dry.text().to_string(),
        _ => {
            let mut edits: Vec<(std::ops::Range<usize>, std::sync::Arc<str>)> = Vec::new();
            for (dry, hunks) in &runs {
                for (outcome, hunk) in dry.hunks().iter().zip(hunks) {
                    if matches!(outcome.outcome, fe_buffer::Outcome::Clean) {
                        edits.push((outcome.range.clone(), hunk.new_text.clone()));
                    }
                }
            }
            edits.sort_by_key(|(range, _)| (range.start, range.end));
            let mut text = before.clone();
            let mut floor = usize::MAX;
            for (range, new_text) in edits.into_iter().rev() {
                if range.end > floor {
                    continue;
                }
                floor = range.start;
                text.replace_range(range, &new_text);
            }
            text
        }
    };
    Some((before, after))
}
