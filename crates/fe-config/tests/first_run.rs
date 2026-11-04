//! The first-run decision and its marker.

use std::{fs, path::Path};

use fe_config::USER_SETTINGS_TEMPLATE;
use fe_config::first_run::{FirstRun, Outcome, decide, record};
use tempfile::TempDir;

fn dirs() -> (TempDir, TempDir) {
    (TempDir::new().unwrap(), TempDir::new().unwrap())
}

fn show(config: &Path, state: &Path) -> FirstRun {
    decide(Some(config), Some(state))
}

#[test]
fn fresh_dirs_show_setup() {
    let (config, state) = dirs();
    assert_eq!(show(config.path(), state.path()), FirstRun::Show);
}

#[test]
fn template_settings_still_show() {
    let (config, state) = dirs();
    fs::write(config.path().join("settings.toml"), USER_SETTINGS_TEMPLATE).unwrap();
    assert_eq!(show(config.path(), state.path()), FirstRun::Show);
}

#[test]
fn existing_users_are_not_asked() {
    let cases: [(&str, &str); 7] = [
        ("config/settings.toml", "[editor]\nfont_size = 14\n"),
        ("config/settings.toml", "this is [not toml"),
        ("config/keymap.toml", ""),
        ("config/theme.toml", ""),
        ("config/vimrc", ""),
        ("state/recent.toml", ""),
        ("state/layouts/one.toml", ""),
    ];
    for (file, text) in cases {
        let (config, state) = dirs();
        let path = match file.split_once('/').unwrap() {
            ("config", rest) => config.path().join(rest),
            (_, rest) => state.path().join(rest),
        };
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        assert_eq!(
            show(config.path(), state.path()),
            FirstRun::Existing,
            "{file}"
        );
    }
}

#[test]
fn empty_layouts_dir_is_not_existing() {
    let (config, state) = dirs();
    fs::create_dir(state.path().join("layouts")).unwrap();
    assert_eq!(show(config.path(), state.path()), FirstRun::Show);
}

#[test]
fn marker_wins() {
    for text in [
        "outcome = \"shown\"\n",
        "outcome = \"completed\"\n",
        "outcome = \"skipped\"\n",
        "outcome = \"existing\"\n",
        "outcome = \"whatever\"\n",
        "not [toml",
    ] {
        let (config, state) = dirs();
        fs::write(state.path().join("setup.toml"), text).unwrap();
        fs::write(config.path().join("keymap.toml"), "").unwrap();
        assert_eq!(show(config.path(), state.path()), FirstRun::Done, "{text}");
    }
    let (config, _) = dirs();
    assert_eq!(decide(Some(config.path()), None), FirstRun::Done);
}

#[test]
fn record_replaces_the_outcome() {
    let (_, root) = dirs();
    let state = root.path().join("missing/state");
    record(&state, Outcome::Shown).unwrap();
    assert_eq!(decide(None, Some(&state)), FirstRun::Done);
    record(&state, Outcome::Completed).unwrap();
    let table: toml::Table = fs::read_to_string(state.join("setup.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(table["outcome"].as_str(), Some("completed"));
    assert_eq!(table["version"].as_str(), Some(env!("CARGO_PKG_VERSION")));
    let names: Vec<_> = fs::read_dir(&state)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["setup.toml"], "no temp file left behind");
}

#[test]
fn concurrent_records_leave_one_marker() {
    let (_, root) = dirs();
    let state = root.path().to_owned();
    std::thread::scope(|scope| {
        for i in 0..8 {
            let state = &state;
            scope.spawn(move || {
                for _ in 0..20 {
                    let outcome = if i % 2 == 0 {
                        Outcome::Shown
                    } else {
                        Outcome::Skipped
                    };
                    record(state, outcome).unwrap();
                }
            });
        }
    });
    let names: Vec<_> = fs::read_dir(&state)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["setup.toml"]);
}

#[cfg(unix)]
#[test]
fn unreadable_state_dir_never_asks() {
    use std::os::unix::fs::PermissionsExt;
    let (config, state) = dirs();
    fs::write(state.path().join("setup.toml"), "outcome = \"shown\"\n").unwrap();
    fs::set_permissions(state.path(), fs::Permissions::from_mode(0o000)).unwrap();
    let got = show(config.path(), state.path());
    fs::set_permissions(state.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert_ne!(got, FirstRun::Show);
}
