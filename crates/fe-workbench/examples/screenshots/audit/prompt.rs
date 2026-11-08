//! The folder trust prompt as it shows the first time a folder is opened,
//! drawn in the window by kiln's prompt (`kiln_ui::prompt_renderer`), as
//! where there are no native alerts. On macOS the app shows an NSAlert sheet
//! instead, which a capture off screen can't draw. Its setup turns on trust
//! prompting (`set_trust_file`) and the in-window prompt, which nothing
//! turns off: it's captured last.

use std::path::Path;

use gpui::VisualTestAppContext;

use crate::{
    audit_scenes::{DEFAULT, Surface, idle},
    token_probe::{Probe, probe, probe_over},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "prompt",
    setup,
    // The root's trust resolves off the main thread: a few frames for the
    // prompt to come.
    steps: |_| vec![idle(), idle(), idle()],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The dialog, left of its buttons.
    probe(330., 395., "surface.panel"),
    // Trust Folder, the primary button, left of its label.
    probe(497., 407., "accent.base"),
    // The scrim over the editor, below the code.
    probe_over(100., 450., "surface.overlay", "surface.background"),
];

/// Turns on trust prompting, with a trust file beside the project, and the
/// in-window prompt.
fn setup(cx: &mut VisualTestAppContext, parent: &Path) {
    let trust = parent.join("trust.toml");
    cx.update(|cx| {
        fe_workbench::set_trust_file(cx, trust);
        cx.set_prompt_builder(fe_ui::prompt_renderer);
    });
}
