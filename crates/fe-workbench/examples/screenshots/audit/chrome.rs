//! The workbench chrome: title bar, tabs, breadcrumbs, the project tree, the
//! rail and the status bar, with the demo's two files open. It's also the
//! scene every bundled theme is captured in, so its probes hold for every
//! theme. `surface.chrome` is drawn nowhere yet: the title bar, the tab
//! strip, the rail and the status bar are `surface.panel`, which it
//! defaults to.

use crate::{
    audit_scenes::{DEFAULT, Surface},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "chrome",
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The title bar, left of the title.
    probe(300., 10., "surface.panel"),
    // The tab strip, right of the tabs.
    probe(600., 45., "surface.panel"),
    // The editor, below the code.
    probe(400., 400., "surface.background"),
    // The project tree, below its last row.
    probe(850., 400., "surface.panel"),
    // The activity rail, between its buttons.
    probe(985., 400., "surface.panel"),
    // The status bar, between its items.
    probe(500., 590., "surface.panel"),
];
