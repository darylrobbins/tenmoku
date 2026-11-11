// SPDX-License-Identifier: Apache-2.0

//! The programs the tests run, made the same way on every OS: scripts, which are `#!/bin/sh`
//! files made executable on Unix and `.cmd` files on Windows, and copies of the system's own
//! programs, from `/usr/bin` and `/bin` on Unix and from `System32` on Windows.
//!
//! Each test file that uses it declares it with `#[path]`, the crate's unit tests among them,
//! and no one file uses all of it.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

/// Writes the script `name` into `dir`, over any file of that name, and returns its path. On
/// Unix it is `dir/name`: `sh` after a `#!/bin/sh` line, made executable. On Windows it is
/// `dir/name.cmd`: `cmd` after `@echo off`, with Windows' line ends; `PATHEXT` makes it a
/// program, run through `cmd.exe`.
pub(crate) fn script(dir: &Path, name: &str, sh: &str, cmd: &str) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = cmd;
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{sh}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
    #[cfg(windows)]
    {
        let _ = sh;
        let path = dir.join(format!("{name}.cmd"));
        let body: String = cmd.lines().map(|line| format!("{line}\r\n")).collect();
        fs::write(&path, format!("@echo off\r\n{body}")).unwrap();
        path
    }
}

/// Copies the system's program `name` into `dir`, under the file name it has there, and returns
/// the copy's path. It is the first found on the `PATH` a spawned program gets
/// ([`kiln_core::platform::system_path`]): `/usr/bin` or `/bin` on Unix, and on Windows
/// `System32` or the folders beside it, where `name` is found with an extension of `PATHEXT`'s.
pub(crate) fn system_program(dir: &Path, name: &str) -> PathBuf {
    let system_path = kiln_core::platform::system_path();
    let system = std::env::split_paths(&system_path)
        .flat_map(|system| kiln_core::platform::program_candidates(&system, name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("no {name} in {}", system_path.display()));
    let copy = dir.join(system.file_name().unwrap());
    place(&system, &copy);
    copy
}

#[cfg(not(any(target_os = "macos", windows)))]
fn place(system: &Path, copy: &Path) {
    fs::copy(system, copy).unwrap();
}

/// macOS kills (SIGKILL at exec) a copied system binary: its signature is the platform's, which
/// binds it to `/usr/bin` and `/bin`. Re-signing it ad hoc isn't enough everywhere: on
/// macOS 15 (GitHub's runner) the copy is still killed, likely because its arm64e slice runs only
/// under the platform's signature. So the copy is a program of our own, built by `cc` (which links every
/// Rust program here): it execs the system binary in place, keeping its pid, arguments and
/// environment as the call gave them.
#[cfg(target_os = "macos")]
fn place(system: &Path, copy: &Path) {
    let source = copy.with_extension("c");
    fs::write(
        &source,
        format!(
            "#include <unistd.h>\n\
             int main(int argc, char **argv) {{\n\
             \x20   (void)argc;\n\
             \x20   execv(\"{}\", argv);\n\
             \x20   return 127;\n\
             }}\n",
            system.display()
        ),
    )
    .unwrap();
    let output = std::process::Command::new("cc")
        .arg("-o")
        .arg(copy)
        .arg(&source)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    fs::remove_file(&source).unwrap();
}

/// A `System32` program keeps its messages apart, in `<language>\<file>.mui` beside it, and
/// looks for them beside itself: each language's is copied with it.
#[cfg(windows)]
fn place(system: &Path, copy: &Path) {
    fs::copy(system, copy).unwrap();
    let mut resources = system.file_name().unwrap().to_owned();
    resources.push(".mui");
    for entry in fs::read_dir(system.parent().unwrap()).unwrap() {
        let entry = entry.unwrap();
        let found = entry.path().join(&resources);
        if found.is_file() {
            let language = copy.parent().unwrap().join(entry.file_name());
            fs::create_dir_all(&language).unwrap();
            fs::copy(&found, language.join(&resources)).unwrap();
        }
    }
}
