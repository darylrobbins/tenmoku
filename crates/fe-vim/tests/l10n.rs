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

/// Every `fegroup` of the bundled LazyVim preset has its `vim-group-<slug>`
/// message, equal to the name as written (en-US is unchanged), and a user's
/// own group name isn't looked up.
#[test]
fn every_bundled_fegroup_has_its_key() {
    let preset = include_str!("../../fastedit/presets/lazyvim.vimrc");
    let parsed = fe_vim::parse_vimrc_with_leader(preset, &[fe_vim::VimKey::Char(' ')]);
    assert!(!parsed.groups.is_empty());
    for group in &parsed.groups {
        let resolved = fe_vim::preset_group_name(&group.name);
        assert_eq!(resolved, group.name);
        let key = format!("vim-group-{}", group.name.replace('/', "-"));
        assert!(
            kiln_l10n::try_format(&key, &kiln_l10n::Args::new()).is_some(),
            "{} has no {key}",
            group.name
        );
    }
    let source = include_str!("../l10n/en-US/vim.ftl");
    let keys = source
        .lines()
        .filter(|l| l.starts_with("vim-group-"))
        .count();
    assert_eq!(
        keys,
        parsed
            .groups
            .iter()
            .map(|g| &g.name)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    );
}

/// Every built-in which-key title is a message.
#[test]
fn every_builtin_key_has_its_message() {
    for table in fe_vim::BUILTIN_KEYS {
        for (_, key) in table.keys {
            assert!(
                kiln_l10n::try_format(key, &kiln_l10n::Args::new()).is_some(),
                "{key}"
            );
        }
    }
}
