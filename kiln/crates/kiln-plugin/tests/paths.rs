// SPDX-License-Identifier: Apache-2.0

//! How host paths map to guest paths and back (`kiln_plugin::paths`), under both OSes' rules on
//! every OS: the mapping is pure string code.

use std::path::Path;
use std::sync::Mutex;

use kiln_plugin::bindings::Error;
use kiln_plugin::paths::{self, Rules, Unmapped, root_to_guest, to_guest, to_host, to_host_within};

#[test]
fn paths_drive_path_round_trips() {
    for (host, guest) in [
        (r"C:\Users\d\work\a.rs", "/C:/Users/d/work/a.rs"),
        (r"D:\", "/D:/"),
        (r"C:\Users\d\naïve dir\b.txt", "/C:/Users/d/naïve dir/b.txt"),
    ] {
        assert_eq!(to_guest(host, Rules::Windows).as_deref(), Ok(guest));
        assert_eq!(to_host(guest, Rules::Windows).as_deref(), Ok(host));
    }
    // A bare drive is the current directory on it, not its root: it maps neither way, is not
    // preopened, and grants nothing.
    assert_eq!(to_guest("D:", Rules::Windows), Err(Unmapped::NotAbsolute));
    assert_eq!(
        root_to_guest("D:", Rules::Windows),
        Err(Unmapped::NotAbsolute)
    );
    let host = to_host("/D:", Rules::Windows);
    assert!(matches!(host, Err(Error::InvalidArgument(_))), "{host:?}");
    let host = to_host_within("/D:/outside", Rules::Windows, ["D:"]);
    assert!(matches!(host, Err(Error::Denied(_))), "{host:?}");
    // `/` separates on Windows too, and a file over 260 characters under a short root keeps
    // `\\?\`, which its guest path drops.
    assert_eq!(
        to_guest("C:/work/a.rs", Rules::Windows).as_deref(),
        Ok("/C:/work/a.rs")
    );
    let deep = format!(r"C:\work\{}\a.rs", "d".repeat(300));
    let guest = to_guest(&format!(r"\\?\{deep}"), Rules::Windows).unwrap();
    assert_eq!(guest, format!("/C:/work/{}/a.rs", "d".repeat(300)));
    assert_eq!(to_host(&guest, Rules::Windows).unwrap(), deep);
}

#[test]
fn paths_lowercase_drive_is_uppercased() {
    assert_eq!(
        to_guest(r"c:\work\a.rs", Rules::Windows).as_deref(),
        Ok("/C:/work/a.rs")
    );
    assert_eq!(
        to_guest(r"\\?\e:\work\a.rs", Rules::Windows).as_deref(),
        Ok("/E:/work/a.rs")
    );
    assert_eq!(
        root_to_guest(r"d:\work", Rules::Windows).as_deref(),
        Ok("/D:/work")
    );
    assert_eq!(
        to_host("/c:/work/a.rs", Rules::Windows).unwrap(),
        r"C:\work\a.rs"
    );
}

#[test]
fn paths_guest_backslash_is_invalid_argument() {
    for guest in [r"/C:/work\a.rs", r"/C:\work", r"\C:\work"] {
        let host = to_host(guest, Rules::Windows);
        assert!(
            matches!(&host, Err(Error::InvalidArgument(why)) if why.contains('\\')),
            "{guest}: {host:?}"
        );
    }
}

#[test]
fn paths_guest_without_drive_prefix_is_invalid_argument_on_windows_rules() {
    for guest in [
        "/work/a.rs",
        "C:/work/a.rs",
        "/C:work/a.rs",
        "/1:/work",
        "//server/share/a.rs",
        "work/a.rs",
        "",
    ] {
        let host = to_host(guest, Rules::Windows);
        assert!(
            matches!(host, Err(Error::InvalidArgument(_))),
            "{guest:?}: {host:?}"
        );
    }
    // Host paths with no drive have no guest path either.
    for host in [r"\work\a.rs", r"C:work\a.rs", r"work\a.rs", ""] {
        assert_eq!(
            to_guest(host, Rules::Windows),
            Err(Unmapped::NotAbsolute),
            "{host:?}"
        );
    }
}

/// The host's warnings, by message.
static WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Warnings;

impl log::Log for Warnings {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            // Format before locking, so a lazily formatted argument can't re-enter the logger.
            let line = record.args().to_string();
            WARNINGS.lock().unwrap().push(line);
        }
    }
    fn flush(&self) {}
}

