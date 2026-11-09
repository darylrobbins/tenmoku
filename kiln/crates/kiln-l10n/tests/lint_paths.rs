// SPDX-License-Identifier: Apache-2.0

//! Paths in lint messages use `/` on every OS (Windows prints `\` natively).

use std::path::Path;

use kiln_l10n::lint::CatalogueCheck;

#[test]
fn backslash_components_render_with_slashes() {
    let path = Path::new("l10n\\en-US\\alpha.ftl");
    assert_eq!(CatalogueCheck::slash_path(path), "l10n/en-US/alpha.ftl");
}

#[test]
fn slash_components_are_unchanged() {
    let path = Path::new("l10n/en-US/alpha.ftl");
    assert_eq!(CatalogueCheck::slash_path(path), "l10n/en-US/alpha.ftl");
}
