//! The Git log with the newest commit's card open, the floating surface
//! (`surface.raised`, a hairline and the float shadow) over the list, whose
//! lanes are the theme's signal colours.

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, idle, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "git_log",
    git: true,
    // The card reads its totals once it's built, as its row draws.
    steps: |_| vec![run("git.log"), card(0), idle(), idle()],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The card, over the sidebar, right of its copy button.
    probe(250., 465., "surface.raised"),
    // The panel, below the graph.
    probe(174., 540., "surface.panel"),
];

/// Shows commit `ix`'s card, as hovering its row shows it.
fn card(ix: usize) -> Step {
    Box::new(move |workbench, _, cx| {
        let Some(panel) = workbench.git_log_panel(cx) else {
            return fail("the dock has no Git Log panel".to_owned());
        };
        panel.update(cx, |panel, cx| panel.show_card(ix, cx));
    })
}
