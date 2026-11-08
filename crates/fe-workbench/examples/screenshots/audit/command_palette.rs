//! The command palette, open with nothing typed.

use crate::{
    audit_scenes::{DEFAULT, Surface, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "command_palette",
    steps: |_| vec![run("command_palette.toggle")],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The selected first row, left of its name.
    probe(206., 84., "list.row_selected"),
    // The palette, right of the rows.
    probe(600., 300., "surface.panel"),
    // The input, right of its placeholder.
    probe(600., 64., "surface.panel"),
];
