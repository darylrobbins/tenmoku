//! [`SettingsModel`]: the Settings editor's rows, from the schema, with the
//! user's and the project's values.

use std::collections::BTreeSet;

use fe_config::{
    EnumVariant, SettingEntry, SettingKind, Settings, SettingsModel, SettingsStore, ValueSource,
    settings_schema,
};
use serde_json::{Value, json};

fn entry<'a>(model: &'a SettingsModel, key: &str) -> &'a SettingEntry {
    model
        .entries()
        .iter()
        .find(|entry| entry.key_path() == key)
        .unwrap_or_else(|| panic!("no row for {key}"))
}

fn keys(entries: &[&SettingEntry]) -> Vec<String> {
    entries.iter().map(|entry| entry.key_path()).collect()
}

/// The dotted keys of every leaf of `schema`'s table, found apart from the
/// model: a property is a table if what it refers to has `properties`.
fn schema_leaves(schema: &Value, defs: &Value, prefix: &str, leaves: &mut Vec<String>) {
    let resolve = |schema: &Value| -> Value {
        match schema.get("$ref").and_then(Value::as_str) {
            Some(path) => defs[path.trim_start_matches("#/$defs/")].clone(),
            None => schema.clone(),
        }
    };
    let schema = resolve(schema);
    for (name, property) in schema["properties"].as_object().unwrap() {
        let key = format!("{prefix}{name}");
        let property = resolve(property);
        if property.get("properties").is_some() {
            schema_leaves(&property, defs, &format!("{key}."), leaves);
        } else {
            leaves.push(key);
        }
    }
}

/// The dotted keys of `value`'s leaves, stopping at `rows`.
fn value_leaves(value: &Value, prefix: &str, rows: &BTreeSet<String>, leaves: &mut Vec<String>) {
    for (name, value) in value.as_object().unwrap() {
        let key = format!("{prefix}{name}");
        if value.is_object() && !rows.contains(&key) {
            value_leaves(value, &format!("{key}."), rows, leaves);
        } else {
            leaves.push(key);
        }
    }
}

#[test]
fn every_schema_leaf_has_one_row() {
    let model = SettingsModel::new(None, None);
    let rows: Vec<String> = model.entries().iter().map(SettingEntry::key_path).collect();
    let unique: BTreeSet<String> = rows.iter().cloned().collect();
    assert_eq!(unique.len(), rows.len(), "a key has two rows: {rows:?}");

    let schema = settings_schema();
    let mut leaves = Vec::new();
    schema_leaves(&schema, &schema["$defs"], "", &mut leaves);
    assert_eq!(rows, leaves, "rows in schema order, one per leaf");

    // Every setting the defaults serialize is a row; those they don't are
    // unset by default.
    let mut defaults = Vec::new();
    let settings = serde_json::to_value(Settings::default()).unwrap();
    value_leaves(&settings, "", &unique, &mut defaults);
    for key in &defaults {
        assert!(unique.contains(key), "{key} is a default but no row");
    }
    for entry in model.entries() {
        if !defaults.contains(&entry.key_path()) {
            assert_eq!(entry.default, Value::Null, "{}", entry.key_path());
        }
    }

    for key in [
        "editor.tab_size",
        "editor.minimap.enabled",
        "editor.sticky_scroll.enabled",
        "lsp.servers",
        "lsp.timeouts.hover",
        "vim.remap",
        "chat.model",
        "theme.mode",
        "status_bar.visible",
    ] {
        assert!(unique.contains(key), "{key}");
    }
    for key in [
        "editor.minimap",
        "lsp.servers.enabled",
        "vim.remap.from",
        "editor",
    ] {
        assert!(!unique.contains(key), "{key} isn't a leaf");
    }
}

