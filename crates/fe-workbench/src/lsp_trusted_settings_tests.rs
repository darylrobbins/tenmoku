//! A trusted root's language servers get the project file's `[lsp]` table
//! unfiltered (`fe_config::settings::LspLayer`, over the user file's
//! `lsp` table); an untrusted one's manager keeps the filtered settings the
//! workbench was given.

use std::{fs, path::PathBuf, sync::Arc};

use fe_config::{Settings, SettingsEvent, SettingsStore, settings::LspLayer};
use fe_lsp::fake::{FakeServer, Record};
use gpui::TestAppContext;
use lsp_types::{ConfigurationItem, ConfigurationParams, request::WorkspaceConfiguration};
use serde_json::{Value, json};

use crate::{lsp::Layers, tests::lsp::Lsp};

const USER: &str = r#"
[lsp.servers.rust-analyzer]
command = "ra-user"

[lsp.servers.rust-analyzer.settings.rust-analyzer]
cargo = { features = "user" }
"#;

/// The project's settings, its features set to `features`.
fn project(features: &str) -> String {
    format!(
        "[lsp.servers.rust-analyzer]\ncommand = \"ra-project\"\n\n\
         [lsp.servers.rust-analyzer.settings.rust-analyzer]\ncargo = {{ features = {features} }}\n"
    )
}

