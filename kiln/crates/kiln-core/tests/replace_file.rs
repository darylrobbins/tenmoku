// SPDX-License-Identifier: Apache-2.0

//! `replace_file` over a target another handle holds open: on Windows, one opened without delete
//! sharing, which a plain rename can't replace.

use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use kiln_core::platform::replace_file;

fn dir(name: &str) -> PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("kiln-replace-file-{}-{name}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Opens `path` for reading, without delete sharing on Windows.
fn hold(path: &std::path::Path) -> File {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::share_mode(&mut options, 1 | 2); // read | write
    options.open(path).unwrap()
}

#[test]
fn replace_file_succeeds_over_a_target_held_open() {
    let dir = dir("held");
    let (from, to) = (dir.join("from"), dir.join("to"));
    fs::write(&from, "new").unwrap();
    fs::write(&to, "old").unwrap();
    let held = hold(&to);
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        drop(held);
    });
    replace_file(&from, &to).unwrap();
    releaser.join().unwrap();
    assert_eq!(fs::read_to_string(&to).unwrap(), "new");
}

/// On Windows the target is held for the whole test; on Unix, where nothing holds a rename back,
/// it is a non-empty directory, which a rename can't replace.
#[test]
fn replace_file_gives_up_after_one_second() {
    let dir = dir("gives-up");
    let (from, to) = (dir.join("from"), dir.join("to"));
    fs::write(&from, "new").unwrap();
    #[cfg(windows)]
    let (_held, kept) = {
        fs::write(&to, "old").unwrap();
        (hold(&to), to.clone())
    };
    #[cfg(not(windows))]
    let kept = {
        fs::create_dir(&to).unwrap();
        fs::write(to.join("inside"), "old").unwrap();
        to.join("inside")
    };
    let started = Instant::now();
    assert!(replace_file(&from, &to).is_err());
    assert!(started.elapsed() < Duration::from_millis(1500));
    assert_eq!(fs::read_to_string(&kept).unwrap(), "old");
}
