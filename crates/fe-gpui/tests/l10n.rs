//! The editor view's strings are in its catalogue: no hard-coded user-facing
//! literal in its source, and every key it uses exists.

#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &fe_gpui::l10n::RESOURCES).check();
}
