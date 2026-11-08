//! Workspace trust decisions in user state: `fe_config::trust` and the
//! state directory they live in.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};

use fe_config::paths::{state_dir_from, trust_path};
#[cfg(unix)]
use fe_config::trust::canonical;
use fe_config::trust::{Trust, TrustStore};

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// The names in `dir`.
fn listing(dir: &Path) -> Vec<OsString> {
    let mut names: Vec<OsString> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    names
}

#[test]
fn nearest_decision_wins() {
    let p = Path::new;
    let mut store = TrustStore::in_memory();
    assert_eq!(store.trust_of(p("/a")), Trust::Undecided);

    store.set(p("/a"), true).unwrap();
    assert_eq!(store.trust_of(p("/a")), Trust::Trusted);
    assert_eq!(store.trust_of(p("/a/b/c")), Trust::Trusted);
    assert_eq!(store.trust_of(p("/ab")), Trust::Undecided);
    assert_eq!(store.trust_of(p("/")), Trust::Undecided);

    store.set(p("/a/b"), false).unwrap();
    assert_eq!(store.trust_of(p("/a/b")), Trust::Untrusted);
    assert_eq!(store.trust_of(p("/a/b/c")), Trust::Untrusted);
    assert_eq!(store.trust_of(p("/a")), Trust::Trusted);

    store.set(p("/a/b/c"), true).unwrap();
    assert_eq!(store.trust_of(p("/a/b/c/d")), Trust::Trusted);
}

#[test]
#[cfg(unix)]
fn persists_across_loads() {
    let temp = tempfile::tempdir().unwrap();
    let base = canonical(temp.path());
    let state = base.join("state");
    let file = state.join("trust.toml");
    let (x, y) = (base.join("x"), base.join("y"));
    fs::create_dir(&x).unwrap();
    fs::create_dir(&y).unwrap();

    let mut store = TrustStore::load(&file);
    assert_eq!(store.trust_of(&x), Trust::Undecided);
    assert!(!state.exists(), "loading creates nothing");

    store.set(&x, true).unwrap();
    store.set(&y, false).unwrap();
    assert_eq!(mode(&state), 0o700);
    assert_eq!(mode(&file), 0o600);
    let text = fs::read_to_string(&file).unwrap();
    text.parse::<toml::Table>().expect("trust.toml is TOML");
    assert_eq!(listing(&state), ["trust.toml"]);

    let loaded = TrustStore::load(&file);
    assert_eq!(loaded.trust_of(&x), Trust::Trusted);
    assert_eq!(loaded.trust_of(&y), Trust::Untrusted);

    let link = base.join("link");
    symlink(&x, &link).unwrap();
    assert_eq!(canonical(&link), x);
    assert_eq!(loaded.trust_of(&canonical(&link)), Trust::Trusted);

    store.set(&x, false).unwrap();
    assert_eq!(TrustStore::load(&file).trust_of(&x), Trust::Untrusted);
}

#[test]
fn bad_files_fail_closed() {
    for contents in ["[folders\n\"/a\" = true\n", "[folders]\n\"/a\" = 3\n"] {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("trust.toml");
        fs::write(&file, contents).unwrap();

        let mut store = TrustStore::load(&file);
        for folder in ["/a", "/a/b", "/"] {
            assert_eq!(
                store.trust_of(Path::new(folder)),
                Trust::Undecided,
                "{contents:?}"
            );
        }
        assert!(store.set(Path::new("/a"), true).is_err(), "{contents:?}");
        assert_eq!(store.trust_of(Path::new("/a")), Trust::Trusted);
        assert_eq!(fs::read(&file).unwrap(), contents.as_bytes());
        assert_eq!(listing(temp.path()), ["trust.toml"]);
    }
}

#[test]
fn state_dir_resolution() {
    // `/state/fe` isn't absolute on Windows, which wants a drive.
    let abs = |path: &str| match path.strip_prefix('/') {
        Some(_) if cfg!(windows) => PathBuf::from(format!("C:{path}")),
        _ => PathBuf::from(path),
    };
    let resolve = |vars: &[(&str, &str)]| {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|&(name, value)| (name.to_owned(), abs(value).into_os_string()))
            .collect();
        state_dir_from(|name| vars.get(name).cloned())
    };
    let all = [
        ("TENMOKU_STATE_DIR", "/state/fe"),
        ("XDG_STATE_HOME", "/xdg"),
        ("HOME", "/home/u"),
    ];
    assert_eq!(resolve(&all), Some(abs("/state/fe")));
    assert_eq!(resolve(&all[1..]), Some(abs("/xdg/tenmoku")));
    assert_eq!(
        resolve(&all[2..]),
        Some(abs("/home/u/.local/state/tenmoku"))
    );
    assert_eq!(resolve(&[]), None);

    // Empty variables, and a relative XDG_STATE_HOME, count as unset.
    assert_eq!(
        resolve(&[("TENMOKU_STATE_DIR", ""), ("XDG_STATE_HOME", "/xdg")]),
        Some(abs("/xdg/tenmoku"))
    );
    assert_eq!(
        resolve(&[("XDG_STATE_HOME", "relative"), ("HOME", "/home/u")]),
        Some(abs("/home/u/.local/state/tenmoku"))
    );
    assert_eq!(
        resolve(&[("XDG_STATE_HOME", ""), ("HOME", "/home/u")]),
        Some(abs("/home/u/.local/state/tenmoku"))
    );
    assert_eq!(resolve(&[("HOME", "")]), None);

    let expected = state_dir_from(|name| std::env::var_os(name)).map(|dir| dir.join("trust.toml"));
    assert_eq!(trust_path(), expected);
}
