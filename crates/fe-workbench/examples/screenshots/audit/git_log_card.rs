//! The Git log with the newest commit's card showing beside its row, as
//! hovering the row shows it (`GitLogPanel::show_card`): its avatar, author
//! and dates, message, co-author trailer, refs, totals and short id.

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, idle, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "git_log_card",
    git: true,
    // The card reads its totals once it's built, as its row draws.
    steps: |_| vec![run("git.log"), card(0), idle(), idle()],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The card, over the editor, right of its copy button.
    probe(700., 465., "surface.panel"),
    // The Source Control panel, below the graph.
    probe(850., 500., "surface.panel"),
];

/// Shows commit `ix`'s card.
fn card(ix: usize) -> Step {
    Box::new(move |workbench, _, cx| {
        let Some(panel) = workbench.git_log_panel(cx) else {
            return fail("the dock has no Git Log panel".to_owned());
        };
        panel.update(cx, |panel, cx| panel.show_card(ix, cx));
    })
}
