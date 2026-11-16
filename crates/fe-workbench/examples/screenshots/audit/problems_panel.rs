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
    // The bottom panel, below its message.
    probe(640., 560., "surface.panel"),
    // The editor above it, right of the code's empty fourth line.
    probe(700., 165., "surface.background"),
];
