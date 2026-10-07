mod layers;
mod lsp;
mod settings;
mod watch;
mod watch_late;

use std::{fs, path::PathBuf};

/// Writes `text` to `name` in `dir`, returning its path.
fn write(dir: &tempfile::TempDir, name: &str, text: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, text).unwrap();
    path
}
