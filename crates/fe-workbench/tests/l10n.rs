//! fe-workbench's catalogue checks (`docs/decisions/xte5e7hqsw-i18n-design.md`
//! § 8): no hard-coded user-facing literal in its migrated files, every key
//! it uses exists, and every command it declares or registers is titled from
//! its `command-<id>` message.

use std::collections::BTreeMap;

use kiln::__private::{CommandRecord, inventory};

#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &fe_workbench::l10n::RESOURCES).check();
}

/// Every command of fe-workbench's, by ID, with the English title it's
/// declared or registered with: its `#[kiln::command]`s, as `inventory`
/// recorded them, and its registries' (the palette's list, less the
/// editor's, which are fe-gpui's).
fn commands() -> BTreeMap<String, String> {
    let mut commands = BTreeMap::new();
    for record in inventory::iter::<CommandRecord>() {
        if record.krate == "fe_workbench" {
            commands.insert(record.id.to_owned(), record.title.to_owned());
        }
    }
    let editor: Vec<String> = (fe_gpui::commands::command_list().into_iter())
        .map(|meta| meta.id.to_string())
        .collect();
    for meta in fe_workbench::command_list() {
        if !editor.contains(&meta.id.to_string()) {
            commands.insert(meta.id.to_string(), meta.title.clone());
        }
    }
    commands
}

/// Every command of fe-workbench's has its `command-<id>` message, the key
/// kiln-command's resolver titles it by, with the English it's declared
/// with, byte for byte; and every `command-*` message is one of its
/// commands'.
#[test]
fn every_command_has_its_key() {
    let commands = commands();
    assert!(commands.len() > 300, "{}", commands.len());
    let errors =
        fe_command::l10n::check_command_keys(&fe_workbench::l10n::RESOURCES, commands.keys());
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    for (id, title) in &commands {
        let key = fe_command::l10n::command_key(id);
        assert_eq!(kiln_l10n::message(&key), title.as_str(), "{key}");
        assert_eq!(
            fe_command::l10n::command_label(id, ""),
            title.as_str(),
            "{id}"
        );
    }
}
