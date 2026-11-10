//! fe-editor's catalogue (kiln-l10n), `docs/decisions/xte5e7hqsw-i18n-design.md`
//! § 3. Its commands' titles are `command-<id>` messages, and their
//! descriptions those messages' `.description` attributes.

kiln_l10n::resources! {
    dynamic_prefixes = ["command-"];
    "en-US" => ["../l10n/en-US/fe-editor.ftl"],
}

use kiln_l10n::SharedString;

/// The title fe-editor's command `id` registers with, resolved now: its
/// `command-<id>` message ([`fe_command::l10n::command_key`]), which
/// `label()` resolves again in the active locale wherever it's shown.
pub(crate) fn command_title(id: &str) -> SharedString {
    kiln_l10n::message(&fe_command::l10n::command_key(id))
}

/// The description fe-editor's command `id` registers with, resolved now:
/// its `command-<id>` message's `.description`, which `detail()` resolves
/// again in the active locale wherever it's shown.
pub(crate) fn command_description(id: &str) -> SharedString {
    fe_command::l10n::command_detail(id, "")
}
