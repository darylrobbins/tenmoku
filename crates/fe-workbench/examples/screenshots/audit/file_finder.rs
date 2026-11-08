//! Go to File, open with nothing typed.

use crate::{
    audit_scenes::{DEFAULT, Surface, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "file_finder",
    steps: |_| vec![run("quick_open.toggle")],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The selected first file, right of its name.
    probe(240., 118., "list.row_selected"),
    // The picker, right of the files.
    probe(600., 200., "surface.panel"),
];