/// Writes `root`'s `.tenmoku/settings.toml`.
fn write_project(root: &std::path::Path, text: &str) {
    let path = fe_config::project_settings_path(root);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn user_path(s: &Lsp) -> PathBuf {
    s.config.path().join(fe_config::paths::SETTINGS_FILE)
}

/// The settings the host's store has for `s`'s files: the project's `lsp`
/// keys narrowed.
fn filtered(s: &Lsp) -> Settings {
    let project = fe_config::project_settings_path(&s.root);
    let store = SettingsStore::load(Some(user_path(s)), Some(project));
    Settings::clone(store.settings())
}

/// Reloads `s`'s settings as the host would: with the filtered ones.
fn reload(s: &mut Lsp) {
    reload_no_draw(s);
    s.draw();
}

fn reload_no_draw(s: &mut Lsp) {
    let settings = filtered(s);
    give(s, settings);
}

/// Gives `s` `settings`, as the host does on a reload.
fn give(s: &mut Lsp, settings: Settings) {
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    let workbench = s.workbench.clone();
    workbench.update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
}

/// Tells `s` its project settings file changed, as its watcher would.
fn project_changed(s: &mut Lsp) {
    s.workbench.read_with(&s.cx, |w, _| w.lsp_project_changed());
}

/// Tells `s`'s app its user settings file changed, as its watcher would,
/// and lets the read land.
fn user_changed(s: &mut Lsp) {
    let workbench = s.workbench.clone();
    workbench.update(&mut s.cx, |w, cx| w.lsp_user_changed(cx));
    s.draw();
}

/// The settings `launch` was last sent in `workspace/didChangeConfiguration`.
fn changed_configuration(fake: &FakeServer, launch: usize) -> Option<Value> {
    fake.records_of(launch).iter().rev().find_map(|r| match r {
        Record::Notification { method, params } if method == "workspace/didChangeConfiguration" => {
            Some(params["settings"].clone())
        }
        _ => None,
    })
}

fn handed(s: &mut Lsp) -> fe_config::settings::LspSettings {
    s.workbench
        .read_with(&s.cx, |w, _| w.lsp_handed())
        .expect("a manager")
}

/// rust-analyzer's command in the settings last handed to `s`'s manager.
fn handed_command(s: &mut Lsp) -> Option<String> {
    handed(s).servers["rust-analyzer"].command.clone()
}

/// The command launch `launch` (1-based) of `fake` ran.
fn command(fake: &FakeServer, launch: usize) -> PathBuf {
    fake.launched()[launch - 1].0.command.clone()
}

/// What launch `launch` gets for `workspace/configuration` of `section`.
fn configuration(fake: &FakeServer, launch: usize, section: &str) -> Value {
    let items = vec![ConfigurationItem {
        scope_uri: None,
        section: Some(section.to_owned()),
    }];
    let answers = fake
        .request_at::<WorkspaceConfiguration>(launch, ConfigurationParams { items })
        .unwrap();
    answers[0].clone()
}

#[gpui::test]
fn trusted_root_launches_with_project_settings(cx: &mut TestAppContext) {
    let mut s = Lsp::untrusted(cx);
    s.write_user_settings(USER);
    write_project(&s.root, &project(r#"["project"]"#));
    reload(&mut s);

    // Untrusted: the manager has the filtered settings, and nothing starts.
    assert_eq!(handed(&mut s), filtered(&s).lsp);
    assert_eq!(handed_command(&mut s).as_deref(), Some("ra-user"));
    s.open("src/main.rs");
    assert_eq!(s.fake.launches(), 0);

    // Trusted: the project's command, and its settings answer
    // workspace/configuration.
    s.set_trust(true);
    s.fake.wait_initialized();
    assert_eq!(s.fake.launches(), 1);
    assert_eq!(command(&s.fake, 1), PathBuf::from("ra-project"));
    assert_eq!(
        configuration(&s.fake, 1, "rust-analyzer.cargo.features"),
        json!(["project"])
    );
    assert_eq!(handed_command(&mut s).as_deref(), Some("ra-project"));

    // A root that's trusted from the start launches with them too.
    let mut t = s.another_rooted(&s.root.clone());
    assert!(t.workbench.read_with(&t.cx, |w, _| w.root_trusted()));
    t.open("src/lib.rs");
    t.fake.wait_initialized_at(2);
    assert_eq!(command(&t.fake, 2), PathBuf::from("ra-project"));
    assert_eq!(
        configuration(&t.fake, 2, "rust-analyzer.cargo.features"),
        json!(["project"])
    );
}

#[gpui::test]
fn trust_changes_and_reloads(cx: &mut TestAppContext) {
    let mut s = Lsp::untrusted(cx);
    s.write_user_settings(USER);
    write_project(&s.root, &project(r#"["project"]"#));
    reload(&mut s);
    s.open("src/main.rs");
    s.set_trust(true);
    s.fake.wait_initialized();
    assert_eq!(command(&s.fake, 1), PathBuf::from("ra-project"));

    // Untrusted: the server stops, and the manager is back on the filtered
    // settings.
    s.set_trust(false);
    s.fake.wait_for_at(1, "shutdown, exit", |records| {
        records.iter().any(|r| r.is_notification("exit"))
    });
    assert_eq!(handed(&mut s), filtered(&s).lsp);

    // Trusted again: it starts again with the project's command.
    s.set_trust(true);
    s.fake.wait_initialized_at(2);
    assert_eq!(command(&s.fake, 2), PathBuf::from("ra-project"));

    // The project file's settings edited, then a reload: the server is
    // told the new ones, and isn't launched again.
    write_project(&s.root, &project(r#"["edited"]"#));
    reload(&mut s);
    let records = s.fake.wait_for_at(2, "didChangeConfiguration", |records| {
        records
            .iter()
            .any(|r| r.is_notification("workspace/didChangeConfiguration"))
    });
    let sent = records.iter().find_map(|r| match r {
        Record::Notification { method, params } if method == "workspace/didChangeConfiguration" => {
            Some(params["settings"].clone())
        }
        _ => None,
    });
    assert_eq!(
        sent,
        Some(json!({ "rust-analyzer": { "cargo": { "features": ["edited"] } } }))
    );
    assert_eq!(
        configuration(&s.fake, 2, "rust-analyzer.cargo.features"),
        json!(["edited"])
    );
    assert_eq!(s.fake.launches(), 2);

    // A read of the old root's settings that lands after the root changed
    // is dropped. The new root, trusted as a subfolder, has no project
    // file, so its server gets the user's command.
    reload_no_draw(&mut s);
    let (read, generation) = s
        .workbench
        .read_with(&s.cx, |w, _| (w.lsp.trusted_read, w.lsp.generation));
    let mut stale = LspLayer::project(fe_config::project_settings_path(&s.root));
    stale.read();
    let stale = Layers {
        user: None,
        project: stale,
    };
    let src = s.path("src");
    let workbench = s.workbench.clone();
    s.cx.update(|window, cx| workbench.update(cx, |w, cx| w.set_root(src.clone(), window, cx)));
    s.draw();
    assert!(s.workbench.read_with(&s.cx, |w, _| w.root_trusted()));
    s.open("src/main.rs");
    s.fake.wait_initialized_at(3);
    assert_eq!(command(&s.fake, 3), PathBuf::from("ra-user"));
    // Now the old read lands, after the new root's own.
    workbench.update(&mut s.cx, |w, cx| {
        w.apply_trusted_settings(read, generation, &stale, cx)
    });
    s.draw();
    assert_eq!(handed_command(&mut s).as_deref(), Some("ra-user"));
    assert_eq!(s.fake.launches(), 3);

    // A workbench opened afterwards on an untrusted root starts with the
    // filtered settings the host gave, not this root's trusted ones, nor
    // its own project's.
    let other = tempfile::tempdir().unwrap();
    let other_root = dunce::canonicalize(other.path()).unwrap();
    write_project(
        &other_root,
        "[lsp.servers.rust-analyzer]\ncommand = \"ra-other\"\n",
    );
    let mut t = s.another_rooted(&other_root);
    assert!(!t.workbench.read_with(&t.cx, |w, _| w.root_trusted()));
    assert_eq!(handed(&mut t), filtered(&s).lsp);
    assert_eq!(handed_command(&mut t).as_deref(), Some("ra-user"));
    assert_eq!(s.fake.launches(), 3);
}

#[gpui::test]
fn project_file_edits_reach_servers_without_a_reload(cx: &mut TestAppContext) {
    // The app's store narrows the project file, so an edit to the keys it
    // drops changes nothing it has and reloads nothing: the workbench
    // watches the file itself (here, told of each change as the watcher
    // would tell it).
    let mut s = Lsp::untrusted(cx);
    write_project(&s.root, &project(r#"["project"]"#));
    reload(&mut s);
    s.open("src/main.rs");
    s.set_trust(true);
    s.fake.wait_initialized();
    assert_eq!(command(&s.fake, 1), PathBuf::from("ra-project"));

    // Its settings: didChangeConfiguration, no relaunch.
    write_project(&s.root, &project(r#"["edited"]"#));
    project_changed(&mut s);
    let edited = json!({ "rust-analyzer": { "cargo": { "features": ["edited"] } } });
    s.until("didChangeConfiguration", |s| {
        changed_configuration(&s.fake, 1) == Some(edited.clone())
    });
    assert_eq!(s.fake.launches(), 1);

    // Its command: the server is launched again with it.
    write_project(
        &s.root,
        &project(r#"["edited"]"#).replace("ra-project", "ra-next"),
    );
    project_changed(&mut s);
    s.until("a relaunch", |s| s.fake.launches() == 2);
    s.fake.wait_initialized_at(2);
    assert_eq!(command(&s.fake, 2), PathBuf::from("ra-next"));

    // Untrusted, the file's edits no longer apply.
    s.set_trust(false);
    write_project(&s.root, &project(r#"["untrusted"]"#));
    project_changed(&mut s);
    s.draw();
    assert_eq!(handed(&mut s), filtered(&s).lsp);
}

#[gpui::test]
fn invalid_files_keep_the_last_good_settings(cx: &mut TestAppContext) {
    let mut s = Lsp::untrusted(cx);
    write_project(&s.root, &project(r#"["project"]"#));
    reload(&mut s);
    s.open("src/main.rs");
    s.set_trust(true);
    s.fake.wait_initialized();
    assert_eq!(command(&s.fake, 1), PathBuf::from("ra-project"));

    // A broken project file keeps its last good table, through a reload
    // and through untrusting and trusting again.
    write_project(&s.root, "[lsp.servers.rust-analyzer\ncommand = ");
    reload(&mut s);
    assert_eq!(handed_command(&mut s).as_deref(), Some("ra-project"));
    s.set_trust(false);
    s.set_trust(true);
    s.fake.wait_initialized_at(2);
    assert_eq!(command(&s.fake, 2), PathBuf::from("ra-project"));
    assert_eq!(
        configuration(&s.fake, 2, "rust-analyzer.cargo.features"),
        json!(["project"])
    );
    // So does one the schema rejects.
    write_project(&s.root, "[lsp.servers.rust-analyzer]\ncommand = \"\"\n");
    reload(&mut s);
    assert_eq!(handed_command(&mut s).as_deref(), Some("ra-project"));

    // The user's file is a layer of its own, kept for every window: turning
    // servers off there holds when the file breaks, whatever the host
    // delivers, and the project's file can't turn them back on.
    s.write_user_settings("lsp.enabled = false\n");
    reload(&mut s);
    s.fake.wait_for_at(2, "shutdown, exit", |records| {
        records.iter().any(|r| r.is_notification("exit"))
    });
    s.write_user_settings("lsp.enabled = \n");
    // A window the host settles on another root reads its own store, which
    // has no last good table for the user's file: the defaults.
    let fresh = Settings::clone(
        SettingsStore::load(Some(user_path(&s)), Some(project_path(&s))).settings(),
    );
    assert!(fresh.lsp.enabled);
    give(&mut s, fresh);
    s.draw();
    s.set_trust(false);
    s.set_trust(true);
    assert!(!handed(&mut s).enabled);
    assert_eq!(handed_command(&mut s).as_deref(), Some("ra-project"));
    s.open("src/lib.rs");
    assert_eq!(s.fake.launches(), 2);
}

fn project_path(s: &Lsp) -> PathBuf {
    fe_config::project_settings_path(&s.root)
}

#[gpui::test]
fn removed_disabling_flags_stop_applying(cx: &mut TestAppContext) {
    // The host's store keeps a project file's `enabled = false` keys. The
    // trusted settings don't come from it: once the file stops setting
    // them, they're gone, with no host reload to say so.
    let mut s = Lsp::untrusted(cx);
    write_project(
        &s.root,
        "lsp.enabled = false\n[lsp.servers.rust-analyzer]\nenabled = false\n",
    );
    reload(&mut s);
    s.open("src/main.rs");
    s.set_trust(true);
    s.draw();
    assert!(!handed(&mut s).enabled);

    // Removed, the host's settings still have the flags until it reloads.
    write_project(&s.root, &project(r#"["project"]"#));
    project_changed(&mut s);
    s.until("the servers enabled", |s| {
        s.workbench
            .read_with(&s.cx, |w, _| w.lsp_handed())
            .is_some_and(|lsp| lsp.enabled)
    });
    s.draw();
    s.fake.wait_initialized();
    assert_eq!(handed_command(&mut s).as_deref(), Some("ra-project"));

    // An invalid command with the flags changed beside it keeps the last
    // good file, not the flags.
    write_project(
        &s.root,
        "lsp.enabled = false\n[lsp.servers.rust-analyzer]\ncommand = \"\"\n",
    );
    project_changed(&mut s);
    s.draw();
    assert!(handed(&mut s).enabled);
    assert_eq!(handed_command(&mut s).as_deref(), Some("ra-project"));
}

#[gpui::test]
fn documents_opened_before_trust_get_the_trusted_size_limit(cx: &mut TestAppContext) {
    // 14 bytes: src/main.rs fits, with an edit, and src/other.rs doesn't.
    let mut s = Lsp::untrusted(cx);
    fs::write(s.path("src/other.rs"), "pub fn g() {}\n// longer\n").unwrap();
    write_project(&s.root, "lsp.max_file_bytes = 14\n");
    reload(&mut s);
    // Opened under the filtered limit, while the root isn't trusted.
    s.open("src/other.rs");
    s.open("src/main.rs");
    s.set_trust(true);
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "main.rs", |r| r.is_some());
    // An edit to main.rs reaches the server after any didOpen of the
    // other: none came.
    s.cx.simulate_input("x");
    s.draw();
    s.fake.wait_for_document(&main, "the edit", |replica| {
        replica.is_some_and(|replica| replica.text.starts_with('x'))
    });
    assert_eq!(s.sent(1, "textDocument/didOpen", &s.uri("src/other.rs")), 0);
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 1);
}

#[gpui::test]
fn the_user_layer_follows_the_file_while_untrusted(cx: &mut TestAppContext) {
    // A valid disabling setting read while untrusted holds when the file
    // breaks before the root is first trusted.
    let mut s = Lsp::untrusted(cx);
    s.write_user_settings("lsp.enabled = false\n");
    user_changed(&mut s);
    reload(&mut s);
    s.write_user_settings("lsp.enabled = \n");
    user_changed(&mut s);
    let fresh = Settings::clone(
        SettingsStore::load(Some(user_path(&s)), Some(project_path(&s))).settings(),
    );
    give(&mut s, fresh);
    s.draw();
    s.set_trust(true);
    assert!(!handed(&mut s).enabled);
}

#[gpui::test]
fn a_disabling_edit_while_untrusted_reaches_the_user_layer(cx: &mut TestAppContext) {
    // Delivered to a secondary window, it replaces an enabling value read
    // earlier.
    let mut s = Lsp::untrusted(cx);
    let mut other = s.another_rooted(&s.root.clone());
    s.write_user_settings("lsp.enabled = true\n");
    user_changed(&mut s);
    reload(&mut other);
    s.write_user_settings("lsp.enabled = false\n");
    user_changed(&mut s);
    reload(&mut other);
    s.write_user_settings("lsp.enabled = \n");
    user_changed(&mut s);
    let fresh = Settings::clone(
        SettingsStore::load(Some(user_path(&s)), Some(project_path(&s))).settings(),
    );
    give(&mut other, fresh);
    other.draw();
    other.set_trust(true);
    assert!(!handed(&mut other).enabled);
}

#[gpui::test]
fn starting_a_manager_does_not_wait_on_a_user_file_read(cx: &mut TestAppContext) {
    let mut s = Lsp::untrusted(cx);
    let layer = s
        .workbench
        .read_with(&s.cx, |w, _| w.lsp_user_layer())
        .expect("a user layer");
    // A read holds the layer: the UI thread's path check must not wait.
    let guard = layer.lock().unwrap();
    let root = s.root.clone();
    let workbench = s.workbench.clone();
    workbench.update(&mut s.cx, |w, cx| w.lsp_start_manager(root, cx));
    drop(guard);
    s.draw();
}

#[gpui::test]
fn a_rootless_window_keeps_the_user_layer_current(cx: &mut TestAppContext) {
    // The app's only window is rootless: it initializes and maintains the
    // user layer itself.
    let mut s = Lsp::rootless(cx);
    assert!(
        s.workbench
            .read_with(&s.cx, |w, _| w.lsp_user_layer())
            .is_some()
    );
    // A valid disabling value read by the rootless window holds through
    // the file breaking, before a rooted window opens and its root is
    // trusted.
    s.write_user_settings("lsp.enabled = false\n");
    user_changed(&mut s);
    s.write_user_settings("lsp.enabled = \n");
    user_changed(&mut s);
    let mut rooted = s.another_rooted(&s.root.clone());
    rooted.set_trust(true);
    assert!(!handed(&mut rooted).enabled);
}

#[gpui::test]
fn a_disabling_edit_masked_by_the_project_reaches_the_user_layer(cx: &mut TestAppContext) {
    let mut s = Lsp::untrusted(cx);
    s.write_user_settings("lsp.enabled = true\n");
    write_project(&s.root, "lsp.enabled = false\n");
    reload(&mut s);
    // The edit changes no effective setting: no reload is delivered.
    s.write_user_settings("lsp.enabled = false\n");
    user_changed(&mut s);
    s.write_user_settings("lsp.enabled = \n");
    user_changed(&mut s);
    write_project(&s.root, "");
    reload(&mut s);
    s.set_trust(true);
    assert!(!handed(&mut s).enabled);
}
