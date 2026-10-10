// SPDX-License-Identifier: LicenseRef-Proprietary

//! A typed [`SettingsStore`] over an app's own settings struct.

use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{SettingsStore, Severity};

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
