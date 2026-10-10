//! Writing one setting into fastedit's `settings.toml`: kiln-core's
//! [`kiln_core::write_setting`], creating a missing file from fastedit's
//! template.

use std::{io, path::Path};

use crate::USER_SETTINGS_TEMPLATE;

/// Sets the boolean at `key` (`["vim", "enabled"]`) in the settings file at
/// `path`, keeping everything else in it; a missing or empty file starts
/// from [`USER_SETTINGS_TEMPLATE`]. See [`kiln_core::write_setting`].
pub fn write_setting(path: &Path, key: &[&str], value: bool) -> io::Result<()> {
    kiln_core::write_setting(path, key, value, USER_SETTINGS_TEMPLATE)
}
