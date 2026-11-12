//! A warning toast with a detail line, as a failed save shows one: 340 px
//! at the editor area's lower right, on the opaque `surface.raised` (the
//! audit's TO1-TO3), a warning icon, no coloured border.

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
    // The card's left padding, beside the icon: the raised surface.
    probe(655., 530., "surface.raised"),
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
