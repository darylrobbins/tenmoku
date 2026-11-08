//! A warning toast with a detail line, as a failed save shows one. Its
//! fill is translucent (`surface.overlay`, the audit's finding) over its own
//! shadow, so no token gives the colour there: the probe is its warning dot.

use fe_workbench::Workbench;

use crate::{
    audit_scenes::{DEFAULT, Step, Surface},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "toast",
    steps: |_| vec![toast()],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The warning dot's middle.
    probe(648., 522., "status.warning"),
    // The editor, left of the toast.
    probe(400., 400., "surface.background"),
];

/// A warning toast with a detail line, as a failed save shows one.
fn toast() -> Step {
    Box::new(|_, _, cx| {
        Workbench::notify(
            cx,
            log::Level::Warn,
            "Couldn't save src/main.rs".into(),
            Some("Permission denied (os error 13)".into()),
        );
    })
}
