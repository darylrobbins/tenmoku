//! The find bar over `main.rs`, with four matches of the selected
//! `counts`.

use crate::{
    audit_scenes::{DEFAULT, Surface, run, select},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "find_bar",
    steps: |_| vec![select("counts", 0), run("find.show")],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The find bar, between the query and the match count.
    probe(300., 101., "surface.panel"),
    probe(500., 101., "surface.panel"),
    // The editor below it, below the code.
    probe(400., 450., "surface.background"),
];
