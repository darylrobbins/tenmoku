//! Where a build's daemon listens (D § 3).

#![cfg(unix)]

use std::path::Path;
use tenmoku_server::Paths;

const BUILD: &str = "0.3.0+ab12cd34ef56";

#[test]
fn the_runtime_dir_comes_first_and_names_files_for_the_build() {
    let paths = Paths::new(
        BUILD,
        Some(Path::new("/run/user/1000")),
        Some(Path::new("/home/me")),
    )
    .unwrap();
    assert_eq!(paths.dir, Path::new("/run/user/1000/tenmoku"));
    assert_eq!(
        paths.socket,
        Path::new("/run/user/1000/tenmoku/0.3.0+ab12cd34ef56.sock")
    );
    assert_eq!(
        paths.lock,
        Path::new("/run/user/1000/tenmoku/0.3.0+ab12cd34ef56.lock")
    );
    assert_eq!(
        paths.log,
        Path::new("/run/user/1000/tenmoku/0.3.0+ab12cd34ef56.log")
    );
}

#[test]
fn without_a_runtime_dir_the_files_go_under_the_builds_cache() {
    let paths = Paths::new(BUILD, None, Some(Path::new("/home/me"))).unwrap();
    assert_eq!(
        paths.dir,
        Path::new("/home/me/.cache/tenmoku-server/0.3.0+ab12cd34ef56/run")
    );
}

#[test]
fn a_folder_too_long_for_a_socket_gives_way_to_tmp() {
    let home = format!("/home/{}", "m".repeat(80));
    let paths = Paths::new(BUILD, None, Some(Path::new(&home))).unwrap();
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    assert_eq!(paths.dir, Path::new(&format!("/tmp/tenmoku-server-{uid}")));
    assert!(paths.socket.as_os_str().len() < 104);
}

#[test]
fn a_build_that_cant_name_a_file_is_refused() {
    for build in ["", "a/b", ".."] {
        assert!(
            Paths::new(build, Some(Path::new("/run")), None).is_err(),
            "{build:?}"
        );
    }
}

#[test]
fn neither_a_runtime_dir_nor_a_home_is_an_error() {
    assert!(Paths::new(BUILD, None, None).is_err());
}
