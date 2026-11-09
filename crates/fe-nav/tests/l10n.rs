// SPDX-License-Identifier: Apache-2.0

//! The literal lint (kiln-l10n): no hard-coded user-facing text in this
//! crate's non-test source outside `l10n-allow.toml`.

#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &fe_nav::l10n::RESOURCES).check();
}
