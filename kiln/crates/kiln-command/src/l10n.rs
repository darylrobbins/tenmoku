// SPDX-License-Identifier: Apache-2.0

//! Command titles in the active locale: the one resolver every
//! representation of a command reads its title through ([`CommandMeta`]'s
//! `label()` and `detail()`, and the typed and contributed commands of the
//! crates above), so no reader formats a key itself.
//!
//! A command's title is the message `command-<id>`, with the id's `.` as
//! `-`, in the catalogue of the crate that registers it. The title it was
//! registered with stays its English fallback: a command with no key, such
//! as a plugin's, shows that, silently.
//!
//! [`CommandMeta`]: crate::CommandMeta

use std::collections::HashMap;
use std::sync::{LazyLock, PoisonError, RwLock};

use kiln_l10n::{Args, SharedString, Text};

/// The catalogue this module resolves through, for crates that depend only
/// on kiln-command: kiln-platform's tests register a fake locale with it.
pub use kiln_l10n as catalogue;

kiln_l10n::resources! {
    "en-US" => ["../l10n/en-US/kiln-command.ftl"],
}

/// `command-git-unstage_file` for `git.unstage_file`: the id's `.` as `-`.
pub fn command_key(id: &str) -> String {
    format!("command-{}", id.replace('.', "-"))
}

/// The title to show for `id`, in the active locale, in this order:
///
/// 1. the message `command-<id>`, given the arguments every command title
///    may use ([`set_label_arg`], such as `$app`);
/// 2. the label kiln derived for `id` ([`derive_label`]), resolved now;
/// 3. `fallback`, the English title the command was registered with.
///
/// A miss is silent, with no log: plugin commands have no key by design. A
/// message that doesn't format, as one using `$app` before the app has set
/// it, counts as a miss.
pub fn command_label(id: &str, fallback: &str) -> SharedString {
    let args = LABEL_ARGS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    if let Some(found) = kiln_l10n::try_format(&command_key(id), &args)
        && found.errors.is_empty()
    {
        return found.text;
    }
    let derived = DERIVED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(id)
        .cloned();
    match derived {
        Some(label) => label.resolve(),
        None => SharedString::from(fallback.to_owned()),
    }
}

/// `command-<id>`'s `.description` attribute in the active locale, else
/// `fallback`, silently.
pub fn command_detail(id: &str, fallback: &str) -> SharedString {
    kiln_l10n::attribute(&command_key(id), "description", &Args::new())
        .unwrap_or_else(|| SharedString::from(fallback.to_owned()))
}

/// Process-wide, as the catalogue is: what [`set_label_arg`] set.
static LABEL_ARGS: LazyLock<RwLock<Args>> = LazyLock::new(RwLock::default);

/// Gives every command title the argument `name`: kiln sets `app` to the
/// app's name, for titles such as `command-app-hide = Hide { $app }`. A
/// second call for the same `name` replaces the first.
pub fn set_label_arg(name: &'static str, value: impl Into<SharedString>) {
    LABEL_ARGS
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .set(name, value.into());
}

/// Process-wide, as the catalogue is: what [`derive_label`] recorded.
static DERIVED: LazyLock<RwLock<HashMap<String, Text>>> = LazyLock::new(RwLock::default);

/// Labels `id`, a command whose title is built from other text, such as a
/// panel's toggle, when it has no `command-<id>` of its own. A second call
/// for the same `id` replaces the first.
pub fn derive_label(id: &str, label: Text) {
    DERIVED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id.to_owned(), label);
}

/// The registry check, for a crate's tests (the `lint` feature): every
/// command in `ids` has a `command-<id>` message in `resources`' en-US files,
/// and every `command-*` message there names one of `ids`. The errors, one
/// a line; none when they match.
///
/// ```ignore
/// let ids = registry().commands().map(|meta| meta.id.to_string());
/// let errors = kiln_command::l10n::check_command_keys(&my_crate::l10n::RESOURCES, ids);
/// assert!(errors.is_empty(), "{}", errors.join("\n"));
/// ```
#[cfg(feature = "lint")]
pub fn check_command_keys<S: AsRef<str>>(
    resources: &kiln_l10n::Resources,
    ids: impl IntoIterator<Item = S>,
) -> Vec<String> {
    use std::collections::BTreeSet;

    use fluent_syntax::ast::Entry;

    let mut keys = BTreeSet::new();
    for file in resources
        .files
        .iter()
        .filter(|file| file.locale == kiln_l10n::FALLBACK_LOCALE)
    {
        let resource = match fluent_syntax::parser::parse(file.source) {
            Ok(resource) | Err((resource, _)) => resource,
        };
        for entry in &resource.body {
            if let Entry::Message(message) = entry
                && message.id.name.starts_with("command-")
            {
                keys.insert(message.id.name.to_string());
            }
        }
    }
    let crate_name = resources.crate_name;
    let mut errors = Vec::new();
    let mut wanted = BTreeSet::new();
    for id in ids {
        let id = id.as_ref();
        let key = command_key(id);
        if !keys.contains(&key) {
            errors.push(format!(
                "the command {id} has no `{key}` in {crate_name}'s en-US resources"
            ));
        }
        wanted.insert(key);
    }
    for key in keys.difference(&wanted) {
        errors.push(format!(
            "`{key}` in {crate_name}'s en-US resources names no command of {crate_name}"
        ));
    }
    errors
}
