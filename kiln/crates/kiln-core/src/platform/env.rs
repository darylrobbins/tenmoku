// SPDX-License-Identifier: Apache-2.0

//! The environment a program spawned for a plugin gets: one rule set per OS, each a plain
//! function of the environment it is given, so any of them can be tested on any host.

use std::ffi::{OsStr, OsString};

use kiln_l10n::verbatim;

use super::Os;

/// The `PATH` a spawned program gets on Unix: the system's own directories, not the app's.
pub(crate) const UNIX_PATH: &str = "/usr/bin:/bin";

/// What Windows programs need beyond `PATH`, as Windows spells them.
const WINDOWS_PASSED: [&str; 12] = [
    verbatim!("SystemRoot"),
    "windir",
    verbatim!("SystemDrive"),
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
    "PATHEXT",
    verbatim!("ProgramData"),
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
];

/// The `PATH` a spawned program gets on this OS.
pub fn system_path() -> OsString {
    system_path_for(Os::current(), std::env::vars_os())
}

/// The `PATH` a spawned program gets on `os`, given the app's environment (Windows names its
/// directories under `SystemRoot`, `C:\Windows` when the app has none).
pub fn system_path_for(os: Os, os_env: impl IntoIterator<Item = (OsString, OsString)>) -> OsString {
    match os {
        Os::Windows => windows_path(&system_root(os_env)),
        Os::MacOs | Os::Linux => UNIX_PATH.into(),
    }
}

/// The environment a spawned program gets out of the app's `os_env`, by this OS's rules.
pub fn minimal_environment(
    os_env: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    minimal_environment_for(Os::current(), os_env)
}

/// The environment a program spawned on `os` gets out of the app's `os_env`.
pub fn minimal_environment_for(
    os: Os,
    os_env: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    match os {
        Os::Windows => windows_environment(os_env),
        Os::MacOs | Os::Linux => unix_environment(os_env),
    }
}

/// Unix: `PATH` set to [`UNIX_PATH`], and `HOME`, `LANG`, `LC_*`, `TMPDIR` and `TERM` as the
/// app has them.
pub fn unix_environment(
    os_env: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    let passed = |name: &OsString| {
        name.to_str().is_some_and(|name| {
            matches!(name, "HOME" | "LANG" | "TMPDIR" | "TERM") || name.starts_with("LC_")
        })
    };
    let mut minimal = vec![("PATH".into(), UNIX_PATH.into())];
    minimal.extend(os_env.into_iter().filter(|(name, _)| passed(name)));
    minimal
}

/// Windows: `PATH` set to the `System32` form, and `SystemRoot`, `windir`, `SystemDrive`,
/// `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `TEMP`, `TMP`, `PATHEXT`, `ProgramData`,
/// `NUMBER_OF_PROCESSORS` and `PROCESSOR_ARCHITECTURE` as the app has them. Names match
/// without regard to case (`Path` is `PATH`) and are passed as written above.
pub fn windows_environment(
    os_env: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    let os_env: Vec<_> = os_env.into_iter().collect();
    let mut minimal = vec![(
        "PATH".into(),
        windows_path(&system_root(os_env.iter().cloned())),
    )];
    for wanted in WINDOWS_PASSED {
        let found = os_env.iter().find(|(name, _)| {
            name.to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(wanted))
        });
        if let Some((_, value)) = found {
            minimal.push((wanted.into(), value.clone()));
        }
    }
    minimal
}

fn system_root(os_env: impl IntoIterator<Item = (OsString, OsString)>) -> OsString {
    os_env
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(OsStr::new("SystemRoot")))
        .map_or_else(|| r"C:\Windows".into(), |(_, value)| value)
}

fn windows_path(root: &OsStr) -> OsString {
    let mut path = OsString::new();
    for (i, tail) in [r"\System32", "", r"\System32\Wbem"]
        .into_iter()
        .enumerate()
    {
        if i > 0 {
            path.push(";");
        }
        path.push(root);
        path.push(tail);
    }
    path
}
