//! fe-config's messages, in `l10n/<locale>/*.ftl`: see `kiln_l10n`.
//!
//! - `config.ftl` holds the problems found in settings, theme and snippet
//!   files, written by hand.
//! - `settings.ftl` holds the Settings editor's text, generated from
//!   `settings.schema.json`: each setting's and table's description, each
//!   value's, and each table's title.
//! - `settings-template.ftl` holds the prose of `settings-template.toml`,
//!   generated from it: one message for each block of comment lines that
//!   isn't commented TOML ([`user_settings_template`] renders it).
//!
//! The two generated files are goldens: fe-config's tests fail when either
//! is out of date, and rewrite both with `UPDATE_GOLDEN=1`. Their keys come
//! from [`setting_key`], which the Settings editor and the template share.
//!
//! [`user_settings_template`]: crate::user_settings_template

#[cfg(test)]
mod ftl;
#[cfg(test)]
pub(crate) mod generate;
pub(crate) mod template;

kiln_l10n::resources! {
    dynamic_prefixes = ["settings-"];
    "en-US" => [
        "../l10n/en-US/config.ftl",
        "../l10n/en-US/settings.ftl",
        "../l10n/en-US/settings-template.ftl",
    ],
}

/// What a [`setting_key`] names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    /// A setting's or a table's description in the Settings editor:
    /// `settings-<path>`.
    Description,
    /// The block of prose before a setting or a table in the settings
    /// template: `settings-template-<path>`.
    Template,
    /// A table's title in the Settings editor, by its name alone:
    /// `settings-group-<name>`.
    Group,
}

/// The message key for `path`, a dotted settings path such as
/// `workbench.preview_tabs`: `settings`, then `kind`'s word, then each part
/// of the path, joined with `-`. So `(Description, "workbench.preview_tabs")`
/// is `settings-workbench-preview_tabs`, and `Template` makes it
/// `settings-template-workbench-preview_tabs`.
///
/// No two paths make one key: a schema's names are `[A-Za-z0-9_]`, which
/// the generators check, and no top-level table is named `template` or
/// `group`.
pub fn setting_key(kind: KeyKind, path: &str) -> String {
    let mut key = String::from(match kind {
        KeyKind::Description => "settings",
        KeyKind::Template => "settings-template",
        KeyKind::Group => "settings-group",
    });
    for part in path.split('.').filter(|part| !part.is_empty()) {
        key.push('-');
        key.push_str(part);
    }
    key
}

/// The attribute of a setting's message that describes its `value`: the
/// value itself, or `empty` for `""`.
pub(crate) fn variant_attribute(value: &str) -> String {
    match value {
        "" => "empty".to_owned(),
        value => value.to_owned(),
    }
}
