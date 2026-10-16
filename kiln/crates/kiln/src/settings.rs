// SPDX-License-Identifier: LicenseRef-Proprietary

//! [`SettingsSection`]: a settings type that is one table of the app's
//! `settings.toml`, as `#[kiln::settings(section = "..")]` makes one.

use kiln_core::{Diagnostic, SettingsOptions, SettingsStore, SettingsType};
use kiln_platform::config::ConfigService;
use schemars::JsonSchema;
use serde_json::Value as Json;

/// A settings type that is the table [`SettingsSection::SECTION`] of the
/// app's one settings file ([`crate::Config::settings_file`]): `[notes]`.
/// Each section reads its own table, checked against its schema
/// ([`schema`]), and a key in another section's table isn't unknown to
/// it. [`crate::Config::section`] adds one; `#[kiln::settings]`
/// ([`crate::settings`]) implements it.
pub trait SettingsSection: SettingsType + JsonSchema {
    /// The table's name: `notes` for `[notes]`.
    const SECTION: &'static str;
}

/// `S`'s JSON schema, as schemars writes it, its doc lines the keys'
/// descriptions: what a section's table is checked against.
pub fn schema<S: JsonSchema>() -> Json {
    schemars::schema_for!(S).to_value()
}

/// The store of the section `S` of `file`, which the sections `all`
/// share, added to `service`, and its diagnostics.
pub(crate) fn section_store<S: SettingsSection>(
    service: ConfigService,
    file: Option<std::path::PathBuf>,
    all: &[&'static str],
) -> (ConfigService, Vec<Diagnostic>) {
    let options = SettingsOptions::new()
        .schema(schema::<S>())
        .section(S::SECTION, all.iter().copied());
    let store = SettingsStore::<S>::load_with(file, None, options);
    let diagnostics = store.diagnostics().to_vec();
    (service.settings(store), diagnostics)
}
