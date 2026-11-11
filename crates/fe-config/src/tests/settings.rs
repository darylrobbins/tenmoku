use std::{fs, path::PathBuf};

use crate::{Diagnostic, Settings, SettingsStore, settings_schema};

use super::write;

/// The store's only diagnostic, which must be an error.
fn only_error(store: &SettingsStore) -> &Diagnostic {
    let [diagnostic] = store.diagnostics() else {
        panic!("expected one diagnostic: {:#?}", store.diagnostics());
    };
    assert!(diagnostic.is_error(), "{diagnostic}");
    diagnostic
}

#[test]
fn errors_have_locations_and_keep_last_good() {
    let dir = tempfile::tempdir().unwrap();
    let good = "[editor]\nfont_size = 16\ntab_size = 2\n";
    // (text, line, column, key)
    let cases = [
        ("[editor]\nfont_size = 16\ntab_size = = 2\n", 3, 12, None),
        (
            "[editor]\ntab_size = 2\nfont_size = \"big\"\n",
            3,
            13,
            Some("editor.font_size"),
        ),
        (
            "[editor]\n  font_size = 16\n  tab_size = 0\n",
            3,
            14,
            Some("editor.tab_size"),
        ),
    ];
    for (text, line, column, key) in cases {
        let path = write(&dir, "settings.toml", good);
        let mut store = SettingsStore::load(Some(path.clone()), None);
        assert!(store.diagnostics().is_empty());
        fs::write(&path, text).unwrap();
        let event = store
            .reload(std::slice::from_ref(&path))
            .expect("new diagnostics");
        let diagnostic = only_error(&store);
        assert_eq!(event.diagnostics, store.diagnostics());
        assert_eq!(diagnostic.file, path);
        assert_eq!(
            (
                diagnostic.line,
                diagnostic.column,
                diagnostic.key.as_deref()
            ),
            (line, column, key),
            "{diagnostic}"
        );
        // The last good values stay in effect.
        assert_eq!(event.settings, event.previous);
        assert_eq!(store.settings().editor.font_size, 16.0);
        assert_eq!(store.settings().editor.tab_size, 2);
    }

    // At startup an erroring layer is empty: the defaults and a valid
    // project layer apply.
    let user = write(
        &dir,
        "user.toml",
        "[editor]\nfont_size = 20\ntab_size = 0\n",
    );
    let project = write(&dir, "project.toml", "editor.soft_wrap = true\n");
    let store = SettingsStore::load(Some(user.clone()), Some(project));
    assert_eq!(only_error(&store).file, user);
    let mut expected = Settings::default();
    expected.editor.soft_wrap = true;
    assert_eq!(**store.settings(), expected);
}

#[test]
fn values_of_the_wrong_shape_are_located_errors() {
    let dir = tempfile::tempdir().unwrap();
    // (text, line, column, key)
    let cases = [
        // Serde would take the array as the struct.
        ("editor = [\"Menlo\", 0.0, 0.0, 0, true]\n", 1, 10, "editor"),
        ("[editor]\nsoft_wrap = 1\n", 2, 13, "editor.soft_wrap"),
        // Too big for an i64.
        (
            "[editor]\ntab_size = 9223372036854775808\n",
            2,
            12,
            "editor.tab_size",
        ),
    ];
    for (text, line, column, key) in cases {
        // At startup, and on a reload that keeps the last good table.
        let path = write(&dir, "settings.toml", text);
        let store = SettingsStore::load(Some(path.clone()), None);
        let diagnostic = only_error(&store);
        assert_eq!(
            (
                diagnostic.line,
                diagnostic.column,
                diagnostic.key.as_deref()
            ),
            (line, column, Some(key)),
            "{diagnostic}"
        );
        assert_eq!(**store.settings(), Settings::default());

        fs::write(&path, "editor.font_size = 16\n").unwrap();
        let mut store = SettingsStore::load(Some(path.clone()), None);
        fs::write(&path, text).unwrap();
        store.reload(std::slice::from_ref(&path)).expect("an error");
        assert_eq!(only_error(&store).key.as_deref(), Some(key));
        assert_eq!(store.settings().editor.font_size, 16.0);
    }
}

#[test]
fn unknown_keys_warn() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(
        &dir,
        "settings.toml",
        "colour = 1\n[editor]\nfont_size = 14\ncolour = \"red\"\ntab_size = 3\n",
    );
    let store = SettingsStore::load(Some(path.clone()), None);
    let warnings: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| (d.is_error(), d.key.as_deref(), d.line, d.column))
        .collect();
    assert_eq!(
        warnings,
        [
            (false, Some("colour"), 1, 1),
            (false, Some("editor.colour"), 4, 1)
        ],
        "{:#?}",
        store.diagnostics()
    );
    let shown = store.diagnostics()[1].to_string();
    assert!(
        shown.ends_with(":4:1: warning: editor.colour: unknown key, ignored"),
        "{shown}"
    );
    assert_eq!(store.settings().editor.font_size, 14.0);
    assert_eq!(store.settings().editor.tab_size, 3);

    // Keys from the file can't send escape sequences to a terminal. The key
    // is written with a TOML escape.
    let path = write(&dir, "escape.toml", "\"\\u001b[2J\" = 1\n");
    let store = SettingsStore::load(Some(path), None);
    let shown = store.diagnostics()[0].to_string();
    assert!(
        shown.contains("\\u{1b}[2J") && !shown.contains('\u{1b}'),
        "{shown:?}"
    );
}

