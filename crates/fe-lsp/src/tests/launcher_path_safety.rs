use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::launcher::LoginPath;

/// The `PATH` list separator: `:` on Unix, `;` on Windows.
const SEP: &str = if cfg!(windows) { ";" } else { ":" };

/// A program named `name` in `dir`: a script with an execute bit on Unix,
/// `name.exe` on Windows, which runs what has a `PATHEXT` extension.
fn executable(dir: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    #[cfg(unix)]
    let file = dir.join(name);
    #[cfg(windows)]
    let file = dir.join(format!("{name}.exe"));
    std::fs::write(&file, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    file
}

fn resolve(login: &LoginPath, path: impl Into<OsString>, root: &Path) -> Option<PathBuf> {
    login.resolve(Path::new("rust-analyzer"), Some(&path.into()), root)
}

#[test]
fn relative_path_entries_are_not_searched() {
    let root = tempfile::tempdir().unwrap();
    executable(root.path(), "rust-analyzer");
    executable(&root.path().join("node_modules/.bin"), "rust-analyzer");
    let login = LoginPath::with_home("/bin/sh", None);
    let nonexistent = format!("/nonexistent{SEP}");
    for path in [".", "", &nonexistent, "node_modules/.bin"] {
        assert_eq!(resolve(&login, path, root.path()), None, "PATH {path:?}");
    }

    let bin = tempfile::tempdir().unwrap();
    let want = executable(bin.path(), "rust-analyzer");
    let path = [
        "",
        ".",
        "node_modules/.bin",
        &bin.path().display().to_string(),
    ]
    .join(SEP);
    assert_eq!(resolve(&login, path, root.path()), Some(want));
}

#[test]
fn home_and_absolute_entries_still_work() {
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let abs = tempfile::tempdir().unwrap();
    let login = LoginPath::with_home("/bin/sh", Some(home.path().to_owned()));

    let abs_bin = executable(abs.path(), "rust-analyzer");
    let path = format!("~/bin{SEP}{}", abs.path().display());
    assert_eq!(resolve(&login, path.clone(), root.path()), Some(abs_bin));

    let home_bin = executable(&home.path().join("bin"), "rust-analyzer");
    assert_eq!(resolve(&login, path, root.path()), Some(home_bin));

    assert_eq!(
        login.resolve(Path::new("./bin/server"), None, root.path()),
        Some(root.path().join("./bin/server"))
    );
}
