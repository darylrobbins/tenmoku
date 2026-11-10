//! The Search panel with five results of `count` in one file.

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "search_panel",
    steps: |_| vec![run("search.show"), search("count")],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The query row, left of the query.
    probe(144., 152., "surface.panel"),
    // The panel, below the results.
    probe(174., 400., "surface.panel"),
];

/// Types `query` into the search panel's input and runs the search.
fn search(query: &'static str) -> Step {
    Box::new(move |workbench, _, cx| {
        let panel = workbench.dock().read(cx).search().clone();
        let input = panel.read(cx).input().clone();
        input.update(cx, |input, cx| input.set_text(query, cx));
        panel.update(cx, |panel, cx| panel.run(cx));
    })
}