#[test]
fn reload_reports_only_changes() {
    let dir = tempfile::tempdir().unwrap();
    let user = write(&dir, "user.toml", "editor.font_size = 14\n");
    let project = write(&dir, "project.toml", "editor.tab_size = 2\n");
    let both = [user.clone(), project.clone()];
    let mut store = SettingsStore::load(Some(user.clone()), Some(project.clone()));
    assert_eq!(store.paths(), both);

    assert!(store.reload(&both).is_none(), "nothing changed");
    assert!(store.reload(&[]).is_none());

    // Only the changed layer is read: the project's edit isn't seen until
    // its file is reported.
    fs::write(&user, "editor.font_size = 15\n").unwrap();
    fs::write(&project, "editor.tab_size = 3\n").unwrap();
    let event = store.reload(std::slice::from_ref(&user)).expect("a change");
    assert_eq!(event.previous.editor.font_size, 14.0);
    assert_eq!(event.settings.editor.font_size, 15.0);
    assert_eq!(event.settings.editor.tab_size, 2);
    assert!(event.diagnostics.is_empty());
    assert_eq!(store.settings(), &event.settings);

    let event = store.reload(&[project]).expect("a change");
    assert_eq!(event.previous.editor.tab_size, 2);
    assert_eq!(event.settings.editor.tab_size, 3);

    // An unrelated path reads nothing.
    assert!(
        store
            .reload(&[PathBuf::from("/elsewhere/settings.toml")])
            .is_none()
    );
}

#[test]
fn schema_matches_golden() {
    // Read at run time: a shared target can hold a test binary built for
    // another checkout.
    let path =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("settings.schema.json");
    let schema = settings_schema();
    let pretty = serde_json::to_string_pretty(&schema).unwrap() + "\n";
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        fs::write(&path, &pretty).unwrap();
    }
    let golden = fs::read_to_string(&path).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&golden).unwrap(),
        schema,
        "settings.schema.json is out of date: rerun with UPDATE_GOLDEN=1 and review the diff"
    );
    assert_eq!(golden, pretty, "settings.schema.json is pretty-printed");
    // The Settings editor's text, which the schema's descriptions are the
    // en-US source of, regenerates with it.
    let ftl = crate::l10n::generate::settings_ftl(&schema)
        .unwrap_or_else(|errors| panic!("{}", errors.join("\n")));
    super::l10n::golden("l10n/en-US/settings.ftl", &ftl);

    // Each setting has its type, default and bounds.
    let editor = &schema["$defs"]["EditorSettings"]["properties"];
    for (key, ty, default, min, max) in [
        ("font_size", "number", 13.0, Some(6.0), Some(72.0)),
        ("line_height", "number", 1.54, Some(1.0), Some(3.0)),
        ("tab_size", "integer", 4.0, Some(1.0), Some(16.0)),
    ] {
        let property = &editor[key];
        assert_eq!(property["type"], ty, "{key}");
        assert_eq!(property["default"].as_f64(), Some(default), "{key}");
        assert_eq!(property["minimum"].as_f64(), min, "{key}");
        assert_eq!(property["maximum"].as_f64(), max, "{key}");
    }
    assert_eq!(editor["soft_wrap"]["type"], "boolean");
    assert_eq!(editor["soft_wrap"]["default"], false);
    assert_eq!(editor["font_family"]["minLength"], 1);
    assert!(editor["font_family"]["type"].to_string().contains("string"));
}

#[test]
fn reduce_motion_follows_the_system_unless_overridden() {
    use crate::settings::ReduceMotion;

    let dir = tempfile::tempdir().unwrap();
    let user = write(&dir, "user.toml", "");
    let mut store = SettingsStore::load(Some(user.clone()), None);
    // The default follows the system's value, either way.
    let setting = |store: &SettingsStore| store.settings().workbench.reduce_motion;
    assert_eq!(setting(&store), ReduceMotion::System);
    assert!(setting(&store).resolve(true));
    assert!(!setting(&store).resolve(false));

    // An override wins over the system's value, in both directions, and the
    // reload reports the change.
    fs::write(&user, "[workbench]\nreduce_motion = \"off\"\n").unwrap();
    let event = store.reload(std::slice::from_ref(&user)).expect("a change");
    assert_eq!(event.previous.workbench.reduce_motion, ReduceMotion::System);
    assert_eq!(setting(&store), ReduceMotion::Off);
    assert!(!setting(&store).resolve(true), "off beats a system on");

    fs::write(&user, "[workbench]\nreduce_motion = \"on\"\n").unwrap();
    let event = store.reload(std::slice::from_ref(&user)).expect("a change");
    assert_eq!(event.previous.workbench.reduce_motion, ReduceMotion::Off);
    assert!(setting(&store).resolve(false), "on beats a system off");

    // Back to `system`: the system's value applies again.
    fs::write(&user, "").unwrap();
    store.reload(std::slice::from_ref(&user)).expect("a change");
    assert!(setting(&store).resolve(true));
    assert!(!setting(&store).resolve(false));
}
