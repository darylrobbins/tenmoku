//! fe-editor's catalogue checks (`docs/decisions/xte5e7hqsw-i18n-design.md`
//! § 8): no hard-coded user-facing literal in its source, and every key it
//! uses exists.

#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &fe_editor::l10n::RESOURCES).check();
}

/// Every command in fe-editor's registry has its `command-<id>` message,
/// the key kiln-command's resolver titles it by, and every `command-*`
/// message is one of its commands'; the two with a description have it as
/// that message's `.description`, which they register with.
#[test]
fn every_command_has_its_key() {
    let registry = fe_editor::commands::registry();
    let ids: Vec<String> = registry
        .commands()
        .map(|meta| meta.id.to_string())
        .collect();
    assert_eq!(ids.len(), 104, "{ids:?}");
    let errors = fe_command::l10n::check_command_keys(&fe_editor::l10n::RESOURCES, &ids);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    for meta in registry.commands() {
        let id = meta.id.to_string();
        let key = fe_command::l10n::command_key(&id);
        assert_eq!(kiln_l10n::message(&key), meta.title.as_str(), "{id}");
        let description = kiln_l10n::attribute(&key, "description", &kiln_l10n::Args::new());
        match id.as_str() {
            "editor.insert" | "editor.move" => {
                assert_eq!(
                    description.as_deref(),
                    Some(meta.description.as_str()),
                    "{id}"
                );
                assert!(!meta.description.is_empty(), "{id}");
            }
            _ => assert_eq!(description, None, "{id}"),
        }
    }
}

/// The messages moved to the catalogue read, in en-US, byte for byte as
/// they did when they were formatted in place.
#[test]
fn messages_read_as_before() {
    use std::path::PathBuf;

    use fe_editor::{ChangesetError, CopyError, SaveError};

    let path = PathBuf::from("src/a.rs");
    let changeset = [
        (
            ChangesetError::Conflict { path: path.clone() },
            "src/a.rs changed since it was proposed",
        ),
        (
            ChangesetError::NoSuchFile { path: path.clone() },
            "src/a.rs is not in the changeset",
        ),
        (
            ChangesetError::NotPending { path },
            "that hunk of src/a.rs is not pending",
        ),
    ];
    for (error, english) in changeset {
        assert_eq!(error.to_string(), english);
    }
    assert_eq!(
        CopyError::TooLarge { len: 70_000_000 }.to_string(),
        "the selection is 70000000 bytes, more than the 67108864 a copy takes"
    );
    assert_eq!(
        SaveError::ChangedOnDisk.to_string(),
        "the file changed on disk since it was read"
    );
    let untitled = fe_editor::EditorModel::new(fe_buffer::Buffer::new("x"), None);
    let error = untitled.prepare_save(false).err().expect("no path");
    assert_eq!(error.to_string(), "buffer has no path");
    let error = serde_json::from_value::<fe_editor::commands::MoveArgs>(serde_json::json!({
        "motion": "left",
        "count": 10_001,
    }))
    .expect_err("too many");
    assert!(
        error
            .to_string()
            .starts_with("count 10001 is more than 10000"),
        "{error}"
    );
    for (key, english) in [
        ("editor-undo-failed", "undo failed: boom"),
        ("editor-redo-failed", "redo failed: boom"),
        ("editor-save-failed", "save failed: boom"),
    ] {
        assert_eq!(kiln_l10n::tr!(key, error = "boom"), english);
    }
    for (key, english) in [
        ("editor-save-is-a-directory", "Is a directory"),
        ("editor-save-not-a-regular-file", "not a regular file"),
        (
            "editor-save-symlink-loop",
            "too many levels of symbolic links",
        ),
    ] {
        assert_eq!(kiln_l10n::tr!(key), english);
    }
}
