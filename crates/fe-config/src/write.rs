//! Writing settings into fastedit's `settings.toml`: kiln-core's
//! [`kiln_core::write_setting`] and its kin, creating a missing file from
//! fastedit's template. The Settings editor writes through
//! [`crate::settings_edit`], which also writes the project's file.

use std::{io, path::Path};

pub use kiln_core::write::SettingValue;

use crate::user_settings_template;

/// Sets the boolean at `key` (`["vim", "enabled"]`) in the settings file at
/// `path`, keeping everything else in it; a missing or empty file starts
/// from [`user_settings_template`], in the active locale. See [`kiln_core::write_setting`].
pub fn write_setting(path: &Path, key: &[&str], value: bool) -> io::Result<()> {
    kiln_core::write_setting(path, key, value, user_settings_template())
}

/// Sets the string at `key` (`["theme", "name"]`) in the settings file at
/// `path`, as [`write_setting`] does a boolean. See
/// [`kiln_core::write::write_setting_str`].
pub fn write_setting_str(path: &Path, key: &[&str], value: &str) -> io::Result<()> {
    kiln_core::write::write_setting_str(path, key, value, user_settings_template())
}

/// Makes each of `changes` in turn, in one write, to the settings file at
/// `path`: sets its key's value (a boolean, string, integer, float or list
/// of strings), or, for `None`, removes the key. See
/// [`kiln_core::write::write_settings`].
pub fn write_settings(
    path: &Path,
    changes: &[(&[&str], Option<SettingValue<'_>>)],
) -> io::Result<()> {
    kiln_core::write::write_settings(path, changes, user_settings_template())
}
