//! The project tree with Git status: the project is a repository with a
//! modified, a staged and an untracked file.

use crate::{
    audit_scenes::{DEFAULT, Surface},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "project_tree_git",
    git: true,
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The selected row, `main.rs`, the tree unfocused.
    probe(850., 202., "list.row_selected_inactive"),
    // The tree, below its last row.
    probe(850., 400., "surface.panel"),
];
