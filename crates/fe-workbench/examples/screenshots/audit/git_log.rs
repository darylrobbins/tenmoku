//! The Git log, the Graph view of Source Control, with no card.

use crate::{
    audit_scenes::{DEFAULT, Surface, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "git_log",
    git: true,
    steps: |_| vec![run("git.log")],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The selected newest commit, right of its summary.
    probe(224., 391., "list.row_selected"),
    // The panel, below the graph.
    probe(174., 500., "surface.panel"),
];
