//! `tenmoku --version`, which each release job's smoke step runs on the
//! binary it ships (.github/workflows/release.yml): it prints the Cargo
//! version on stdout and exits 0, before any window or IPC.

use std::process::Command;

fn tenmoku(arg: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tenmoku"))
        .arg(arg)
        .env("TENMOKU_NO_IPC", "1")
        .output()
        .unwrap()
}

#[test]
fn version_prints_the_cargo_version_and_exits_0() {
    for flag in ["--version", "-V"] {
        let out = tenmoku(flag);
        assert!(out.status.success(), "{flag}: {:?}", out.status);
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            format!("tenmoku {}\n", env!("CARGO_PKG_VERSION")),
            "{flag}"
        );
    }
}
