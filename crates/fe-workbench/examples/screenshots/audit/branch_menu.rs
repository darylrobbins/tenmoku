//! The branches menu, open under the title bar's branch widget on the demo
//! repository, as a click on the widget opens it: the search field, the
//! actions, then the Recent and Local sections, Update Project highlighted
//! from the keyboard.

use gpui::Keystroke;

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, idle, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "branch_menu",
    git: true,
    // The branches are listed in the background, then the menu shows.
    steps: |_| vec![run("git.branches_menu"), idle(), idle(), key("down")],
    probes: PROBES,
    ..DEFAULT
};

/// The menu opens under the branch widget, from its left edge, 460 px wide:
/// the widget's left is between 150 and 300 px at the scene's size, so the
/// menu covers 300 to 610 px whatever the widget's width.
const PROBES: &[Probe] = &[
    // The menu, right of Update Project's title, over the editor; Update
    // Project is its first row, under the search field.
    probe(450., 81., "list.row_selected"),
    // The menu, right of Commit…'s title, the row below.
    probe(450., 105., "surface.panel"),
];

/// Presses `keys` in the focused element: the menu's search field.
fn key(keys: &'static str) -> Step {
    Box::new(move |_, window, cx| {
        let keystroke = Keystroke::parse(keys).expect("a keystroke");
        window.defer(cx, move |window, cx| {
            window.dispatch_keystroke(keystroke, cx);
        });
    })
}
