// SPDX-License-Identifier: Apache-2.0

//! The environment a spawned program gets, by each OS's rules. Each rule function is given an
//! environment, so every test runs on every host.

use std::ffi::OsString;
use std::path::Path;

use kiln_core::platform::{unix_environment, windows_environment};
use kiln_plugin::grants::interpreter_for;

fn given(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(n, v)| ((*n).into(), (*v).into()))
        .collect()
}

fn text(environment: Vec<(OsString, OsString)>) -> Vec<(String, String)> {
    environment
        .into_iter()
        .map(|(n, v)| (n.into_string().unwrap(), v.into_string().unwrap()))
        .collect()
}

#[test]
fn environment_unix_passes_path_home_lang_tmpdir_term_only() {
    let minimal = text(unix_environment(given(&[
        ("PATH", "/opt/x"),
        ("HOME", "/home/a"),
        ("LANG", "C"),
        ("LC_ALL", "C"),
        ("TMPDIR", "/t"),
        ("TERM", "xterm"),
        ("SystemRoot", r"C:\Windows"),
        ("SECRET", "x"),
    ])));
    let expect = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(n, v)| ((*n).into(), (*v).into()))
            .collect()
    };
    assert_eq!(
        minimal,
        expect(&[
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/home/a"),
            ("LANG", "C"),
            ("LC_ALL", "C"),
            ("TMPDIR", "/t"),
            ("TERM", "xterm"),
        ])
    );
}

#[test]
fn environment_windows_passes_systemroot_set_case_insensitively() {
    let minimal = text(windows_environment(given(&[
        ("Path", r"C:\evil"),
        ("SYSTEMROOT", r"C:\Windows"),
        ("WinDir", r"C:\Windows"),
        ("SystemDrive", "C:"),
        ("UserProfile", r"C:\Users\a"),
        ("AppData", "a"),
        ("LocalAppData", "l"),
        ("Temp", "t"),
        ("tmp", "t2"),
        ("pathext", ".COM;.EXE;.BAT;.CMD"),
        ("ProgramData", "p"),
        ("NUMBER_OF_PROCESSORS", "8"),
        ("PROCESSOR_ARCHITECTURE", "AMD64"),
        ("HOME", "/h"),
        ("TMPDIR", "/t"),
        ("SECRET", "x"),
    ])));
    let names: Vec<&str> = minimal.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "PATH",
            "SystemRoot",
            "windir",
            "SystemDrive",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "TEMP",
            "TMP",
            "PATHEXT",
            "ProgramData",
            "NUMBER_OF_PROCESSORS",
            "PROCESSOR_ARCHITECTURE",
        ]
    );
    assert_eq!(minimal[9].1, ".COM;.EXE;.BAT;.CMD");
    assert_ne!(minimal[0].1, r"C:\evil");
}

#[test]
fn environment_windows_path_is_system32_form() {
    let minimal = text(windows_environment(given(&[("SystemRoot", r"D:\Win")])));
    assert_eq!(
        minimal[0],
        (
            "PATH".to_owned(),
            r"D:\Win\System32;D:\Win;D:\Win\System32\Wbem".to_owned()
        )
    );
    let none = text(windows_environment(given(&[])));
    assert_eq!(
        none[0].1,
        r"C:\Windows\System32;C:\Windows;C:\Windows\System32\Wbem"
    );
}

#[test]
fn environment_bat_and_cmd_have_cmd_interpreter() {
    for name in ["a.bat", "a.cmd", "A.BAT", "b.Cmd"] {
        assert_eq!(
            interpreter_for(Path::new(name)).as_deref(),
            Some("cmd.exe"),
            "{name}"
        );
    }
    for name in ["a.exe", "a.com", "a", "a.sh"] {
        assert_eq!(interpreter_for(Path::new(name)), None, "{name}");
    }
}