#[test]
fn paths_unc_and_verbatim_roots_are_skipped_with_a_warning() {
    log::set_logger(&Warnings).unwrap();
    log::set_max_level(log::LevelFilter::Warn);

    let long = format!(r"C:\{}", "r".repeat(300));
    for (root, why) in [
        (r"\\server\share".to_owned(), Unmapped::Unc),
        (r"\\server\share\work".to_owned(), Unmapped::Unc),
        ("//server/share/work".to_owned(), Unmapped::Unc),
        (r"\\?\UNC\server\share\work".to_owned(), Unmapped::Unc),
        (r"\\?\unc\server\share".to_owned(), Unmapped::Unc),
        (format!(r"\\?\{long}"), Unmapped::LongRoot),
        (long.clone(), Unmapped::LongRoot),
        (r"\\.\PhysicalDrive0".to_owned(), Unmapped::Device),
    ] {
        assert_eq!(root_to_guest(&root, Rules::Windows), Err(why), "{root}");
        WARNINGS.lock().unwrap().clear();
        assert_eq!(paths::root(Path::new(&root), Rules::Windows), None);
        let warnings = WARNINGS.lock().unwrap().clone();
        assert!(
            matches!(&warnings[..], [warning] if warning.contains(&root) && warning.contains(&why.to_string())),
            "{root}: {warnings:?}"
        );
    }
    // A network share has no guest path as a buffer's path either.
    assert_eq!(
        to_guest(r"\\server\share\a.rs", Rules::Windows),
        Err(Unmapped::Unc)
    );

    // A root at 260 characters is preopened, without a warning.
    let fits = format!(r"C:\{}", "r".repeat(257));
    assert_eq!(fits.len(), paths::MAX_ROOT);
    WARNINGS.lock().unwrap().clear();
    assert_eq!(
        paths::root(Path::new(&fits), Rules::Windows),
        Some(format!("/C:/{}", "r".repeat(257)))
    );
    assert_eq!(WARNINGS.lock().unwrap().len(), 0);
}

#[test]
fn paths_identity_on_unix_rules() {
    for path in [
        "/",
        "/Users/d/work/a.rs",
        r"/work/odd\name",
        "/C:/work",
        "/work/naïve dir/b.txt",
    ] {
        assert_eq!(to_guest(path, Rules::Unix).as_deref(), Ok(path));
        assert_eq!(root_to_guest(path, Rules::Unix).as_deref(), Ok(path));
        assert_eq!(to_host(path, Rules::Unix).as_deref(), Ok(path));
    }
    // Only absolute paths cross.
    assert_eq!(
        to_guest("work/a.rs", Rules::Unix),
        Err(Unmapped::NotAbsolute)
    );
    assert!(matches!(
        to_host("work/a.rs", Rules::Unix),
        Err(Error::InvalidArgument(_))
    ));
    // A long root is fine on Unix.
    let long = format!("/{}", "r".repeat(300));
    assert_eq!(root_to_guest(&long, Rules::Unix), Ok(long));
}

#[test]
fn paths_outside_every_root_are_refused_after_mapping() {
    let roots = [r"C:\work", r"\\server\share\work"];
    assert_eq!(
        to_host_within("/C:/work/src/a.rs", Rules::Windows, roots).as_deref(),
        Ok(r"C:\work\src\a.rs")
    );
    assert_eq!(
        to_host_within("/c:/work", Rules::Windows, roots).as_deref(),
        Ok(r"C:\work")
    );
    for guest in [
        "/C:/workshop/a.rs",
        "/C:/other/a.rs",
        "/D:/work/a.rs",
        "/C:/",
    ] {
        let host = to_host_within(guest, Rules::Windows, roots);
        assert!(matches!(host, Err(Error::Denied(_))), "{guest}: {host:?}");
    }
    // A root with no guest path reaches nothing, and `..` is refused before any root is
    // looked at.
    let host = to_host_within("//server/share/work/a.rs", Rules::Windows, roots);
    assert!(matches!(host, Err(Error::InvalidArgument(_))), "{host:?}");
    for guest in ["/C:/work/../secret", "/C:/work/./a.rs"] {
        let host = to_host_within(guest, Rules::Windows, roots);
        assert!(
            matches!(host, Err(Error::InvalidArgument(_))),
            "{guest}: {host:?}"
        );
    }

    let roots = ["/home/d/work"];
    assert_eq!(
        to_host_within("/home/d/work/a.rs", Rules::Unix, roots).as_deref(),
        Ok("/home/d/work/a.rs")
    );
    for guest in ["/home/d/workshop", "/home/d", "/etc/passwd"] {
        let host = to_host_within(guest, Rules::Unix, roots);
        assert!(matches!(host, Err(Error::Denied(_))), "{guest}: {host:?}");
    }
    let host = to_host_within("/home/d/work/../.ssh/id", Rules::Unix, roots);
    assert!(matches!(host, Err(Error::InvalidArgument(_))), "{host:?}");
    let host = to_host_within("/home/d/work/a.rs", Rules::Unix, [] as [&str; 0]);
    assert!(matches!(host, Err(Error::Denied(_))), "{host:?}");
}