#[test]
fn kinds_follow_the_schema() {
    let model = SettingsModel::new(None, None);
    let kind = |key| entry(&model, key).kind.clone();
    assert_eq!(kind("editor.soft_wrap"), SettingKind::Bool);
    assert_eq!(kind("editor.code_actions_on_save"), SettingKind::TextList);
    assert_eq!(kind("terminal.shell_args"), SettingKind::TextList);
    assert_eq!(kind("vim.leader"), SettingKind::Text { min_length: None });
    assert_eq!(kind("lsp.servers"), SettingKind::Other);
    assert_eq!(kind("vim.remap"), SettingKind::Other);
    assert!(matches!(kind("editor.tab_size"), SettingKind::Int { .. }));
    assert!(matches!(
        kind("editor.font_size"),
        SettingKind::Float { .. }
    ));
    // Optional: a string or nothing.
    assert_eq!(
        kind("editor.font_family"),
        SettingKind::Text {
            min_length: Some(1)
        }
    );
    assert!(matches!(
        kind("terminal.font_size"),
        SettingKind::Float { .. }
    ));

    let SettingKind::Enum(variants) = kind("editor.minimap.side") else {
        panic!("editor.minimap.side is an enum");
    };
    let values: Vec<&str> = variants
        .iter()
        .map(|variant| variant.value.as_str())
        .collect();
    assert_eq!(values, ["right", "left"]);
    let SettingKind::Enum(variants) = kind("editor.auto_save") else {
        panic!("editor.auto_save is an enum");
    };
    assert_eq!(
        variants[0],
        EnumVariant {
            value: "off".into(),
            description: "Files save only when you save them.".into(),
        }
    );
    assert_eq!(variants.len(), 5);
    let SettingKind::Enum(variants) = kind("lsp.typescript_server") else {
        panic!("lsp.typescript_server is an enum");
    };
    let values: Vec<&str> = variants
        .iter()
        .map(|variant| variant.value.as_str())
        .collect();
    assert_eq!(values, ["typescript-language-server", "typescript-go"]);
    // The empty string is a variant like any other.
    assert!(matches!(kind("vim.clipboard"), SettingKind::Enum(v) if v[0].value.is_empty()));

    let tab_size = entry(&model, "editor.tab_size");
    assert_eq!(tab_size.default, json!(4));
    assert_eq!(tab_size.group, "Editor");
    assert!(!tab_size.description.is_empty());
    assert_eq!(entry(&model, "editor.minimap.side").default, json!("right"));
    assert_eq!(entry(&model, "editor.font_family").default, Value::Null);
}

#[test]
fn ranges_come_from_schemars_attributes() {
    let model = SettingsModel::new(None, None);
    let kind = |key| entry(&model, key).kind.clone();
    // #[schemars(range(min = 1, max = 16))]
    assert_eq!(
        kind("editor.tab_size"),
        SettingKind::Int {
            min: Some(1),
            max: Some(16)
        }
    );
    // #[schemars(range(min = 6.0, max = 72.0))]
    assert_eq!(
        kind("editor.font_size"),
        SettingKind::Float {
            min: Some(6.0),
            max: Some(72.0)
        }
    );
    // Only a minimum.
    assert_eq!(
        kind("lsp.timeouts.hover"),
        SettingKind::Int {
            min: Some(1),
            max: None
        }
    );
    assert_eq!(
        kind("chat.max_tokens"),
        SettingKind::Int {
            min: Some(1024),
            max: Some(128_000)
        }
    );
    // #[schemars(length(min = 1))]
    assert_eq!(
        kind("chat.model"),
        SettingKind::Text {
            min_length: Some(1)
        }
    );
    assert_eq!(
        kind("theme.name"),
        SettingKind::Text {
            min_length: Some(1)
        }
    );
}

