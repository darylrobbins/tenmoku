//! A fake program's `.exe` on Windows, for the Runs tests
//! (src/tests/test_runner.rs's `install_fake`): a bare `cargo` is run there
//! only as `cargo.exe`, so this, copied to `<bin>/cargo.exe`, runs the fake
//! `cargo` script beside it (`<bin>/cargo`, the one the other platforms run)
//! with the `sh` on `PATH`, passing its arguments and exit code through.
//!
//! The script sees its path with `/`s, as it joins names to it, and its
//! folder in `FAKE_CARGO_PWD`, as Windows writes it (with `/`s): its `pwd`
//! is the MSYS form (`/c/…`), which the workbench can't read as a path.

use std::{env, path::Path, process};

fn main() {
    // Without a verbatim `\\?\`, which sh can't read.
    let slashed = |path: &Path| {
        let path = path.to_string_lossy().replace('\\', "/");
        path.strip_prefix("//?/").unwrap_or(&path).to_owned()
    };
    let exe = env::current_exe().expect("the shim's own path");
    let script = exe.with_extension("");
    let cwd = env::current_dir().expect("the folder it runs in");
    let status = process::Command::new("sh")
        .arg(slashed(&script))
        .args(env::args_os().skip(1))
        .env("FAKE_CARGO_PWD", slashed(&cwd))
        .status()
        .expect("sh to run the fake script");
    process::exit(status.code().unwrap_or(1));
}
