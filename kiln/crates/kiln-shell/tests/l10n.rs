// SPDX-License-Identifier: Apache-2.0

//! kiln-shell's catalogue: the literal lint (kiln-l10n), no hard-coded
//! user-facing text in this crate's non-test source outside
//! `l10n-allow.toml`, and every key it uses exists.

#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &kiln_shell::l10n::RESOURCES).check();
}
