//! The Problems panel in its empty state.

use crate::{
    audit_scenes::{DEFAULT, Surface, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "problems_panel",
    steps: |_| vec![run("problems.show")],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The panel, below its message.
    probe(174., 400., "surface.panel"),
    // The editor beside it, below the code.
    probe(400., 400., "surface.background"),
];
