//! The start page, the empty pane of a folder with nothing open.

use crate::{
    audit_scenes::{DEFAULT, Surface},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "start_page",
    open_demo: false,
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The selected first action, New File, at its left end.
    probe(400., 202., "list.row_selected"),
    // The page, below its last section.
    probe(300., 450., "surface.background"),
];
