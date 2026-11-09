//! jdtls and kotlin-lsp, the built-in Java and Kotlin servers.

use std::fs;
use std::path::PathBuf;

use fe_buffer::Buffer;
use fe_config::settings::LspSettings;
use serde_json::json;

use super::{open, root, routed, server, strings, uri};
use crate::document::language_id;
use crate::registry::{self, Registry, server_for, server_root};
use crate::tests::ROOT;

const JDTLS: &str = "jdtls";
const KOTLIN: &str = "kotlin-lsp";
const JAVA_MARKERS: [&str; 6] = [
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    ".git",
];

#[test]
fn java_and_kotlin_get_their_servers_by_default() {
    let specs = Registry::default().specs();
    let java = server_for(&specs, "java").unwrap();
    assert_eq!(java.name, JDTLS);
    assert_eq!(java.command.to_str(), Some("jdtls"));
    assert!(java.args.is_empty());
    assert_eq!(java.languages, strings(&["java"]));
    assert_eq!(java.root_markers, strings(&JAVA_MARKERS));

    let kotlin = server_for(&specs, "kotlin").unwrap();
    assert_eq!(kotlin.name, KOTLIN);
    assert_eq!(kotlin.command.to_str(), Some("kotlin-lsp"));
    assert_eq!(kotlin.args, strings(&["--stdio"]));
    assert_eq!(kotlin.languages, strings(&["kotlin"]));
    assert_eq!(kotlin.root_markers, strings(&JAVA_MARKERS[1..]));
    assert!(specs.iter().all(|s| s.name != "kotlin-language-server"));
}

#[test]
fn java_and_kotlin_language_ids_pass_through() {
    for (file, id) in [("A.java", "java"), ("A.kt", "kotlin")] {
        let path = PathBuf::from(ROOT).join(file);
        assert_eq!(language_id(id, &path), id);

        let (fake, mut manager, _events) = routed();
        manager.update_settings(&LspSettings::default());
        let mut buffer = Buffer::new("class A {}\n");
        let name = format!("{ROOT}/{file}");
        open(&mut manager, root(), &mut buffer, &name, id).expect("a server takes it");
        fake.wait_for_document(&uri(&name), "didOpen", |r| r.is_some());
        assert_eq!(fake.document(&uri(&name)).unwrap().language_id, id);
    }
}

#[test]
fn settings_can_disable_or_override_the_java_servers() {
    let mut registry = Registry::default();
    let mut settings = LspSettings::default();
    for name in [JDTLS, KOTLIN] {
        settings
            .servers
            .insert(name.into(), server(json!({ "enabled": false })));
    }
    registry.set_settings(settings);
    let specs = registry.specs();
    assert!(server_for(&specs, "java").is_none());
    assert!(server_for(&specs, "kotlin").is_none());

    let mut settings = LspSettings::default();
    settings
        .servers
        .insert(JDTLS.into(), server(json!({ "command": "/opt/jdtls" })));
    settings.servers.insert(
        KOTLIN.into(),
        server(json!({ "command": "/opt/kotlin-lsp" })),
    );
    registry.set_settings(settings);
    let specs = registry.specs();
    let java = server_for(&specs, "java").unwrap();
    assert_eq!(java.command.to_str(), Some("/opt/jdtls"));
    assert!(java.args.is_empty());
    assert_eq!(java.languages, strings(&["java"]));
    assert_eq!(java.root_markers, strings(&JAVA_MARKERS));
    let kotlin = server_for(&specs, "kotlin").unwrap();
    assert_eq!(kotlin.command.to_str(), Some("/opt/kotlin-lsp"));
    assert_eq!(kotlin.args, strings(&["--stdio"]));
    assert_eq!(kotlin.languages, strings(&["kotlin"]));
    assert_eq!(kotlin.root_markers, strings(&JAVA_MARKERS[1..]));
}

#[test]
fn root_is_the_marker_folder_the_decision_file_names() {
    let dir = tempfile::tempdir().unwrap();
    let markers = registry::builtin_specs()
        .into_iter()
        .find(|s| s.name == JDTLS)
        .unwrap()
        .root_markers;
    let outside = dir.path().join("window");

    // Gradle: the nearest build file roots a module; with none in the module,
    // the folder holding settings.gradle does.
    let gradle = dir.path().join("gradle");
    let app = gradle.join("app");
    let file = app.join("src/main/java/A.java");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "").unwrap();
    fs::write(gradle.join("settings.gradle"), "").unwrap();
    assert_eq!(server_root(&file, Some(&outside), &markers), gradle);
    fs::write(app.join("build.gradle.kts"), "").unwrap();
    assert_eq!(server_root(&file, Some(&outside), &markers), app);

    // Maven: the module's own pom.xml, not the parent's.
    let maven = dir.path().join("maven");
    let module = maven.join("core");
    let file = module.join("src/main/java/B.java");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "").unwrap();
    fs::write(maven.join("pom.xml"), "").unwrap();
    assert_eq!(server_root(&file, Some(&outside), &markers), maven);
    fs::write(module.join("pom.xml"), "").unwrap();
    assert_eq!(server_root(&file, Some(&outside), &markers), module);
}
