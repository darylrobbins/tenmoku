//! `[remote]`: the four remote-development settings, user-only
//! (`docs/decisions/j8wvgszah0-remote-development.md` § 3 grace, § 4
//! dev_server, § 8 ports and auto_forward; `docs/designs/remote-development.md`
//! § 9). Nothing reads them yet: this is the table and its model rows.

use std::fs;

use fe_config::{
    Settings, SettingsStore, Severity,
    remote_settings::{AutoForward, PortForward},
    settings::PROJECT_REMOTE,
    settings_schema,
};

/// A store loaded from `contents` as the user's settings file.
fn user(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    fs::write(&path, contents).unwrap();
    SettingsStore::load(Some(path), None)
}

/// A store loaded from `contents` as a project's settings file.
fn project(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    fs::write(&path, contents).unwrap();
    SettingsStore::load(None, Some(path))
}

/// Each diagnostic as (severity, key, line), as the other settings tests
/// compare them.
fn found(store: &SettingsStore) -> Vec<(Severity, Option<&str>, usize)> {
    store
        .diagnostics()
        .iter()
        .map(|d| (d.severity, d.key.as_deref(), d.line))
        .collect()
}

#[test]
fn unset_empty_notify_and_ten_by_default() {
    let remote = Settings::default().remote;
    assert_eq!(remote.dev_server, None);
    assert_eq!(remote.ports, []);
    assert_eq!(remote.auto_forward, AutoForward::Notify);
    assert_eq!(remote.reconnect_grace, 10);
}

#[test]
fn dev_server_is_a_path() {
    let store = user("[remote]\ndev_server = \"/opt/tenmoku-server\"\n");
    assert_eq!(found(&store), []);
    assert_eq!(
        store.settings().remote.dev_server.as_deref(),
        Some(std::path::Path::new("/opt/tenmoku-server"))
    );
}

#[test]
fn ports_take_a_bare_port_or_host_local() {
    let store = user("[remote]\nports = [3000, \"5432:15432\"]\n");
    assert_eq!(found(&store), []);
    let ports = &store.settings().remote.ports;
    assert_eq!(
        ports,
        &[
            PortForward::Same(3000),
            PortForward::Mapped("5432:15432".to_owned())
        ]
    );
    // A bare port is the same port at both ends; a mapped one is host:local.
    assert_eq!(ports[0].resolve(), Some((3000, 3000)));
    assert_eq!(ports[1].resolve(), Some((5432, 15432)));
}

#[test]
fn the_highest_and_lowest_ports_are_taken() {
    let store = user("[remote]\nports = [1, 65535, \"1:65535\"]\n");
    assert_eq!(found(&store), []);
    let ports = &store.settings().remote.ports;
    assert_eq!(ports[0].resolve(), Some((1, 1)));
    assert_eq!(ports[1].resolve(), Some((65535, 65535)));
    assert_eq!(ports[2].resolve(), Some((1, 65535)));
}

#[test]
fn a_bad_port_is_refused_and_the_list_dropped() {
    // Not a port at all, out of range at either end, a missing half, a third
    // part, and zero, which no program listens on.
    for bad in [
        "[\"nope\"]",
        "[\"5432:\"]",
        "[\":15432\"]",
        "[\"5432:15432:1\"]",
        "[\"65536:1\"]",
        "[\"1:65536\"]",
        "[\"0:1\"]",
        "[0]",
        "[65536]",
        "[-1]",
        "[3000.5]",
    ] {
        let store = user(&format!("[remote]\nports = {bad}\n"));
        let diagnostics = found(&store);
        assert_eq!(diagnostics.len(), 1, "{bad}: {:#?}", store.diagnostics());
        assert_eq!(diagnostics[0].0, Severity::Warning, "{bad}");
        assert_eq!(diagnostics[0].1, Some("remote.ports"), "{bad}");
        assert_eq!(store.settings().remote.ports, [], "{bad}");
    }
}

#[test]
fn auto_forward_takes_notify_forward_or_off() {
    for (text, want) in [
        ("notify", AutoForward::Notify),
        ("forward", AutoForward::Forward),
        ("off", AutoForward::Off),
    ] {
        let store = user(&format!("[remote]\nauto_forward = \"{text}\"\n"));
        assert_eq!(found(&store), [], "{text}");
        assert_eq!(store.settings().remote.auto_forward, want, "{text}");
    }
}

