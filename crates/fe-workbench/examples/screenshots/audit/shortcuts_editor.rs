//! The Keyboard Shortcuts tab: its search box and the table.

use crate::{
    audit_scenes::{DEFAULT, Surface, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "shortcuts_editor",
    steps: |_| vec![run("workbench.open_keyboard_shortcuts")],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The search box, right of its placeholder.
    probe(720., 113., "surface.background"),
    // The table, between the header and the first row.
    probe(720., 160., "surface.background"),
    // The table, between two columns.
    probe(600., 560., "surface.background"),
];
