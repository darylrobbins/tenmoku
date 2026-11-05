// SPDX-License-Identifier: Apache-2.0

//! Where an app's files go, per platform and style, from a fake environment.

use std::{collections::HashMap, ffi::OsString, path::Path};

use crate::{AppId, AppPaths, Os, PathStyle};

fn notes() -> AppId {
    AppId::new("com.example.notes", "notes", "Notes").vendor("Example")
}

fn paths(style: PathStyle, os: Os, vars: &[(&str, &str)]) -> AppPaths {
    let env: HashMap<String, OsString> = vars
        .iter()
        .map(|(name, value)| (name.to_string(), OsString::from(value)))
        .collect();
    AppPaths::resolve(&notes(), style, os, |name| env.get(name).cloned())
}

#[test]
fn linux_follows_xdg() {
    let home = [("HOME", "/home/u")];
    let p = paths(PathStyle::Native, Os::Linux, &home);
    assert_eq!(p.config_dir(), Some(Path::new("/home/u/.config/notes")));
    assert_eq!(p.state_dir(), Some(Path::new("/home/u/.local/state/notes")));
    assert_eq!(p.cache_dir(), Some(Path::new("/home/u/.cache/notes")));

    let p = paths(
        PathStyle::Native,
        Os::Linux,
        &[
            ("HOME", "/home/u"),
            ("XDG_CONFIG_HOME", "/cfg"),
            ("XDG_STATE_HOME", "relative/state"),
            ("XDG_CACHE_HOME", ""),
        ],
    );
    assert_eq!(p.config_dir(), Some(Path::new("/cfg/notes")));
    // Relative and empty XDG variables count as unset.
    assert_eq!(p.state_dir(), Some(Path::new("/home/u/.local/state/notes")));
    assert_eq!(p.cache_dir(), Some(Path::new("/home/u/.cache/notes")));
}

#[test]
fn macos_uses_application_support_or_xdg() {
    let home = [("HOME", "/Users/u")];
    let p = paths(PathStyle::Native, Os::MacOs, &home);
    let support = "/Users/u/Library/Application Support/com.example.notes";
    assert_eq!(p.config_dir(), Some(Path::new(support)));
    assert_eq!(
        p.state_dir(),
        Some(Path::new(&format!("{support}/State")[..]))
    );
    assert_eq!(
        p.cache_dir(),
        Some(Path::new("/Users/u/Library/Caches/com.example.notes"))
    );

    let p = paths(PathStyle::Xdg, Os::MacOs, &home);
    assert_eq!(p.config_dir(), Some(Path::new("/Users/u/.config/notes")));
}

#[test]
fn windows_uses_appdata() {
    let p = paths(
        PathStyle::Native,
        Os::Windows,
        &[
            ("APPDATA", r"C:\Users\u\AppData\Roaming"),
            ("LOCALAPPDATA", r"C:\Users\u\AppData\Local"),
        ],
    );
    let roaming = Path::new(r"C:\Users\u\AppData\Roaming")
        .join("Example")
        .join("Notes");
    let local = Path::new(r"C:\Users\u\AppData\Local")
        .join("Example")
        .join("Notes");
    assert_eq!(p.config_dir(), Some(roaming.as_path()));
    assert_eq!(p.state_dir(), Some(local.join("State").as_path()));
    assert_eq!(p.cache_dir(), Some(local.join("Cache").as_path()));
}

#[test]
fn the_app_variables_win() {
    let vars = [
        ("HOME", "/home/u"),
        ("XDG_CONFIG_HOME", "/cfg"),
        ("NOTES_CONFIG_DIR", "/override/config"),
        ("NOTES_STATE_DIR", "/override/state"),
    ];
    for os in [Os::Linux, Os::MacOs, Os::Windows] {
        let p = paths(PathStyle::Native, os, &vars);
        assert_eq!(
            p.config_dir(),
            Some(Path::new("/override/config")),
            "{os:?}"
        );
        assert_eq!(p.state_dir(), Some(Path::new("/override/state")), "{os:?}");
    }
    let empty = paths(
        PathStyle::Native,
        Os::Linux,
        &[("HOME", "/h"), ("NOTES_CONFIG_DIR", "")],
    );
    assert_eq!(empty.config_dir(), Some(Path::new("/h/.config/notes")));
}

#[test]
fn files_within_the_directories() {
    let p = paths(PathStyle::Xdg, Os::Linux, &[("HOME", "/h")]);
    assert_eq!(
        p.settings_file().unwrap(),
        Path::new("/h/.config/notes/settings.toml")
    );
    assert_eq!(
        p.keymap_file().unwrap(),
        Path::new("/h/.config/notes/keymap.toml")
    );
    assert_eq!(
        p.themes_dir().unwrap(),
        Path::new("/h/.config/notes/themes")
    );
    assert_eq!(
        p.trust_file().unwrap(),
        Path::new("/h/.local/state/notes/trust.toml")
    );
    assert_eq!(
        p.project_settings_file(Path::new("/src/app")),
        Path::new("/src/app/.notes/settings.toml")
    );

    let homeless = paths(PathStyle::Native, Os::Linux, &[]);
    assert_eq!(homeless.config_dir(), None);
    assert_eq!(homeless.settings_file(), None);
}

#[test]
fn the_env_prefix_comes_from_the_short_name() {
    let id = AppId::new("org.example.my-app", "my-app", "My App");
    let p = AppPaths::resolve(&id, PathStyle::Xdg, Os::Linux, |name| {
        (name == "MY_APP_CONFIG_DIR").then(|| "/x".into())
    });
    assert_eq!(p.config_dir(), Some(Path::new("/x")));
}