#[test]
fn a_bad_auto_forward_is_refused_and_notify_kept() {
    let store = user("[remote]\nauto_forward = \"always\"\n");
    assert_eq!(
        found(&store),
        [(Severity::Error, Some("remote.auto_forward"), 2)],
        "{:#?}",
        store.diagnostics()
    );
    assert_eq!(
        store.settings().remote.auto_forward,
        AutoForward::Notify,
        "the default is kept"
    );
}

#[test]
fn reconnect_grace_takes_one_to_sixty_minutes() {
    for minutes in [1, 10, 59, 60] {
        let store = user(&format!("[remote]\nreconnect_grace = {minutes}\n"));
        assert_eq!(found(&store), [], "{minutes}");
        assert_eq!(
            store.settings().remote.reconnect_grace,
            minutes,
            "{minutes}"
        );
    }
}

#[test]
fn an_out_of_range_grace_is_refused_and_ten_kept() {
    for bad in ["0", "61", "-1", "2.5", "\"ten\""] {
        let store = user(&format!("[remote]\nreconnect_grace = {bad}\n"));
        let diagnostics = found(&store);
        assert_eq!(diagnostics.len(), 1, "{bad}: {:#?}", store.diagnostics());
        assert_eq!(diagnostics[0].0, Severity::Error, "{bad}");
        assert_eq!(
            diagnostics[0].1,
            Some("remote.reconnect_grace"),
            "{bad}: {:#?}",
            store.diagnostics()
        );
        assert_eq!(store.settings().remote.reconnect_grace, 10, "{bad}");
    }
}

#[test]
fn remote_settings_are_user_only() {
    // Design § 9: "All four are in user settings only; a project file can't
    // set them." § 4 says the same of dev_server by name.
    let store = project(
        "[remote]\n\
         dev_server = \"/tmp/not-mine\"\n\
         ports = [3000]\n\
         auto_forward = \"forward\"\n\
         reconnect_grace = 60\n",
    );
    let remote = &store.settings().remote;
    assert_eq!(
        remote.dev_server, None,
        "a project file can't name a server"
    );
    assert_eq!(remote.ports, []);
    assert_eq!(remote.auto_forward, AutoForward::Notify);
    assert_eq!(remote.reconnect_grace, 10);

    // One warning per key it set, each naming the key.
    let diagnostics = store.diagnostics();
    let mut keys: Vec<Option<&str>> = diagnostics.iter().map(|d| d.key.as_deref()).collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            Some("remote.auto_forward"),
            Some("remote.dev_server"),
            Some("remote.ports"),
            Some("remote.reconnect_grace"),
        ],
        "{diagnostics:#?}"
    );
    for d in diagnostics {
        assert_eq!(d.severity, Severity::Warning, "{d:?}");
        assert_eq!(d.message, PROJECT_REMOTE, "{d:?}");
    }

    // A user file's are applied, so the rejection is the project scope's.
    let store = user("[remote]\nreconnect_grace = 60\n");
    assert_eq!(found(&store), []);
    assert_eq!(store.settings().remote.reconnect_grace, 60);
}

#[test]
fn the_project_table_is_dropped_even_when_it_is_not_a_table() {
    let store = project("remote = 3\n");
    assert_eq!(store.settings().remote, Settings::default().remote);
    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert_eq!(diagnostics[0].key.as_deref(), Some("remote"));
}

#[test]
fn the_schema_lists_the_four_keys_with_descriptions() {
    let schema = settings_schema();
    let remote = &schema["$defs"]["RemoteSettings"]["properties"];
    for key in ["dev_server", "ports", "auto_forward", "reconnect_grace"] {
        let description = remote[key]["description"].as_str();
        assert!(
            description.is_some_and(|text| text.len() > 20),
            "{key}: {:?}",
            remote[key]
        );
    }
    assert_eq!(remote["reconnect_grace"]["default"], 10);
    assert_eq!(remote["reconnect_grace"]["minimum"], 1);
    assert_eq!(remote["reconnect_grace"]["maximum"], 60);
    assert_eq!(remote["auto_forward"]["default"], "notify");
    assert!(
        schema["properties"]["remote"]["description"]
            .as_str()
            .is_some_and(|text| text.contains("user")),
        "the table's own description says it is the user's: {:?}",
        schema["properties"]["remote"]["description"]
    );
}
