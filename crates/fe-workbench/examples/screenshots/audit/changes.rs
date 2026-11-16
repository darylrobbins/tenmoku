//! Changes and commit in B3, on the audit's Git project (a modified, a staged
//! and an untracked file): the message box with its placeholder in the UI
//! font, Amend, "Commit 1 file" and its "and push", then the groups' rows
//! with their checkboxes and letters, and the graph below.

use crate::{
    audit_scenes::{DEFAULT, Surface, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "changes",
    git: true,
    steps: |_| vec![run("git.show_changes")],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The empty message box, below its first row (whose band is the
    // editor's current line, `[editor] active_line`), right of its
    // placeholder.
    probe(180., 150., "surface.background"),
    // The panel, below the graph.
    probe(174., 500., "surface.panel"),
];
