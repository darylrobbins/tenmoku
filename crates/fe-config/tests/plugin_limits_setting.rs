//! `[extensions.plugin_limits."<id>"]`: per-plugin limits, the user's own,
//! lowered to what the host allows with a diagnostic.

use std::fs;

use fe_config::{PROJECT_EXTENSIONS, SettingsStore, Severity};

fn load_user(text: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    fs::write(&user, text).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn plugin_limits_user_layer_applies() {
    let store = load_user(
        "[extensions.plugin_limits.\"acme.cargo\"]\n\
         background_memory = \"512MiB\"\nbackground_wall = \"30m\"\n\
         call_deadline_ms = 250\nmemory = \"128MiB\"\n",
    );
    assert_eq!(store.diagnostics(), []);
    let settings = store.settings();
    let limits = &settings.extensions.plugin_limits["acme.cargo"];
    assert_eq!(limits.background_memory_bytes(), Some(512 << 20));
    assert_eq!(limits.background_wall_secs(), Some(30 * 60));
    assert_eq!(limits.call_deadline_ms, Some(250));
    assert_eq!(limits.memory_bytes(), Some(128 << 20));
}

#[test]
fn plugin_data_quota_is_read_and_held_to_two_gib() {
    let store =
        load_user("[extensions.plugin_limits.\"acme.cargo\"]\nbackground_data = \"256MiB\"\n");
    assert_eq!(store.diagnostics(), []);
    let settings = store.settings();
    let limits = &settings.extensions.plugin_limits["acme.cargo"];
    assert_eq!(limits.background_data_bytes(), Some(256 << 20));

    let store =
        load_user("[extensions.plugin_limits.\"acme.cargo\"]\nbackground_data = \"3GiB\"\n");
    let settings = store.settings();
    let limits = &settings.extensions.plugin_limits["acme.cargo"];
    assert_eq!(limits.background_data_bytes(), Some(2 << 30));
    assert!(
        store
            .diagnostics()
            .iter()
            .any(|d| d.severity == Severity::Warning && d.message.contains("background_data")),
        "{:?}",
        store.diagnostics()
    );

    // Not a size: dropped with a warning.
    let store =
        load_user("[extensions.plugin_limits.\"acme.cargo\"]\nbackground_data = \"lots\"\n");
    let settings = store.settings();
    assert_eq!(
        settings.extensions.plugin_limits["acme.cargo"].background_data_bytes(),
        None
    );
    assert!(!store.diagnostics().is_empty());
}

#[test]
fn plugin_limits_project_layer_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project.toml");
    fs::write(
        &project,
        "[extensions.plugin_limits.\"acme.cargo\"]\nbackground_wall = \"30m\"\n",
    )
    .unwrap();
    let store = SettingsStore::load(None, Some(project));
    assert!(store.settings().extensions.plugin_limits.is_empty());
    assert!(
        store
            .diagnostics()
            .iter()
            .any(|d| d.severity == Severity::Warning && d.message == PROJECT_EXTENSIONS),
        "{:?}",
        store.diagnostics()
    );
}

#[test]
fn plugin_limits_clamped_with_diagnostic() {
    let store = load_user(
        "[extensions.plugin_limits.\"acme.cargo\"]\n\
         background_wall = \"90m\"\nbackground_memory = \"8GiB\"\n\
         call_deadline_ms = 5000\nmemory = \"1GiB\"\n",
    );
    let settings = store.settings();
    let limits = &settings.extensions.plugin_limits["acme.cargo"];
    assert_eq!(limits.background_wall_secs(), Some(3600));
    // Held to min(2 GiB, RAM / 4), whatever this computer's RAM.
    let ceiling = fe_config::settings::background_memory_ceiling(
        kiln_core::platform::physical_ram_bytes().unwrap_or(4 << 30),
    );
    assert_eq!(limits.background_memory_bytes(), Some(ceiling));
    assert_eq!(limits.call_deadline_ms, Some(1000));
    assert_eq!(limits.memory_bytes(), Some(256 << 20));
    let keys: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| (d.severity, d.key.clone().unwrap()))
        .collect();
    for key in [
        "background_wall",
        "background_memory",
        "call_deadline_ms",
        "memory",
    ] {
        let full = format!("extensions.plugin_limits.\"acme.cargo\".{key}");
        assert!(
            keys.contains(&(Severity::Warning, full.clone())),
            "{full}: {keys:?}"
        );
    }
}

#[test]
fn plugin_limits_unknown_plugin_id_is_kept_not_error() {
    let store = load_user("[extensions.plugin_limits.\"no.such.plugin\"]\nmemory = \"64MiB\"\n");
    assert_eq!(store.diagnostics(), []);
    let settings = store.settings();
    assert!(
        settings
            .extensions
            .plugin_limits
            .contains_key("no.such.plugin")
    );

    // A value that isn't one is dropped with a warning, not an error.
    let store = load_user("[extensions.plugin_limits.\"a.b\"]\nmemory = \"lots\"\n");
    assert_eq!(store.diagnostics().len(), 1);
    assert_eq!(store.diagnostics()[0].severity, Severity::Warning);
    assert_eq!(
        store.settings().extensions.plugin_limits["a.b"].memory,
        None
    );
}
