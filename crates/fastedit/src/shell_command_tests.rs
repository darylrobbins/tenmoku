use std::{ffi::OsString, fs, os::unix::fs::symlink, path::Path};

use super::{install, link_in, path_hint, uninstall};

const PATH: &str = "/usr/bin:/home/x/.local/bin";

#[test]
fn install_links_a_bundle_binary_path_with_spaces_and_creates_the_directory() {
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join("My Apps/Tenmoku.app/Contents/MacOS");
    fs::create_dir_all(&bin).unwrap();
    let target = bin.join("tenmoku");
    fs::write(&target, "").unwrap();
    let link = link_in(home.path());
    assert!(!link.parent().unwrap().exists());
    let report = install(&link, &target, Some(PATH.as_ref()));
    assert_ne!(report.level, log::Level::Error, "{}", report.message);
    assert_eq!(fs::read_link(&link).unwrap(), target);
    assert!(report.message.contains(&link.display().to_string()));
    assert!(report.message.contains(&target.display().to_string()));
}

#[test]
fn install_replaces_our_own_symlink_but_refuses_any_other_file_and_names_it() {
    let home = tempfile::tempdir().unwrap();
    let old = home.path().join("old/tenmoku");
    let new = home.path().join("new/tenmoku");
    let link = link_in(home.path());
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    symlink(&old, &link).unwrap();
    let report = install(&link, &new, None);
    assert_ne!(report.level, log::Level::Error, "{}", report.message);
    assert_eq!(fs::read_link(&link).unwrap(), new);

    // A regular file, and a link to something else, stay.
    fs::remove_file(&link).unwrap();
    fs::write(&link, "mine").unwrap();
    let report = install(&link, &new, None);
    assert_eq!(report.level, log::Level::Error);
    assert!(report.message.contains(&link.display().to_string()));
    assert_eq!(fs::read_to_string(&link).unwrap(), "mine");

    fs::remove_file(&link).unwrap();
    symlink("/usr/bin/true", &link).unwrap();
    let report = install(&link, &new, None);
    assert_eq!(report.level, log::Level::Error);
    assert_eq!(fs::read_link(&link).unwrap(), Path::new("/usr/bin/true"));
}

#[test]
fn uninstall_removes_only_a_symlink_that_points_at_a_tenmoku_binary() {
    let home = tempfile::tempdir().unwrap();
    let link = link_in(home.path());
    assert_ne!(uninstall(&link).level, log::Level::Error);
    fs::create_dir_all(link.parent().unwrap()).unwrap();

    symlink("/Applications/Tenmoku.app/Contents/MacOS/tenmoku", &link).unwrap();
    assert_ne!(uninstall(&link).level, log::Level::Error);
    assert!(fs::symlink_metadata(&link).is_err());

    symlink("/usr/bin/true", &link).unwrap();
    assert_eq!(uninstall(&link).level, log::Level::Error);
    assert!(fs::symlink_metadata(&link).is_ok());
    fs::remove_file(&link).unwrap();

    fs::write(&link, "mine").unwrap();
    assert_eq!(uninstall(&link).level, log::Level::Error);
    assert_eq!(fs::read_to_string(&link).unwrap(), "mine");
}

#[test]
fn path_hint_names_the_export_line_only_when_the_directory_is_not_on_path() {
    let dir = Path::new("/home/x/.local/bin");
    assert_eq!(path_hint(dir, Some(PATH.as_ref())), None);
    let hint = path_hint(dir, Some(OsString::from("/usr/bin").as_os_str())).unwrap();
    assert_eq!(hint, "export PATH=\"/home/x/.local/bin:$PATH\"");
    assert!(path_hint(dir, None).is_some());

    // Nothing in the directory's name is left for the shell to expand.
    let off = Some(OsString::from("/usr/bin"));
    for (home, line) in [
        ("/tmp/home$USER", "export PATH='/tmp/home$USER':\"$PATH\""),
        ("/tmp/`id`", "export PATH='/tmp/`id`':\"$PATH\""),
        ("/tmp/$(id)", "export PATH='/tmp/$(id)':\"$PATH\""),
        ("/tmp/a\"b\\c", "export PATH='/tmp/a\"b\\c':\"$PATH\""),
        ("/tmp/it's", "export PATH='/tmp/it'\\''s':\"$PATH\""),
    ] {
        assert_eq!(path_hint(Path::new(home), off.as_deref()).unwrap(), line);
    }
}
