// SPDX-License-Identifier: Apache-2.0

//! Writing a setting while the file changes under the writer.

#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::{fs, path::Path};

use crate::write::{SYNCED, write_setting};

/// The commented-out defaults a missing file is created from.
const TEMPLATE: &str = "# Settings.\n\n[vim]\n# enabled = false\n";

const OFF: &str = "[vim]\nenabled = false\n";
const ON: &str = "[vim]\nenabled = true\n";

/// Turns vim on in `file`, with `change` made to it once the first new
/// text is synced, just before that would be put in place.
fn write_after(file: &Path, change: impl FnOnce(&Path) + 'static) {
    let mut change = Some(change);
    SYNCED.set(Some(Box::new(move |path: &Path| {
        if let Some(change) = change.take() {
            change(path);
        }
    })));
    let written = write_setting(file, &["vim", "enabled"], true, TEMPLATE);
    SYNCED.set(None);
    written.unwrap();
}

/// A file saved, created, removed or given other permissions since
/// write_setting read it isn't written over: the setting is made to
/// what's there now.
#[test]
#[cfg(unix)]
fn a_change_meanwhile_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");

    fs::write(&file, OFF).unwrap();
    write_after(&file, |path| {
        fs::write(path, format!("# Saved.\n{OFF}")).unwrap();
    });
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        format!("# Saved.\n{ON}")
    );

    fs::remove_file(&file).unwrap();
    write_after(&file, |path| fs::write(path, "# Created.\n").unwrap());
    let created = fs::read_to_string(&file).unwrap();
    assert_eq!(created, format!("# Created.\n\n{ON}"));

    fs::write(&file, OFF).unwrap();
    write_after(&file, |path| fs::remove_file(path).unwrap());
    let template = TEMPLATE.replace("# enabled = false", "enabled = true");
    assert_eq!(fs::read_to_string(&file).unwrap(), template);

    fs::write(&file, OFF).unwrap();
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    write_after(&file, |path| {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    });
    assert_eq!(fs::read_to_string(&file).unwrap(), ON);
    assert_eq!(mode(&file), 0o600);

    let left = fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(left, 1, "no temporary file is left");
}

/// A symlink put in place of the file, or pointed at another, since
/// write_setting resolved it is followed to the file it names now, and
/// stays a link, even where that file holds the same text.
#[test]
#[cfg(unix)]
fn a_link_changed_meanwhile_is_followed() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.toml");
    let (a, b) = (dir.path().join("a.toml"), dir.path().join("b.toml"));

    fs::write(&file, OFF).unwrap();
    fs::write(&a, OFF).unwrap();
    write_after(&file, {
        let a = a.clone();
        move |path| {
            fs::remove_file(path).unwrap();
            symlink(&a, path).unwrap();
        }
    });
    assert_eq!(fs::read_link(&file).unwrap(), a);
    assert_eq!(fs::read_to_string(&a).unwrap(), ON);

    fs::write(&a, OFF).unwrap();
    fs::write(&b, OFF).unwrap();
    write_after(&file, {
        let (file, b) = (file.clone(), b.clone());
        move |_| {
            fs::remove_file(&file).unwrap();
            symlink(&b, &file).unwrap();
        }
    });
    assert_eq!(fs::read_link(&file).unwrap(), b);
    assert_eq!(fs::read_to_string(&a).unwrap(), OFF);
    assert_eq!(fs::read_to_string(&b).unwrap(), ON);

    let left = fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(left, 3, "no temporary file is left");
}
