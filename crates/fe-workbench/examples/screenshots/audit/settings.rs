//! The Settings page in B3 (T569), on its Common section: the serif title,
//! the You / This project switch, the theme cards and the first settings.
//! The user's settings.toml sets `workbench.rail` to its default, `left`,
//! so the first row shows as changed, with its accent dot and Reset, and
//! the Common section has its index mark, while the rail stays where the
//! other captures have it.

use crate::{
    audit_scenes::{DEFAULT, Surface, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "settings",
    settings: "[workbench]\nrail = \"left\"\n",
    steps: |_| vec![run("workbench.open_settings_ui")],
    probes: PROBES,
    ..DEFAULT
};

const PROBES: &[Probe] = &[
    // The section index's current section, Common, in its padding left of
    // its name.
    probe(297., 221., "list.row_selected_inactive"),
    // The accent dot before the rail position's label: the file sets it.
    probe(501., 354., "accent.base"),
    // The list, in the gutter right of the rows' column.
    probe(990., 300., "surface.background"),
];
