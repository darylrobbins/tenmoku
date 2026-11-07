use std::{collections::HashMap, ffi::OsString, path::PathBuf};

use crate::{SettingsStore, config_dir_from, load_layer, merge};

use super::write;

#[test]
fn layers_merge_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let user = write(
        &dir,
        "user.toml",
        "[editor]\nfont_size = 15.5\ntab_size = 8\n",
    );
    let project = write(&dir, "project.toml", "editor.tab_size = 2\n");
    let store = SettingsStore::load(Some(user.clone()), Some(project));
    let editor = &store.settings().editor;
    assert_eq!(editor.tab_size, 2, "the project overrides the user");
    assert_eq!(editor.font_size, 15.5, "the user's other keys survive");
    assert_eq!(editor.line_height, 1.6, "the defaults fill the rest");
    assert!(store.diagnostics().is_empty(), "{:?}", store.diagnostics());

    // Tables merge key by key, at any depth.
    let mut base: toml::Table = toml::from_str("a = 1\n[t]\nx = 1\ny = 1\n").unwrap();
    let over: toml::Table = toml::from_str("b = 2\n[t]\ny = 2\n").unwrap();
    merge(&mut base, &over);
    let expected: toml::Table = toml::from_str("a = 1\nb = 2\n[t]\nx = 1\ny = 2\n").unwrap();
    assert_eq!(base, expected);

    // A missing file is an empty layer.
    let missing = dir.path().join("missing.toml");
    let layer = load_layer(&missing);
    assert!(layer.table.is_empty());
    assert!(layer.diagnostics.is_empty());
    let store = SettingsStore::load(Some(user), Some(missing));
    assert_eq!(store.settings().editor.tab_size, 8);
    assert!(store.diagnostics().is_empty());
}

#[test]
fn config_dir_resolution() {
    // `/override` isn't absolute on Windows, which wants a drive.
    let abs = |path: &str| match path.strip_prefix('/') {
        Some(_) if cfg!(windows) => PathBuf::from(format!("C:{path}")),
        _ => PathBuf::from(path),
    };
    let resolve = |vars: &[(&str, &str)]| {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|(name, value)| (name.to_string(), abs(value).into_os_string()))
            .collect();
        config_dir_from(|name| vars.get(name).cloned())
    };
    let all = [
        ("TENMOKU_CONFIG_DIR", "/override"),
        ("XDG_CONFIG_HOME", "/xdg"),
        ("HOME", "/home/me"),
    ];
    assert_eq!(resolve(&all), Some(abs("/override")));
    assert_eq!(resolve(&all[1..]), Some(abs("/xdg/tenmoku")));
    assert_eq!(resolve(&all[2..]), Some(abs("/home/me/.config/tenmoku")));
    // Empty or relative values don't count.
    assert_eq!(
        resolve(&[
            ("TENMOKU_CONFIG_DIR", ""),
            ("XDG_CONFIG_HOME", "relative"),
            ("HOME", "/home/me")
        ]),
        Some(abs("/home/me/.config/tenmoku"))
    );
    assert_eq!(resolve(&[]), None);
}

#[test]
fn only_small_regular_files_are_read() {
    let dir = tempfile::tempdir().unwrap();
    let big = write(&dir, "big.toml", &"# padding\n".repeat(200_000));
    let mut paths = vec![big, dir.path().to_owned()];
    // Windows has no FIFOs in the file system.
    if cfg!(unix) {
        let fifo = dir.path().join("fifo.toml");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        paths.push(fifo);
    }
    for path in paths {
        let layer = load_layer(&path);
        assert!(layer.table.is_empty());
        let [diagnostic] = layer.diagnostics.as_slice() else {
            panic!(
                "one diagnostic for {}: {:?}",
                path.display(),
                layer.diagnostics
            );
        };
        assert!(diagnostic.is_error(), "{diagnostic}");
    }
}