#[test]
fn source_precedence_is_workspace_over_user_over_default() {
    let user = "[editor]\ntab_size = 2\nsoft_wrap = true\n";
    let project = "[editor]\ntab_size = 8\n";
    let model = SettingsModel::new(Some(user), Some(project));
    assert_eq!(model.errors(), [] as [String; 0]);

    let tab_size = model.value(entry(&model, "editor.tab_size"));
    assert_eq!(
        (tab_size.value, tab_size.source),
        (json!(8), ValueSource::Workspace)
    );
    let soft_wrap = model.value(entry(&model, "editor.soft_wrap"));
    assert_eq!(
        (soft_wrap.value, soft_wrap.source),
        (json!(true), ValueSource::User)
    );
    let font_size = model.value(entry(&model, "editor.font_size"));
    assert_eq!(
        (font_size.value, font_size.source),
        (json!(13.0), ValueSource::Default)
    );

    let tab_size = entry(&model, "editor.tab_size");
    assert_eq!(model.user_value(tab_size), Some(json!(2)));
    assert_eq!(model.workspace_value(tab_size), Some(json!(8)));
    let font_size = entry(&model, "editor.font_size");
    assert_eq!(model.user_value(font_size), None);
    assert_eq!(model.workspace_value(font_size), None);

    // Nested tables and tables of tables.
    let user = "[editor.minimap]\nside = \"left\"\n\n[lsp.servers.gopls]\ncommand = \"gopls\"\n";
    let project = "[lsp.servers.gopls]\nenabled = false\n";
    let model = SettingsModel::new(Some(user), Some(project));
    let side = model.value(entry(&model, "editor.minimap.side"));
    assert_eq!(
        (side.value, side.source),
        (json!("left"), ValueSource::User)
    );
    let servers = model.value(entry(&model, "lsp.servers"));
    assert_eq!(servers.source, ValueSource::Workspace);
    assert_eq!(
        servers.value,
        json!({"gopls": {"command": "gopls", "enabled": false}})
    );
    // A table in a table of tables gets its keys' defaults.
    let model = SettingsModel::new(
        Some(
            "[lsp.servers.gopls]\ncommand = \"gopls\"\n\n[[vim.remap]]\nmode = \"n\"\nfrom = \"H\"\nto = \"^\"\n",
        ),
        None,
    );
    assert_eq!(
        model.value(entry(&model, "lsp.servers")).value,
        json!({"gopls": {"command": "gopls", "enabled": true}})
    );
    assert_eq!(
        model.value(entry(&model, "vim.remap")).value,
        json!([{"mode": "n", "from": "H", "to": "^", "recursive": false}])
    );
    assert_eq!(
        model.user_value(entry(&model, "lsp.servers")),
        Some(json!({"gopls": {"command": "gopls"}}))
    );
}

#[test]
fn values_agree_with_the_settings_store() {
    let user = "[editor]\ntab_size = 2\n\n[vim]\nshell = \"zsh\"\n\n[[vim.remap]]\nmode = \"n\"\nfrom = \"H\"\nto = \"^\"\n\n[lsp.timeouts]\nhover = 900\n\n[lsp.servers.gopls]\ncommand = \"gopls\"\n\n[lsp.servers.pyright]\nargs = [\"--stdio\"]\n";
    let project = "[editor]\ntab_size = 8\nsoft_wrap = true\n\n[vim]\nshell = \"sh\"\nenabled = true\n\n[lsp]\nenabled = false\n\n[lsp.timeouts]\nhover = 1\n\n[chat]\nmodel = \"other\"\n";
    let dir = tempfile::tempdir().unwrap();
    let (user_path, project_path) = (
        dir.path().join("user.toml"),
        dir.path().join("project.toml"),
    );
    std::fs::write(&user_path, user).unwrap();
    std::fs::write(&project_path, project).unwrap();
    let store = SettingsStore::load(Some(user_path), Some(project_path));
    let settings = serde_json::to_value(store.settings().as_ref()).unwrap();

    let model = SettingsModel::new(Some(user), Some(project));
    for entry in model.entries() {
        let mut expected = &settings;
        for name in &entry.key {
            expected = expected.get(name).unwrap_or(&Value::Null);
        }
        assert_eq!(&model.value(entry).value, expected, "{}", entry.key_path());
    }
}

#[test]
fn modified_even_when_equal_to_default() {
    let model = SettingsModel::new(
        Some("[editor]\ntab_size = 4\n"),
        Some("[vim]\nenabled = false\n"),
    );
    let tab_size = entry(&model, "editor.tab_size");
    assert_eq!(tab_size.default, json!(4));
    assert!(model.is_modified(tab_size));
    assert_eq!(model.value(tab_size).source, ValueSource::User);
    let vim = entry(&model, "vim.enabled");
    assert!(model.is_modified(vim));
    assert_eq!(model.value(vim).source, ValueSource::Workspace);
    assert!(!model.is_modified(entry(&model, "editor.soft_wrap")));
    let modified: Vec<String> = model
        .entries()
        .iter()
        .filter(|entry| model.is_modified(entry))
        .map(SettingEntry::key_path)
        .collect();
    assert_eq!(modified, ["editor.tab_size", "vim.enabled"]);
}

