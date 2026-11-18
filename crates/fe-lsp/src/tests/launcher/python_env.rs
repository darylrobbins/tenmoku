//! A Python server installed into the project's virtualenv: found in
//! `.venv/bin` though it's on no `PATH` (the Linux report behind T683).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::registry::builtin_specs;
use crate::{LaunchCancel, LocalLauncher, ServerLauncher};

/// `body` as an executable `/bin/sh` script at `file`.
fn script(file: &Path, body: &str) {
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, format!("#!/bin/sh\n{body}")).unwrap();
    fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn pyright_is_found_in_the_project_venv() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    script(&root.join(".venv/bin/python"), "");
    // Shell builtins only: nothing else is on its PATH. It runs in the
    // root.
    script(
        &root.join(".venv/bin/pyright-langserver"),
        "printf '%s' \"$1\" > ran.txt\n",
    );
    let mut spec = builtin_specs()
        .into_iter()
        .find(|spec| spec.name == "pyright")
        .unwrap();
    // No PATH finds it, and Tenmoku's own VIRTUAL_ENV, if any, is unset.
    spec.env = vec![
        ("PATH".into(), "/nonexistent".into()),
        ("VIRTUAL_ENV".into(), String::new()),
    ];

    let launched = LocalLauncher.launch(&spec, &root, &LaunchCancel::new());
    assert!(launched.is_ok());
    let Ok(mut io) = launched else { return };
    async_io::block_on(io.process.exited());
    let ran = fs::read_to_string(root.join("ran.txt"));
    assert!(ran.is_ok_and(|args| args == "--stdio"));
}
