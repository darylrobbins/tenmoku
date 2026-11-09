mod bracket_pair_colors_default;
mod layers;
mod locale;
mod lsp;
mod settings;
mod template_theme_names;
mod watch;

use std::{fs, path::PathBuf};

/// Writes `text` to `name` in `dir`, returning its path.
fn write(dir: &tempfile::TempDir, name: &str, text: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, text).unwrap();
    path
}
