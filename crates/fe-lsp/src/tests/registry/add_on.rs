//! Add-on servers (docs/decisions/q5j03ax2af-lsp-add-on-servers.md §3): the
//! primary, the add-ons and the lead of a language.

use fe_config::settings::{LspServerSettings, LspSettings, TypescriptServer};
use serde_json::json;

use crate::ServerSpec;
use crate::registry::{self, Registry, extension_spec, server_for, servers_for};

fn spec(name: &str, language: &str, add_on: bool) -> ServerSpec {
    ServerSpec {
        languages: vec![language.into()],
        add_on,
        ..ServerSpec::new(name, name)
    }
}

fn names(specs: &[&ServerSpec]) -> Vec<String> {
    specs.iter().map(|s| s.name.clone()).collect()
}

fn registry_with(servers: serde_json::Value) -> Registry {
    let mut registry = Registry::default();
    registry.set_settings(serde_json::from_value(json!({ "servers": servers })).unwrap());
    registry
}

#[test]
fn add_on_never_chosen_as_lead() {
    let specs = [
        spec("extra", "rust", true),
        spec("main", "rust", false),
        spec("more", "rust", true),
    ];
    assert_eq!(server_for(&specs, "rust").unwrap().name, "main");
    let servers = servers_for(&specs, "rust");
    assert_eq!(servers.primary.unwrap().name, "main");
    assert_eq!(names(&servers.add_ons), ["extra", "more"]);
    assert_eq!(servers.lead().unwrap().name, "main");
    assert!(ServerSpec::new("x", "x").package.is_none());
}

#[test]
fn a_language_of_add_ons_leads_with_the_first() {
    let specs = [spec("a", "vue", true), spec("b", "vue", true)];
    let servers = servers_for(&specs, "vue");
    assert!(servers.primary.is_none());
    assert_eq!(servers.lead().unwrap().name, "a");
    assert_eq!(names(&servers.add_ons), ["a", "b"]);
    assert!(servers_for(&specs, "rust").lead().is_none());
}

#[test]
fn lead_is_first_non_add_on_and_enabled_false_drops_each() {
    let mut registry = Registry::default();
    let ext = LspServerSettings {
        command: Some("ext-add".into()),
        languages: Some(vec!["rust".into()]),
        add_on: Some(true),
        ..Default::default()
    };
    registry.set_extension_specs(vec![extension_spec("ext-add", &ext).unwrap()]);
    registry.register(spec("host-add", "rust", true));
    registry.register(spec("host-main", "rust", false));
    let specs = registry.specs();
    let servers = servers_for(&specs, "rust");
    assert_eq!(servers.lead().unwrap().name, "rust-analyzer");
    assert_eq!(names(&servers.add_ons), ["ext-add", "host-add"]);

    // enabled = false on the lead hands it to the next primary, not an
    // add-on; the add-ons stay.
    let mut registry = registry_with(json!({ "rust-analyzer": { "enabled": false } }));
    registry.set_extension_specs(vec![extension_spec("ext-add", &ext).unwrap()]);
    registry.register(spec("host-add", "rust", true));
    registry.register(spec("host-main", "rust", false));
    let specs = registry.specs();
    let servers = servers_for(&specs, "rust");
    assert_eq!(servers.lead().unwrap().name, "host-main");
    assert_eq!(names(&servers.add_ons), ["ext-add", "host-add"]);

    // On an add-on, just that add-on goes.
    let mut registry = registry_with(json!({ "ext-add": { "enabled": false } }));
    registry.set_extension_specs(vec![extension_spec("ext-add", &ext).unwrap()]);
    registry.register(spec("host-add", "rust", true));
    let specs = registry.specs();
    let servers = servers_for(&specs, "rust");
    assert_eq!(servers.lead().unwrap().name, "rust-analyzer");
    assert_eq!(names(&servers.add_ons), ["host-add"]);

    // With only add-ons left the first leads.
    let registry = registry_with(json!({
        "rust-analyzer": { "enabled": false },
        "mine": { "command": "m", "languages": ["rust"], "add_on": true },
    }));
    let specs = registry.specs();
    assert_eq!(server_for(&specs, "rust").unwrap().name, "mine");

    // typescript_server picks between the primaries; add-ons are untouched.
    for (choice, lead) in [
        (
            TypescriptServer::TypescriptLanguageServer,
            "typescript-language-server",
        ),
        (TypescriptServer::TypescriptGo, "typescript-go"),
    ] {
        let mut registry = Registry::default();
        registry.set_settings(LspSettings {
            typescript_server: choice,
            ..LspSettings::default()
        });
        registry.register(spec("ts-add", "tsx", true));
        let specs = registry.specs();
        let servers = servers_for(&specs, "tsx");
        assert_eq!(servers.lead().unwrap().name, lead);
        assert_eq!(names(&servers.add_ons), ["ts-add"]);
    }

    // The setting turns a built-in into an add-on.
    let mut registry = registry_with(json!({ "typescript-language-server": { "add_on": true } }));
    registry.set_settings(LspSettings {
        typescript_server: TypescriptServer::TypescriptLanguageServer,
        ..registry.settings().clone()
    });
    let specs = registry.specs();
    let servers = servers_for(&specs, "typescript");
    assert!(
        servers.primary.is_none(),
        "typescript-go is the unchosen one"
    );
    assert_eq!(servers.lead().unwrap().name, "typescript-language-server");
    assert_eq!(names(&servers.add_ons), ["typescript-language-server"]);
}

#[test]
fn needs_restart_when_add_on_set_changes() {
    let old = spec("s", "rust", false);
    let new = spec("s", "rust", true);
    assert!(registry::needs_restart(&old, &new));
    assert!(registry::needs_restart(&new, &old));
    assert!(!registry::needs_restart(&old, &old.clone()));
    // The package is metadata about the command, so a change restarts too.
    let packaged = ServerSpec {
        package: Some("@scope/pkg".into()),
        ..old.clone()
    };
    assert!(registry::needs_restart(&old, &packaged));
}

#[test]
fn package_survives_the_settings_over_a_spec() {
    let mut registry = registry_with(json!({ "s": { "command": "other", "add_on": true } }));
    registry.register(ServerSpec {
        package: Some("@scope/pkg".into()),
        languages: vec!["rust".into()],
        ..ServerSpec::new("s", "s")
    });
    let specs = registry.specs();
    let found = specs.iter().find(|s| s.name == "s").unwrap();
    assert_eq!(found.package.as_deref(), Some("@scope/pkg"));
}
