// SPDX-License-Identifier: Apache-2.0

//! A typed [`SettingsStore`] over an app's own settings struct.

use std::{fs, path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::{Layer, ProjectFilter, SettingsOptions, SettingsStore, Severity};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
struct Notes {
    title: String,
    view: View,
}

impl Default for Notes {
    fn default() -> Self {
        Self {
            title: "Notes".into(),
            // Not `View::default()`: the app's defaults for the table.
            view: View {
                font_size: 20.0,
                wrap: true,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
struct View {
    #[schemars(range(min = 6.0, max = 72.0))]
    font_size: f64,
    wrap: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            font_size: 13.0,
            wrap: true,
        }
    }
}

struct Files {
    _dir: tempfile::TempDir,
    user: PathBuf,
    project: PathBuf,
}

fn files(user: &str, project: &str) -> Files {
    let dir = tempfile::tempdir().unwrap();
    let user_path = dir.path().join("settings.toml");
    let project_path = dir.path().join("project.toml");
    fs::write(&user_path, user).unwrap();
    fs::write(&project_path, project).unwrap();
    Files {
        _dir: dir,
        user: user_path,
        project: project_path,
    }
}

fn store(files: &Files) -> SettingsStore<Notes> {
    SettingsStore::load(Some(files.user.clone()), Some(files.project.clone()))
}

#[test]
fn layers_merge_key_by_key_over_the_defaults() {
    let files = files(
        "title = \"Mine\"\n[view]\nfont_size = 15\n",
        "[view]\nwrap = false\n",
    );
    let store = store(&files);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(
        **store.settings(),
        Notes {
            title: "Mine".into(),
            view: View {
                font_size: 15.0,
                wrap: false,
            },
        }
    );
    assert_eq!(store.paths(), [files.user.clone(), files.project.clone()]);
    assert_eq!(store.user_path(), Some(files.user.as_path()));
}

#[test]
fn a_nested_table_keeps_the_parents_defaults_for_keys_it_leaves_out() {
    // serde alone would fill `font_size` from `View::default()`.
    let files = files("[view]\nwrap = false\n", "");
    let store = store(&files);
    assert_eq!(store.settings().view.font_size, 20.0);
    assert!(!store.settings().view.wrap);
}

#[test]
fn reload_applies_valid_edits_and_keeps_the_last_good_on_errors() {
    let files = files("title = \"One\"\n", "");
    let mut store = store(&files);

    fs::write(&files.user, "title = \"Two\"\n").unwrap();
    let event = store.reload(std::slice::from_ref(&files.user)).unwrap();
    assert_eq!(event.previous.title, "One");
    assert_eq!(event.settings.title, "Two");
    assert_eq!(event.diagnostics, []);

    // A syntax error keeps "Two", with an error at the token.
    fs::write(&files.user, "title = \n").unwrap();
    let event = store.reload(std::slice::from_ref(&files.user)).unwrap();
    assert_eq!(event.settings.title, "Two");
    assert_eq!(event.settings, event.previous);
    let [error] = &event.diagnostics[..] else {
        panic!("one diagnostic: {:?}", event.diagnostics);
    };
    assert_eq!(error.severity, Severity::Error);
    assert_eq!(error.file, files.user);
    assert_eq!(error.line, 1);

    // A value of the wrong type is an error at its key, and keeps it too.
    fs::write(&files.user, "title = \"Three\"\n[view]\nwrap = 1\n").unwrap();
    let event = store.reload(std::slice::from_ref(&files.user)).unwrap();
    assert_eq!(event.settings.title, "Two");
    assert_eq!(event.diagnostics[0].key.as_deref(), Some("view.wrap"));
    assert_eq!(event.diagnostics[0].line, 3);

    // The same file again changes nothing, and says so.
    assert!(store.reload(std::slice::from_ref(&files.user)).is_none());

    fs::write(&files.user, "title = \"Four\"\n").unwrap();
    let event = store.reload(std::slice::from_ref(&files.user)).unwrap();
    assert_eq!(event.settings.title, "Four");
    assert_eq!(event.diagnostics, []);
}

#[test]
fn files_not_in_the_batch_are_not_read() {
    let files = files("title = \"One\"\n", "");
    let mut store = store(&files);
    fs::write(&files.user, "title = \"Two\"\n").unwrap();
    assert!(
        store
            .reload(&[files.user.with_file_name("other.toml")])
            .is_none()
    );
    assert_eq!(store.settings().title, "One");
}

#[test]
fn missing_files_are_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let store: SettingsStore<Notes> =
        SettingsStore::load(Some(dir.path().join("settings.toml")), None);
    assert_eq!(**store.settings(), Notes::default());
    assert_eq!(store.diagnostics(), []);
}

#[test]
fn a_schema_warns_of_unknown_keys_and_rejects_out_of_range_values() {
    let schema = schemars::schema_for!(Notes).to_value();
    let files = files("titel = \"x\"\n[view]\nfont_size = 15\n", "");
    let mut store: SettingsStore<Notes> =
        SettingsStore::with_schema(Some(files.user.clone()), None, schema);
    let [warning] = store.diagnostics() else {
        panic!("one diagnostic: {:?}", store.diagnostics());
    };
    assert_eq!(warning.severity, Severity::Warning);
    assert_eq!(warning.key.as_deref(), Some("titel"));
    assert_eq!(store.settings().view.font_size, 15.0);

    fs::write(&files.user, "[view]\nfont_size = 100\n").unwrap();
    let event = store.reload(std::slice::from_ref(&files.user)).unwrap();
    assert_eq!(event.settings.view.font_size, 15.0);
    assert_eq!(event.diagnostics[0].severity, Severity::Error);
    assert_eq!(event.diagnostics[0].key.as_deref(), Some("view.font_size"));
}

/// A filter letting a project set only `view.wrap`, with a warning at each
/// other `view` key it drops.
fn view_wrap_only() -> ProjectFilter {
    ProjectFilter::new("view", |layer: &mut Layer| {
        let Some(toml::Value::Table(view)) = layer.table.remove("view") else {
            return;
        };
        let mut kept = toml::Table::new();
        for (key, value) in view {
            if key == "wrap" {
                kept.insert(key, value);
            } else {
                let warning = layer.diagnostic_at(
                    Severity::Warning,
                    Some(format!("view.{key}")),
                    0,
                    "not in a project".into(),
                );
                layer.diagnostics.push(warning);
            }
        }
        layer.table.insert("view".into(), toml::Value::Table(kept));
    })
}

#[test]
fn a_project_filter_narrows_only_the_projects_file() {
    let files = files(
        "[view]
font_size = 15
",
        "title = \"Project\"\n[view]\nfont_size = 30\nwrap = false\n",
    );
    let options = SettingsOptions::new().project_filter(view_wrap_only());
    let store: SettingsStore<Notes> = SettingsStore::load_with(
        Some(files.user.clone()),
        Some(files.project.clone()),
        options.clone(),
    );
    let settings = store.settings();
    assert_eq!(settings.title, "Project", "keys outside the filter's apply");
    assert_eq!(
        settings.view.font_size, 15.0,
        "the user's, not the project's"
    );
    assert!(!settings.view.wrap, "what the filter keeps applies");
    let [warning] = store.diagnostics() else {
        panic!("one diagnostic: {:?}", store.diagnostics());
    };
    assert_eq!(warning.severity, Severity::Warning);
    assert_eq!(warning.file, files.project);
    assert_eq!(warning.key.as_deref(), Some("view.font_size"));

    // A bad value in a key the filter drops rejects nothing: the narrowed
    // table decides.
    fs::write(
        &files.project,
        "title = \"Again\"\n[view]\nfont_size = \"big\"\n",
    )
    .unwrap();
    let mut store = store;
    let event = store.reload(std::slice::from_ref(&files.project)).unwrap();
    assert_eq!(event.settings.title, "Again");
    assert!(
        event.diagnostics.iter().all(|d| !d.is_error()),
        "{:?}",
        event.diagnostics
    );

    // One in a key it keeps is an error at the start of the file, and the
    // last good table stays.
    fs::write(&files.project, "title = \"Bad\"\n[view]\nwrap = 1\n").unwrap();
    let event = store.reload(std::slice::from_ref(&files.project)).unwrap();
    assert_eq!(event.settings.title, "Again");
    let [error] = &event.diagnostics[..] else {
        panic!("one diagnostic: {:?}", event.diagnostics);
    };
    assert!(error.is_error());
    assert_eq!((error.line, error.key.as_deref()), (1, None));

    // The user's file is never narrowed.
    fs::write(&files.user, "[view]\nfont_size = 30\n").unwrap();
    let store: SettingsStore<Notes> =
        SettingsStore::load_with(Some(files.user.clone()), None, options);
    assert_eq!(store.settings().view.font_size, 30.0);
    assert_eq!(store.diagnostics(), []);
}

#[test]
fn a_schema_can_follow_the_files_table() {
    // A schema that, for a file with a `view` table, allows only a small
    // font there.
    let base = schemars::schema_for!(Notes).to_value();
    let options = SettingsOptions::new().schema_for(move |table: &toml::Table| {
        let mut schema = base.clone();
        if table.contains_key("view") {
            schema["$defs"]["View"]["properties"]["font_size"]["maximum"] = 20.0.into();
        }
        Arc::new(schema)
    });
    let files = files("title = \"x\"\n[view]\nfont_size = 30\n", "");
    let store: SettingsStore<Notes> =
        SettingsStore::load_with(Some(files.user.clone()), None, options.clone());
    let [error] = store.diagnostics() else {
        panic!("one diagnostic: {:?}", store.diagnostics());
    };
    assert!(error.is_error());
    assert_eq!(error.key.as_deref(), Some("view.font_size"));
    assert_eq!(store.settings().view.font_size, 20.0, "the defaults stay");

    fs::write(&files.user, "[view]\nfont_size = 18\n").unwrap();
    let store: SettingsStore<Notes> =
        SettingsStore::load_with(Some(files.user.clone()), None, options);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().view.font_size, 18.0);
}

#[test]
fn a_layer_filter_narrows_the_users_file_too() {
    let files = files(
        "title = \"User\"\n[view]\nfont_size = 15\nwrap = false\n",
        "[view]\nfont_size = 20\n",
    );
    let options = SettingsOptions::new().layer_filter(view_wrap_only());
    let store: SettingsStore<Notes> = SettingsStore::load_with(
        Some(files.user.clone()),
        Some(files.project.clone()),
        options,
    );
    let settings = store.settings();
    assert_eq!(settings.title, "User");
    assert_eq!(
        settings.view.font_size, 20.0,
        "the default: dropped from both files"
    );
    assert!(!settings.view.wrap);
    let files_warned: Vec<_> = store.diagnostics().iter().map(|d| &d.file).collect();
    assert_eq!(files_warned, [&files.user, &files.project]);
}