#[test]
fn bad_toml_leaves_defaults() {
    let model = SettingsModel::new(Some("[editor\ntab_size = 2\n"), Some("tab_size = = 3"));
    assert_eq!(model.errors().len(), 2, "{:?}", model.errors());
    assert!(model.errors()[0].starts_with("user settings: "));
    assert!(model.errors()[1].starts_with("workspace settings: "));
    for entry in model.entries() {
        let value = model.value(entry);
        assert_eq!(value.source, ValueSource::Default, "{}", entry.key_path());
        assert_eq!(value.value, entry.default, "{}", entry.key_path());
        assert!(!model.is_modified(entry));
    }

    // One bad file doesn't take the other with it.
    let model = SettingsModel::new(Some("[editor]\ntab_size = 2\n"), Some("[[["));
    assert_eq!(model.errors().len(), 1);
    assert_eq!(
        model.value(entry(&model, "editor.tab_size")).value,
        json!(2)
    );

    // A table where a value goes, or a value where a table goes.
    let model = SettingsModel::new(Some("editor = 3\n[vim.enabled]\nx = 1\n"), None);
    assert_eq!(model.errors(), [] as [String; 0]);
    assert_eq!(
        model.value(entry(&model, "editor.tab_size")).source,
        ValueSource::Default
    );
    assert_eq!(
        model.value(entry(&model, "vim.enabled")).value,
        json!({"x": 1})
    );
}

#[test]
fn project_only_restrictions_are_flagged() {
    let project = "[chat]\nmodel = \"x\"\n\n[terminal]\nshell = \"sh\"\nscrollback = 5\n\n[vim]\nshell = \"sh\"\nincsearch = false\n\n[theme]\nmode = \"light\"\n\n[lsp]\nenabled = true\nmax_file_bytes = 1\n\n[lsp.servers.pyright]\nenabled = false\ncommand = \"evil\"\n";
    let model = SettingsModel::new(None, Some(project));
    let allowed = |key| model.workspace_allowed(entry(&model, key));
    for key in [
        "chat.model",
        "chat.effort",
        "terminal.shell",
        "terminal.shell_args",
        "vim.shell",
        "vim.remap",
        "theme.mode",
        "lsp.max_file_bytes",
        "lsp.typescript_server",
        "lsp.timeouts.hover",
    ] {
        assert!(!allowed(key), "{key}");
    }
    for key in [
        "lsp.enabled",
        "lsp.servers",
        "terminal.scrollback",
        "vim.incsearch",
        "editor.tab_size",
    ] {
        assert!(allowed(key), "{key}");
    }

    // An ignored key is the project's as written, and modified, but not
    // effective.
    let model_value = |key| model.value(entry(&model, key));
    let chat = entry(&model, "chat.model");
    assert_eq!(model.workspace_value(chat), Some(json!("x")));
    assert_eq!(model_value("chat.model").source, ValueSource::Default);
    for key in [
        "chat.model",
        "terminal.shell",
        "vim.shell",
        "theme.mode",
        "lsp.enabled",
    ] {
        assert!(model.is_modified(entry(&model, key)), "{key}");
    }
    assert!(!model.is_modified(entry(&model, "chat.effort")));
    assert_eq!(model_value("terminal.shell").source, ValueSource::Default);
    assert_eq!(model_value("terminal.scrollback").value, json!(5));
    assert_eq!(model_value("vim.shell").source, ValueSource::Default);
    assert_eq!(model_value("vim.incsearch").value, json!(false));
    assert_eq!(model_value("theme.mode").value, json!("dark"));
    // A project may only turn language servers off.
    assert_eq!(model_value("lsp.enabled").source, ValueSource::Default);
    assert_eq!(
        model_value("lsp.max_file_bytes").source,
        ValueSource::Default
    );
    assert_eq!(
        model_value("lsp.servers").value,
        json!({"pyright": {"enabled": false}})
    );
}

