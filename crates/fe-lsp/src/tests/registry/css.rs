//! vscode-css-language-server, the built-in CSS, SCSS and Less server.

use std::fs;
use std::path::PathBuf;

use fe_buffer::Buffer;
use fe_config::settings::LspSettings;
use serde_json::{Value, json};

use super::{configuration, open, root, routed, server, strings, uri};
use crate::document::language_id;
use crate::registry::{self, Registry, server_for, server_root};
use crate::tests::ROOT;

const NAME: &str = "vscode-css-language-server";

#[test]
fn css_scss_and_less_get_the_css_server_by_default() {
    let specs = Registry::default().specs();
    for language in ["css", "scss", "less"] {
        let css = server_for(&specs, language).unwrap();
        assert_eq!(css.name, NAME);
        assert_eq!(css.command.to_str(), Some(NAME));
        assert_eq!(css.args, strings(&["--stdio"]));
        assert_eq!(css.languages, strings(&["css", "scss", "less"]));
        assert_eq!(css.root_markers, strings(&["package.json"]));
        assert_eq!(
            css.initialization_options,
            Some(json!({ "provideFormatter": true }))
        );
        assert_eq!(css.settings, json!({}));
    }
    assert_eq!(
        server_for(&specs, "go").map(|s| s.name.as_str()),
        Some("gopls")
    );
    assert_eq!(
        server_for(&specs, "rust").map(|s| s.name.as_str()),
        Some("rust-analyzer")
    );
}

#[test]
fn css_language_ids_pass_through() {
    for (file, id) in [("a.css", "css"), ("a.scss", "scss"), ("a.less", "less")] {
        let path = PathBuf::from(ROOT).join(file);
        assert_eq!(language_id(id, &path), id);
    }

    let (fake, mut manager, _events) = routed();
    manager.update_settings(&LspSettings::default());
    let mut buffer = Buffer::new("a { color: red; }\n");
    let name = format!("{ROOT}/a.scss");
    open(&mut manager, root(), &mut buffer, &name, "scss").expect("a server takes it");
    fake.wait_for_document(&uri(&name), "didOpen", |r| r.is_some());
    assert_eq!(fake.document(&uri(&name)).unwrap().language_id, "scss");
}

#[test]
fn settings_can_disable_or_override_the_css_server() {
    let mut registry = Registry::default();
    let mut settings = LspSettings::default();
    settings
        .servers
        .insert(NAME.into(), server(json!({ "enabled": false })));
    registry.set_settings(settings);
    let specs = registry.specs();
    assert!(server_for(&specs, "css").is_none());
    assert!(server_for(&specs, "scss").is_none());

    let mut settings = LspSettings::default();
    settings
        .servers
        .insert(NAME.into(), server(json!({ "command": "/opt/css-ls" })));
    registry.set_settings(settings);
    let specs = registry.specs();
    let css = server_for(&specs, "css").unwrap();
    assert_eq!(css.command.to_str(), Some("/opt/css-ls"));
    assert_eq!(css.args, strings(&["--stdio"]));
    assert_eq!(css.languages, strings(&["css", "scss", "less"]));
    assert_eq!(css.root_markers, strings(&["package.json"]));
    assert_eq!(
        css.initialization_options,
        Some(json!({ "provideFormatter": true }))
    );
}

#[test]
fn css_server_root_is_the_package_json_folder() {
    let dir = tempfile::tempdir().unwrap();
    let markers = registry::builtin_specs()
        .into_iter()
        .find(|s| s.name == NAME)
        .unwrap()
        .root_markers;
    let pkg = dir.path().join("pkg");
    let file = pkg.join("src/styles/a.css");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "").unwrap();

    let outside = dir.path().join("window");
    assert_eq!(
        server_root(&file, Some(&outside), &markers),
        pkg.join("src/styles")
    );
    fs::write(pkg.join("package.json"), "{}").unwrap();
    assert_eq!(server_root(&file, Some(&outside), &markers), pkg);
}

#[test]
fn css_settings_sections_come_from_user_settings() {
    let open_css = |settings: LspSettings| {
        let (fake, mut manager, _events) = routed();
        manager.update_settings(&settings);
        let mut buffer = Buffer::new("a {}\n");
        let name = format!("{ROOT}/a.css");
        open(&mut manager, root(), &mut buffer, &name, "css").unwrap();
        fake.wait_initialized();
        let answers = configuration(&fake, 1, &[Some("css"), Some("scss")]);
        (answers, fake, manager, buffer)
    };

    let (answers, ..) = open_css(LspSettings::default());
    assert_eq!(answers[0], Value::Null);
    assert_eq!(answers[1], Value::Null);

    let mut settings = LspSettings::default();
    settings.servers.insert(
        NAME.into(),
        server(json!({ "settings": { "css": { "validate": false } } })),
    );
    let (answers, ..) = open_css(settings);
    assert_eq!(answers[0], json!({ "validate": false }));
    assert_eq!(answers[1], Value::Null);
}
