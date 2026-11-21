//! The user guide's generated reference pages (`docs/user/src/reference/`)
//! match the code: `examples/gen_user_docs.rs`, run in check mode, finds
//! none stale; every settings key has its heading and every command its
//! row; and the text is the same whatever host makes it. A setting, binding
//! or command changed without regenerating fails here with the command to
//! run.

use std::{collections::BTreeSet, path::Path};

use fe_command::{CommandId, Value};
use fe_gpui::fe_keymap::{self, DEFAULT_KEYMAP_PATH, ItemKind, KeymapLayer};
use kiln::core::Os;

#[allow(dead_code, unreachable_pub)]
#[path = "../examples/gen_user_docs.rs"]
mod gen_user_docs;

fn page_text(file: &str) -> String {
    let path = gen_user_docs::repo_root()
        .join(gen_user_docs::DIR)
        .join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{file}: {error}"))
}

#[test]
fn reference_pages_are_up_to_date() {
    let stale = gen_user_docs::stale(&gen_user_docs::repo_root());
    assert!(
        stale.is_empty(),
        "the user guide's reference pages are stale; regenerate them with `{}`\n\n{}",
        gen_user_docs::REGENERATE,
        stale.join("\n\n")
    );
}

/// Every key path of the settings schema, read here on its own: a table's
/// keys by dotted path, `<name>` for a key of a table of tables, and the
/// tables themselves apart (a struct, a map of structs or a list of
/// structs is a table, not a key).
fn setting_paths(schema: &Value) -> (BTreeSet<String>, BTreeSet<String>) {
    fn resolve<'a>(schema: &'a Value, mut node: &'a Value) -> &'a Value {
        loop {
            if let Some(reference) = node.get("$ref").and_then(Value::as_str) {
                let name = reference.trim_start_matches("#/$defs/");
                node = &schema["$defs"][name];
                continue;
            }
            // `Option<T>`: `anyOf: [T, { type: null }]`.
            if let Some([one, two]) = node
                .get("anyOf")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
            {
                let null = |node: &Value| node.get("type").and_then(Value::as_str) == Some("null");
                if null(two) {
                    node = one;
                    continue;
                }
                if null(one) {
                    node = two;
                    continue;
                }
            }
            return node;
        }
    }
    fn walk(
        schema: &Value,
        node: &Value,
        path: &str,
        keys: &mut BTreeSet<String>,
        tables: &mut BTreeSet<String>,
    ) {
        let node = resolve(schema, node);
        for (name, property) in node["properties"].as_object().expect("a table") {
            let key = if path.is_empty() {
                name.clone()
            } else {
                format!("{path}.{name}")
            };
            let property = resolve(schema, property);
            let map = property
                .get("additionalProperties")
                .map(|values| resolve(schema, values))
                .filter(|values| values.get("properties").is_some());
            let list = (property.get("type").and_then(Value::as_str) == Some("array"))
                .then(|| property.get("items"))
                .flatten()
                .map(|items| resolve(schema, items))
                .filter(|items| items.get("properties").is_some());
            if property.get("properties").is_some() {
                tables.insert(key.clone());
                walk(schema, property, &key, keys, tables);
            } else if let Some(values) = map {
                let key = format!("{key}.<name>");
                tables.insert(key.clone());
                walk(schema, values, &key, keys, tables);
            } else if let Some(items) = list {
                tables.insert(key.clone());
                walk(schema, items, &key, keys, tables);
            } else {
                keys.insert(key);
            }
        }
    }
    let (mut keys, mut tables) = (BTreeSet::new(), BTreeSet::new());
    walk(schema, schema, "", &mut keys, &mut tables);
    (keys, tables)
}

