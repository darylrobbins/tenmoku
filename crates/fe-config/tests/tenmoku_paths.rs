//! Tenmoku's user and project paths.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use fe_config::paths::{app_id, app_paths, config_dir_from, project_settings_path, state_dir_from};

#[test]
fn app_id_is_tenmoku() {
    let id = app_id();
    assert_eq!(id.reverse_dns, "dev.tenmoku.tenmoku");
    assert_eq!(id.short, "tenmoku");
    assert_eq!(id.display, "Tenmoku");
    let root = Path::new("/work/p");
    assert_eq!(
        project_settings_path(root),
        root.join(".tenmoku/settings.toml")
    );
    assert_eq!(
        app_paths().project_extensions_dir(root),
        root.join(".tenmoku/extensions")
    );
}

#[test]
fn fastedit_dir_variables_are_ignored() {
    let env = |name: &str| -> Option<OsString> {
        match name {
            concat!("FASTEDIT_", "CONFIG_DIR") => Some("/old/config".into()),
            concat!("FASTEDIT_", "STATE_DIR") => Some("/old/state".into()),
            "HOME" => Some("/h".into()),
            _ => None,
        }
    };
    assert_eq!(
        config_dir_from(env),
        Some(PathBuf::from("/h/.config/tenmoku"))
    );
    assert_eq!(
        state_dir_from(env),
        Some(PathBuf::from("/h/.local/state/tenmoku"))
    );
}
