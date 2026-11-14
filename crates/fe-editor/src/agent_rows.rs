//! The rows an agent wrote, as the editor's lane marks show them
//! (docs/designs/agent-lanes.md § Editor): its accepted text, from the
//! buffer's [`Provenance`](fe_buffer::Provenance), and the rows its pending
//! hunks replace, from the proposals an editor shows.
//!
//! [`AgentRows::decorations`] turns them into the dotted agent lane in the
//! change bar ([`GutterMark::Dotted`]) on every such row, and the faint
//! agent band ([`Tint::Agent`]) behind a pending hunk's.

use std::ops::Range;

use fe_buffer::{Anchor, Author, Buffer, BufferSnapshot, HunkStatus, Origin, ProposedChange};

use crate::decorations::{Decoration, DecorationKind, GutterMark, Tint};
use crate::lane::{LaneId, Lanes};

/// Whose rows they are, and whether they are undecided work.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct LaneMark {
    /// The lane the agent's session is in; `None` for an agent no lane
    /// claims, as Inline Edit.
    pub lane: Option<LaneId>,
    /// The rows of a pending or stale hunk, not accepted text.
    pub pending: bool,
}

/// The rows an agent wrote, each run as whole rows of the text it was made
/// in, anchored so it follows edits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentRows(pub Vec<(Range<Anchor>, LaneMark)>);

impl AgentRows {
    /// `buffer`'s agent rows now: each run of rows its provenance gives an
    /// agent ([`Provenance::row_authors`], so a row stays the agent's while
    /// any of its text is), and the rows each of `proposals`' pending and
    /// stale hunks replace, whole. A pure insertion replaces no row, so has
    /// none. A
    /// proposal for another buffer, or anchored past it, is left out. Each
    /// agent's lane is the one of `lanes` claiming its session.
    ///
    /// [`Provenance::row_authors`]: fe_buffer::Provenance::row_authors
    pub fn new<'a>(
        buffer: &Buffer,
        proposals: impl IntoIterator<Item = &'a ProposedChange>,
        lanes: &Lanes,
    ) -> Self {
        let snapshot = buffer.current();
        let text = snapshot.text();
        let count = text.row_count();
        let start = |row: u32| match row < count {
            true => text.row_start(row),
            false => text.len(),
        };
        let lane_of = |agent: &str, session: &str| lanes.lane_for(agent, session).map(|l| l.id);
        let mut rows = Vec::new();
        for (runs, author) in buffer.provenance().row_authors(text, 0..count) {
            let Some(Author::Ai { agent, session }) = author else {
                continue;
            };
            let mark = LaneMark {
                lane: lane_of(&agent, &session),
                pending: false,
            };
            rows.push((anchors(snapshot, start(runs.start)..start(runs.end)), mark));
        }
        for proposal in proposals {
            if proposal.buffer() != snapshot.id() || proposal.version() > snapshot.version() {
                continue;
            }
            let lane = match proposal.origin() {
                Origin::Ai { agent, session } => lane_of(agent, session),
                _ => continue,
            };
            let mark = LaneMark {
                lane,
                pending: true,
            };
            for hunk in proposal.hunks(snapshot) {
                let undecided = matches!(hunk.status, HunkStatus::Pending | HunkStatus::Stale);
                if undecided && !hunk.range.is_empty() {
                    // Whole rows, so any piece of a wrapped one in view
                    // finds them.
                    let first = text.offset_to_point(hunk.range.start).row;
                    let end = text.offset_to_point(hunk.range.end);
                    let last = match end.column == 0 && end.row > first {
                        true => end.row - 1,
                        false => end.row,
                    };
                    let range = start(first)..start(last + 1);
                    rows.push((anchors(snapshot, range), mark));
                }
            }
        }
        Self(rows)
    }

    /// The lane marks in `snapshot`, which is at or after the one the rows
    /// were made in: the dotted agent lane over every run, and the agent
    /// band behind a pending one's rows too.
    pub fn decorations(&self, snapshot: &BufferSnapshot) -> Vec<Decoration> {
        let mut out = Vec::new();
        for (range, mark) in &self.0 {
            let start = snapshot.resolve(&range.start);
            let range = start..snapshot.resolve(&range.end).max(start);
            if range.is_empty() {
                continue;
            }
            let dotted = DecorationKind::Gutter(GutterMark::Dotted);
            out.push(Decoration::new(range.clone(), dotted, Tint::Agent));
            if mark.pending {
                let band = DecorationKind::RowBackground;
                out.push(Decoration::new(range, band, Tint::Agent));
            }
        }
        out
    }
}

/// `range` anchored in `snapshot`, its ends staying put as text is typed
/// beside them.
fn anchors(snapshot: &BufferSnapshot, range: Range<usize>) -> Range<Anchor> {
    snapshot.anchor_after(range.start)..snapshot.anchor_before(range.end)
}