/// The page's `## ` and `### ` headings that name a key or table.
fn headings(page: &str, prefix: &str) -> BTreeSet<String> {
    (page.lines())
        .filter_map(|line| line.strip_prefix(prefix))
        .filter_map(|name| name.strip_prefix('`')?.strip_suffix('`'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn generated_pages_cover_every_setting_key() {
    let schema = fe_config::settings_schema();
    let (keys, tables) = setting_paths(&schema);
    assert!(keys.len() > 150, "{} keys", keys.len());
    let page = page_text(gen_user_docs::SETTINGS);
    assert_eq!(headings(&page, "### "), keys, "one heading a key, no other");
    assert_eq!(
        headings(&page, "## "),
        tables,
        "one section a table, no other"
    );
    // And each key's section says its type.
    let sections = page.split("\n### `").skip(1);
    for section in sections {
        let key = section.split('`').next().unwrap_or_default();
        assert!(section.contains("\n- Type: "), "{key} has no type");
    }
}

/// Every command the default keymaps and menus name on any platform, or
/// the palette lists: what the catalog knows of those.
fn named_commands() -> BTreeSet<String> {
    let mut named: Vec<(CommandId, Value)> = Vec::new();
    for keymap in [fe_keymap::DEFAULT_MACOS, fe_keymap::DEFAULT_LINUX] {
        let (layer, diagnostics) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), keymap, None);
        assert_eq!(diagnostics, []);
        for entry in layer.expect("the built-in keymap parses").entries {
            if let Some(command) = entry.command {
                named.push((command, entry.args));
            }
        }
    }
    for (bar, diagnostics) in [
        fe_keymap::default_menus_macos(None),
        fe_keymap::default_menus_linux(None),
    ] {
        assert_eq!(diagnostics, []);
        let bar = bar.expect("the default menus parse");
        for item in bar.menus.iter().flat_map(|menu| &menu.items) {
            if let ItemKind::Command(command) = &item.kind {
                named.push((command.command.clone(), command.args.clone()));
                for alternative in &command.alternatives {
                    named.push((alternative.command.clone(), alternative.args.clone()));
                }
            }
        }
    }
    for meta in fe_workbench::command_list() {
        named.push((meta.id, Value::Null));
    }
    let catalog = fe_workbench::command_catalog();
    (named.into_iter())
        .filter(|(id, args)| {
            catalog.check(id, args) != Err(fe_keymap::CatalogError::UnknownCommand)
        })
        .map(|(id, _)| id.to_string())
        .collect()
}

/// Every command the catalog is built from: the typed ones kiln recorded
/// for the crates it checks them for, and every registry's, the editor's
/// (fe-gpui's: the proposal, diff, image, hex and app ones too) apart.
fn catalog_commands() -> BTreeSet<String> {
    let mut commands = BTreeSet::new();
    for record in kiln::__private::inventory::iter::<kiln::__private::CommandRecord>() {
        if fe_workbench::RECORDED_COMMAND_CRATES.contains(&record.krate) {
            commands.insert(record.id.to_owned());
        }
    }
    for (id, _) in fe_workbench::registry_commands() {
        commands.insert(id.to_string());
    }
    for meta in fe_gpui::commands::catalog_commands() {
        commands.insert(meta.id.to_string());
    }
    commands
}

#[test]
fn every_command_in_the_catalog_has_a_row() {
    let page = page_text(gen_user_docs::COMMANDS);
    let rows: BTreeSet<String> = (page.lines())
        .filter_map(|line| line.strip_prefix("| `"))
        .filter_map(|line| line.split_once('`'))
        .map(|(id, _)| id.to_owned())
        .collect();
    let commands = catalog_commands();
    assert!(commands.len() > 300, "{} commands", commands.len());
    assert_eq!(rows, commands, "one row a catalog command, no other");
    // Bound or not: some of the registries' commands no keymap, menu or
    // palette list names, and they have rows too.
    let named = named_commands();
    assert!(named.is_subset(&rows));
    let unbound: Vec<&String> = commands.difference(&named).collect();
    assert!(!unbound.is_empty(), "every command is named somewhere");
    // And every row is the catalog's.
    let catalog = fe_workbench::command_catalog();
    for id in &rows {
        let id = CommandId::parse(id).unwrap_or_else(|error| panic!("{id}: {error}"));
        assert_ne!(
            catalog.check(&id, &Value::Null),
            Err(fe_keymap::CatalogError::UnknownCommand),
            "{id} has a row but isn't in the catalog"
        );
    }
}

/// Every page has its H1 and says it's generated, and the text is plain:
/// LF only, no tabs, no trailing spaces, no path of this checkout's.
#[test]
fn pages_are_marked_and_host_free() {
    let root = gen_user_docs::repo_root()
        .canonicalize()
        .expect("the checkout's root");
    let root = root.to_string_lossy();
    let home = std::env::var("HOME").unwrap_or_default();
    for (file, text) in gen_user_docs::pages() {
        assert!(text.starts_with("# "), "{file}: no H1");
        let second = text.lines().nth(2).unwrap_or_default();
        assert!(
            second.starts_with(&format!("> {}", gen_user_docs::GENERATED)),
            "{file}: {second:?}"
        );
        assert!(!text.contains('\r'), "{file}: a CR");
        assert!(!text.contains('\t'), "{file}: a tab");
        assert!(
            text.ends_with('\n') && !text.ends_with("\n\n"),
            "{file}: its end"
        );
        for (ix, line) in text.lines().enumerate() {
            assert_eq!(line.trim_end(), line, "{file}:{}: trailing space", ix + 1);
        }
        assert!(!text.contains(root.as_ref()), "{file}: the checkout's path");
        if home.len() > 1 {
            assert!(!text.contains(&home), "{file}: the home folder");
        }
    }
    // A second run gives the same text.
    assert_eq!(gen_user_docs::pages(), gen_user_docs::pages());
}

/// The shortcuts are the same whichever host resolves them: built from
/// the keymap as fe-keymap's build script resolved it for macOS and for
/// Linux (`DEFAULT_MACOS`, `DEFAULT_LINUX`), on this one host, the pages
/// are byte for byte the ones built from the one keymap, `DEFAULT`, which
/// the generator resolves for each platform by name.
#[test]
fn shortcuts_are_the_same_on_every_host() {
    let from_default = gen_user_docs::Keymaps::resolve(&[
        (Os::MacOs, fe_keymap::DEFAULT),
        (Os::Linux, fe_keymap::DEFAULT),
        (Os::Windows, fe_keymap::DEFAULT),
    ]);
    let resolved = gen_user_docs::Keymaps::resolve(&[
        (Os::MacOs, fe_keymap::DEFAULT_MACOS),
        (Os::Linux, fe_keymap::DEFAULT_LINUX),
        // fe-keymap generates no Windows text.
        (Os::Windows, fe_keymap::DEFAULT),
    ]);
    // Each platform's own text binds the same whichever OS parses it, so
    // the host that runs the generator doesn't matter.
    for keymap in [fe_keymap::DEFAULT_MACOS, fe_keymap::DEFAULT_LINUX] {
        let entries = |os| {
            let path = Path::new(DEFAULT_KEYMAP_PATH);
            let (layer, diagnostics) = KeymapLayer::parse_for(path, keymap, None, os);
            assert_eq!(diagnostics, []);
            layer.expect("the built-in keymap parses").entries
        };
        assert_eq!(entries(Os::MacOs), entries(Os::Linux));
        assert_eq!(entries(Os::MacOs), entries(Os::Windows));
    }
    assert!(from_default.bindings.len() > 300);
    let titles = gen_user_docs::commands();
    assert_eq!(
        gen_user_docs::shortcuts_page(&from_default, &titles),
        gen_user_docs::shortcuts_page(&resolved, &titles)
    );
    // macOS's keys differ from Linux's, so the page isn't one platform's
    // keys copied to every column.
    assert!(
        (from_default.bindings.values()).any(|labels| labels[0] != labels[1]),
        "macOS's keys are Linux's"
    );
}

/// `--check` on a checkout whose page is stale writes nothing, fails, and
/// names the page and the lines that differ; a plain run rewrites it, and
/// `--check` then passes.
#[test]
fn check_mode_reports_a_stale_page_and_writes_nothing() {
    let root = tempfile::tempdir().expect("a temporary folder");
    let dir = root.path().join(gen_user_docs::DIR);
    std::fs::create_dir_all(&dir).expect("the reference folder");
    assert_eq!(
        gen_user_docs::run(&[], root.path()),
        std::process::ExitCode::SUCCESS
    );
    let path = dir.join(gen_user_docs::SETTINGS);
    let fresh = std::fs::read_to_string(&path).expect("settings.md was written");
    let stale = fresh.replacen("### `editor.tab_size`", "### `editor.tab_width`", 1);
    assert_ne!(stale, fresh, "the page names editor.tab_size");
    std::fs::write(&path, &stale).expect("the stale page");

    let summaries = gen_user_docs::stale(root.path());
    assert_eq!(summaries.len(), 1, "{summaries:?}");
    let summary = &summaries[0];
    assert!(
        summary.contains("docs/user/src/reference/settings.md is stale"),
        "{summary}"
    );
    assert!(
        summary.contains("- ") && summary.contains("editor.tab_width"),
        "{summary}"
    );
    assert!(
        summary.contains("+ ") && summary.contains("editor.tab_size"),
        "{summary}"
    );
    assert_eq!(
        gen_user_docs::run(&["--check"], root.path()),
        std::process::ExitCode::FAILURE
    );
    let after = std::fs::read_to_string(&path).expect("settings.md");
    assert_eq!(after, stale, "--check wrote the page");

    // A missing page is stale too.
    std::fs::remove_file(dir.join(gen_user_docs::COMMANDS)).expect("commands.md");
    assert_eq!(gen_user_docs::stale(root.path()).len(), 2);

    assert_eq!(
        gen_user_docs::run(&[], root.path()),
        std::process::ExitCode::SUCCESS
    );
    assert_eq!(gen_user_docs::stale(root.path()), Vec::<String>::new());
    assert_eq!(
        gen_user_docs::run(&["--check"], root.path()),
        std::process::ExitCode::SUCCESS
    );
}

/// A key's default is what the tables holding it give it, when theirs
/// differs from its own type's; an `Option` with none is unset, a
/// `serde_json::Value` one too.
#[test]
fn defaults_come_from_the_holding_tables() {
    let schema = serde_json::json!({
        "$defs": {
            "Inner": {
                "properties": {
                    "size": { "default": 1, "type": "integer" },
                    "kept": { "default": "own", "type": "string" },
                },
                "type": "object",
            },
            "Outer": {
                "properties": {
                    "inner": {
                        "$ref": "#/$defs/Inner",
                        "default": { "kept": "own", "size": 7 },
                    },
                    "options": { "description": "Anything." },
                    "by_name": {
                        "additionalProperties": { "$ref": "#/$defs/Inner" },
                        "default": {},
                        "type": "object",
                    },
                    "listed": {
                        "default": [],
                        "items": { "$ref": "#/$defs/Inner" },
                        "type": "array",
                    },
                    "nulled": {
                        "default": null,
                        "items": { "$ref": "#/$defs/Inner" },
                        "type": ["array", "null"],
                    },
                    "absent": {
                        "items": { "$ref": "#/$defs/Inner" },
                        "type": "array",
                    },
                },
                "type": "object",
            },
        },
        "properties": {
            "outer": {
                "$ref": "#/$defs/Outer",
                "default": { "inner": { "kept": "own", "size": 9 } },
            },
        },
        "type": "object",
    });
    let page = gen_user_docs::settings_page(&schema);
    let section = |key: &str| -> String {
        let start = page
            .find(&format!("### `{key}`\n"))
            .unwrap_or_else(|| panic!("no {key} in\n{page}"));
        let rest = &page[start + 1..];
        rest[..rest.find("\n#").unwrap_or(rest.len())].to_owned()
    };
    assert!(
        section("outer.inner.size").contains("- Default: `9`"),
        "{page}"
    );
    assert!(
        section("outer.inner.kept").contains("- Default: `\"own\"`"),
        "{page}"
    );
    assert!(
        section("outer.options").contains("- Default: unset (the schema gives no default)\n"),
        "{page}"
    );
    // A map's or list's default, empty, null or none, each its own way.
    let table = |path: &str| -> String {
        let start = page
            .find(&format!("## `{path}`\n"))
            .unwrap_or_else(|| panic!("no {path} in\n{page}"));
        let rest = &page[start..];
        rest[..rest.find("\n### ").unwrap_or(rest.len())].to_owned()
    };
    assert!(
        table("outer.by_name.<name>").contains("\nDefault: `{}`\n"),
        "{page}"
    );
    assert!(
        table("outer.listed").contains("\nDefault: `[]`\n"),
        "{page}"
    );
    assert!(
        table("outer.nulled").contains("\nDefault: unset\n"),
        "{page}"
    );
    assert!(!table("outer.absent").contains("Default"), "{page}");

    // The real ones: a language server's `initialization_options`, and
    // the empty tables of servers and list of remaps.
    let page = page_text(gen_user_docs::SETTINGS);
    for (path, default) in [("lsp.servers.<name>", "`{}`"), ("vim.remap", "`[]`")] {
        let start = page
            .find(&format!("## `{path}`\n"))
            .unwrap_or_else(|| panic!("no {path}"));
        let rest = &page[start..];
        let section = &rest[..rest.find("\n### ").unwrap_or(rest.len())];
        assert!(
            section.contains(&format!("\nDefault: {default}\n")),
            "{section}"
        );
    }
    let start = page
        .find("### `lsp.servers.<name>.initialization_options`\n")
        .expect("initialization_options has a heading");
    let rest = &page[start + 1..];
    let section = &rest[..rest.find("\n#").unwrap_or(rest.len())];
    assert!(
        section.contains("- Default: unset (the schema gives no default)\n"),
        "{section}"
    );
}

/// The pages show the keymap as the app applies it: an unbind
/// (`command = ""`) removes the keys bound before it, and a later binding
/// of the same keys and context overrides an earlier one.
#[test]
fn unbinds_and_overrides_apply_as_in_the_app() {
    let keymap = r#"
[[bindings]]
keys = "secondary-k"
command = "editor.move_left"
context = "editor"

[[bindings]]
keys = "secondary-j"
command = "editor.move_left"
context = "editor"

[[bindings]]
keys = "secondary-k"
command = ""
context = "editor"

[[bindings]]
keys = "secondary-l"
command = "editor.move_right"
context = "editor"

[[bindings]]
keys = "secondary-l"
command = "editor.move_up"
context = "editor"
"#;
    let keymaps = gen_user_docs::Keymaps::resolve(&[
        (Os::MacOs, keymap),
        (Os::Linux, keymap),
        (Os::Windows, keymap),
    ]);
    let keys: Vec<(String, Vec<String>, Vec<String>)> = (keymaps.bindings.iter())
        .map(|(bound, labels)| (bound.command.clone(), labels[0].clone(), labels[1].clone()))
        .collect();
    let row = |command: &str, mac: &str, pc: &str| {
        (
            command.to_owned(),
            vec![mac.to_owned()],
            vec![pc.to_owned()],
        )
    };
    assert_eq!(
        keys,
        [
            row("editor.move_left", "⌘J", "Ctrl+J"),
            row("editor.move_up", "⌘L", "Ctrl+L"),
        ]
    );
    let page = gen_user_docs::shortcuts_page(&keymaps, &Default::default());
    assert!(!page.contains("editor.move_right"), "{page}");
    assert!(!page.contains("<kbd>⌘K</kbd>"), "{page}");
}

/// A map's or list's default reads differently for each way it can be
/// given: none, null, empty, or with entries, a null entry among them. So
/// a change from one to another changes the page, and the drift check sees
/// it.
#[test]
fn collection_defaults_read_differently() {
    let page = |default: Option<Value>, nullable: bool| {
        let mut map = serde_json::json!({
            "additionalProperties": { "$ref": "#/$defs/Inner" },
            "type": if nullable { serde_json::json!(["object", "null"]) } else { serde_json::json!("object") },
        });
        if let Some(default) = default {
            map["default"] = default;
        }
        let schema = serde_json::json!({
            "$defs": {
                "Inner": {
                    "properties": { "size": { "default": 1, "type": "integer" } },
                    "type": "object",
                },
            },
            "properties": { "by_name": map },
            "type": "object",
        });
        gen_user_docs::settings_page(&schema)
    };
    let pages = [
        page(None, false),
        page(None, true),
        page(Some(Value::Null), true),
        page(Some(serde_json::json!({})), true),
        page(Some(serde_json::json!({ "a": { "size": 2 } })), true),
        page(
            Some(serde_json::json!({ "a": { "size": 2 }, "b": null })),
            true,
        ),
        page(Some(serde_json::json!({ "a": { "size": null } })), true),
    ];
    for (ix, one) in pages.iter().enumerate() {
        for (other_ix, other) in pages.iter().enumerate().skip(ix + 1) {
            assert_ne!(
                one, other,
                "defaults {ix} and {other_ix} read the same:\n{one}"
            );
        }
    }
    assert!(pages[0].contains("A table of these tables") && !pages[0].contains("\nDefault:"));
    assert!(pages[1].contains("\nDefault: unset (the schema gives no default)\n"));
    assert!(pages[2].contains("\nDefault: unset\n"));
    assert!(pages[3].contains("\nDefault: `{}`\n"));
    assert!(
        pages[5].contains("\nDefault: `{ a = { size = 2 }, b = unset }`\n"),
        "{}",
        pages[5]
    );
}
