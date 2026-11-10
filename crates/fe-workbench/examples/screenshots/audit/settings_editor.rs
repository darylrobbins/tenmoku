//! The Settings tab, on its first page (Chat): text fields, a select and
//! toggles.

use crate::{
    audit_scenes::{DEFAULT, Surface, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "settings_editor",
    steps: |_| vec![run("workbench.open_settings_ui")],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The page list's selected page, unfocused, right of its name.
    probe(432., 158., "list.row_selected_inactive"),
    // A toggle that's on, left of its knob.
    probe(955., 349., "accent.base"),
    // The page list, below its last page.
    probe(300., 470., "surface.background"),
];
