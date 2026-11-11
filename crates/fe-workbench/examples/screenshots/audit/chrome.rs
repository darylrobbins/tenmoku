//! The workbench chrome: title bar, tabs, breadcrumbs, the project tree, the
//! rail and the status bar, with the demo's two files open. It's also the
//! scene every bundled theme is captured in, so its probes hold for every
//! theme. The title bar is `surface.chrome` (T1538), with no line under
//! it, and so is the status bar (T1543); the tab strip is still
//! `surface.panel`, which `surface.chrome` defaults to.

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
    // The title bar (38 px): by the traffic lights, which an offscreen
    // window doesn't draw; above the left items; between the left and
    // right items; right of the settings gear; and its last row, where the
    // line under it was.
    probe(40., 19., "surface.chrome"),
    probe(150., 3., "surface.chrome"),
    probe(500., 19., "surface.chrome"),
    probe(996., 19., "surface.chrome"),
    probe(500., 37.5, "surface.chrome"),
    // The tab strip, right of the tabs.
    probe(600., 45., "surface.panel"),
    // The editor, below the code.
    probe(400., 400., "surface.background"),
    // The project tree, below its last row.
    probe(150., 400., "surface.panel"),
    // The activity rail, between its buttons.
    probe(20., 400., "surface.chrome"),
    // The status bar, between its items.
    probe(500., 590., "surface.chrome"),
];
