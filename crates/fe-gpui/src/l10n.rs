// SPDX-License-Identifier: Apache-2.0

//! The editor view's catalogue (kiln-l10n), `docs/decisions/xte5e7hqsw-i18n-design.md`
//! § 3. Its commands' titles are `command-<id>` messages.

kiln_l10n::resources! {
    dynamic_prefixes = ["command-"];
    "en-US" => ["../l10n/en-US/fe-gpui.ftl", "../l10n/en-US/diff.ftl", "../l10n/en-US/hover.ftl"],
}

/// The title fe-gpui's command `id` registers with, resolved now: its
/// `command-<id>` message ([`fe_command::l10n::command_key`]), which
/// `label()` resolves again in the active locale wherever it's shown.
pub(crate) fn command_title(id: &str) -> kiln_l10n::SharedString {
    kiln_l10n::message(&fe_command::l10n::command_key(id))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    /// Every command fe-gpui registers has a `command-<id>` title in its
    /// catalogue, and every `command-*` key there names one: those of the
    /// editor's list that aren't fe-editor's or fe-vim's (the editor's
    /// own, the image viewer's and the diff view's), vim's app commands
    /// and the proposals'.
    #[gpui::test]
    fn commands_and_command_keys_match(cx: &mut gpui::TestAppContext) {
        let of = |meta: &fe_command::CommandMeta| meta.id.to_string();
        let theirs: BTreeSet<String> = (fe_editor::commands::registry().commands().map(of))
            .chain(fe_vim::registry().commands().map(of))
            .collect();
        let mut ids: BTreeSet<String> = (crate::commands::command_list().iter())
            .map(of)
            .filter(|id| !theirs.contains(id))
            .collect();
        ids.extend(cx.update(|cx| {
            crate::init(cx);
            (kiln_platform::commands::app_command_list(cx).iter())
                .map(of)
                .filter(|id| id.starts_with("vim."))
                .collect::<Vec<_>>()
        }));
        ids.extend(crate::view::proposal_registry().commands().map(of));
        assert_eq!(ids.len(), 29, "{ids:?}");
        let errors = fe_command::l10n::check_command_keys(&super::RESOURCES, ids);
        assert!(errors.is_empty(), "{}", errors.join("\n"));
    }
}
