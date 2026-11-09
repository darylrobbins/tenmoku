//! fe-vim's catalogue checks (`docs/decisions/xte5e7hqsw-i18n-design.md` § 8).

#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &fe_vim::l10n::RESOURCES).check();
}

/// Every command vim registers is titled and described from its
/// `command-<id>` message (the key kiln-command's resolver gives it), and
/// every `command-*` message is a registered command's.
#[test]
fn every_command_has_its_key() {
    let registry = fe_vim::registry();
    let mut keys = std::collections::BTreeSet::new();
    for meta in registry.commands() {
        let key = format!("command-{}", meta.id.as_str().replace('.', "-"));
        assert!(
            kiln_l10n::try_format(&key, &kiln_l10n::Args::new()).is_some(),
            "{} has no {key}",
            meta.id
        );
        assert_eq!(meta.title, kiln_l10n::message(&key).as_str(), "{key}");
        let description = kiln_l10n::attribute(&key, "description", &kiln_l10n::Args::new());
        assert!(
            description.is_some_and(|d| !d.is_empty() && d.as_str() == meta.description),
            "{key} has no .description"
        );
        keys.insert(key);
    }
    let source = include_str!("../l10n/en-US/commands.ftl");
    for line in source.lines() {
        if let Some((key, _)) = line.split_once(" = ")
            && key.starts_with("command-")
        {
            assert!(keys.contains(key), "{key} names no command of vim's");
        }
    }
}
