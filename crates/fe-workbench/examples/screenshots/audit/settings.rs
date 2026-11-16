//! The Settings page in B3 (T569), on its Common section: the serif title,
//! the You / This project switch, the theme cards and the first settings.
//! The user's settings.toml sets `workbench.rail` to its default, `left`,
//! so its row shows as changed, with its accent dot and Reset, and the
//! Common section has its index mark, while the rail stays where the other
//! captures have it.
//!
//! The page reads the user's file from the config directory
//! (`fe_config::user_settings_path`), which the audit's isolation puts on a
//! temporary folder, not the file the scene's settings are loaded from
//! (`settings.toml` beside the project), so the first step copies that one
//! there: the rail and, in the light capture, the light theme, whose card
//! then shows as chosen.

use std::{fs, path::Path};

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "settings",
    settings: "[workbench]\nrail = \"left\"\n",
    steps: |root| vec![user_file(root), run("workbench.open_settings_ui")],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The section index's current section, Common, in its padding left of
    // its name.
    probe(297., 221., "list.row_selected_inactive"),
    // The accent dot before the rail position's label, the second Common
    // row's (after Colour-blind mode, T1581): the file sets it.
    probe(501., 422., "accent.base"),
    // The list, in the gutter right of the rows' column.
    probe(990., 300., "surface.background"),
];

/// Copies the scene's settings, `settings.toml` beside project `root`, to
/// the user's settings.toml in the config directory, which the page reads.
fn user_file(root: &Path) -> Step {
    let scene = root.with_file_name("settings.toml");
    Box::new(move |_, _, _| {
        let Some(path) = fe_config::user_settings_path() else {
            return fail("no user settings path".to_owned());
        };
        if let Err(error) = fs::copy(&scene, &path) {
            fail(format!(
                "copying {} to {}: {error}",
                scene.display(),
                path.display()
            ));
        }
    })
}
