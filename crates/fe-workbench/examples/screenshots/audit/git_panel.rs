//! Source Control: the commit box, staged and unstaged changes, and the
//! graph below.

use crate::{
    audit_scenes::{DEFAULT, Surface, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "git_panel",
    git: true,
    steps: |_| vec![run("git.show_changes")],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The commit message box, right of its placeholder's start.
    probe(68., 125., "surface.background"),
    // The Commit button, disabled with no message, left of its label.
    probe(217., 160., "state.disabled"),
    // The panel, below the graph.
    probe(174., 500., "surface.panel"),
];
