//! fe-vim's catalogue files (`docs/decisions/xte5e7hqsw-i18n-design.md` § 3).
//! Other crates don't use these keys: they call [`crate::messages`].

kiln_l10n::resources! {
    "en-US" => ["../l10n/en-US/vim.ftl", "../l10n/en-US/commands.ftl"],
}

use kiln_l10n::{Args, SharedString};

/// `command-vim-move` for `vim.move`: the key kiln-command's resolver gives
/// a command's title (the id's `.` as `-`), whose `.description` attribute
/// is its description.
pub(crate) fn command_key(id: &str) -> String {
    let mut key = String::from("command-");
    key.push_str(&id.replace('.', "-"));
    key
}

/// The title vim's command `id` registers with, from `commands.ftl`.
pub(crate) fn command_title(id: &str) -> SharedString {
    kiln_l10n::message(&command_key(id))
}

/// The description vim's command `id` registers with: its title's
/// `.description` (none if it has none: the test in `tests/l10n.rs` checks
/// every command has one).
pub(crate) fn command_description(id: &str) -> SharedString {
    kiln_l10n::attribute(&command_key(id), "description", &Args::new()).unwrap_or_default()
}