#[test]
fn search_matches_key_description_and_group() {
    let model = SettingsModel::new(None, None);
    assert_eq!(model.search("").len(), model.entries().len());
    assert_eq!(model.search("   ").len(), model.entries().len());

    // By key, ignoring case.
    assert_eq!(keys(&model.search("TAB_SIZE")), ["editor.tab_size"]);
    // By group title: every row of `Editor > Sticky Scroll`.
    assert_eq!(
        keys(&model.search("sticky scroll")),
        [
            "editor.sticky_scroll.default_model",
            "editor.sticky_scroll.enabled",
            "editor.sticky_scroll.max_line_count",
            "editor.sticky_scroll.scroll_with_editor",
        ]
    );
    let minimap = keys(&model.search("Minimap"));
    assert!(
        minimap.contains(&"editor.minimap.scale".to_owned()),
        "{minimap:?}"
    );
    // By description only.
    let description = entry(&model, "editor.auto_close_pairs").description.clone();
    let word = description
        .split_whitespace()
        .max_by_key(|word| word.len())
        .unwrap();
    assert!(keys(&model.search(word)).contains(&"editor.auto_close_pairs".to_owned()));
    assert_eq!(
        model.search("no-such-setting-anywhere"),
        [] as [&SettingEntry; 0]
    );

    // Schema order.
    let all = keys(&model.search(""));
    let found = keys(&model.search("enabled"));
    let mut in_order = found.clone();
    in_order.sort_by_key(|key| all.iter().position(|row| row == key));
    assert_eq!(found, in_order);
    assert!(found.len() > 2);
}

#[test]
fn search_terms_are_anded() {
    let model = SettingsModel::new(None, None);
    let minimap = model.search("minimap");
    let side = model.search("side");
    let both = keys(&model.search("minimap side"));
    assert!(both.contains(&"editor.minimap.side".to_owned()));
    for key in &both {
        assert!(
            keys(&minimap).contains(key) && keys(&side).contains(key),
            "{key}"
        );
    }
    assert!(both.len() < minimap.len());
    assert_eq!(keys(&model.search("side minimap")), both);
    assert_eq!(
        model.search("minimap no-such-word"),
        [] as [&SettingEntry; 0]
    );
}

#[test]
fn at_modified_filter() {
    let user = "[editor]\ntab_size = 2\nsoft_wrap = false\n";
    let project = "[editor]\nfont_size = 14.0\n\n[chat]\nmodel = \"x\"\n";
    let model = SettingsModel::new(Some(user), Some(project));
    assert_eq!(
        keys(&model.search("@modified")),
        [
            "chat.model",
            "editor.font_size",
            "editor.soft_wrap",
            "editor.tab_size"
        ]
    );
    assert_eq!(keys(&model.search("@Modified tab")), ["editor.tab_size"]);
    assert_eq!(keys(&model.search("@modified vim")), [] as [String; 0]);
    assert_eq!(
        keys(&model.search("@user")),
        ["editor.soft_wrap", "editor.tab_size"]
    );
    // What the project's file sets, even where it's ignored.
    assert_eq!(
        keys(&model.search("@workspace")),
        ["chat.model", "editor.font_size"]
    );
    assert_eq!(
        keys(&model.search("@workspace @modified")),
        ["chat.model", "editor.font_size"]
    );
    assert_eq!(keys(&model.search("@user font")), [] as [String; 0]);
}

#[test]
fn groups_are_titled_and_nested() {
    let model = SettingsModel::new(None, None);
    let titles: Vec<&str> = model
        .groups()
        .iter()
        .map(|group| group.title.as_str())
        .collect();
    for title in [
        "Editor",
        "Editor > Minimap",
        "Editor > Sticky Scroll",
        "LSP",
        "LSP > Timeouts",
        "Status Bar",
        "Chat",
        "Vim",
    ] {
        assert!(titles.contains(&title), "{title} in {titles:?}");
    }
    // A table comes before those nested in it, in schema order.
    let at = |title| titles.iter().position(|found| *found == title).unwrap();
    assert!(at("Editor") < at("Editor > Minimap"));
    assert!(at("Editor > Minimap") < at("Editor > Sticky Scroll"));
    assert!(at("Chat") < at("Editor"));

    let minimap = &model.groups()[at("Editor > Minimap")];
    assert_eq!(minimap.key, ["editor", "minimap"]);
    assert_eq!(minimap.count, 7);
    assert!(!minimap.description.is_empty());
    let status_bar = &model.groups()[at("Status Bar")];
    assert_eq!(status_bar.count, 1);

    // Each row is in its group, and the counts cover every row once.
    for group in model.groups() {
        let rows = model
            .entries()
            .iter()
            .filter(|entry| entry.group == group.title)
            .count();
        assert_eq!(rows, group.count, "{}", group.title);
    }
    let total: usize = model.groups().iter().map(|group| group.count).sum();
    assert_eq!(total, model.entries().len());
    assert_eq!(entry(&model, "lsp.timeouts.hover").group, "LSP > Timeouts");
}
