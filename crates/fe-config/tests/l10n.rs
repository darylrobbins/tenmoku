//! fe-config's catalogue: every key its source looks up is in its en-US
//! resources, and every `.ftl` it ships parses and is listed.

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &fe_config::l10n::RESOURCES).check();
}
