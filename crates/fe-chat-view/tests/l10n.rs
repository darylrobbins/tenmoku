//! The chat view's strings are in its catalogue: no hard-coded user-facing
//! literal in its source, and every key it uses exists.

#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &fe_chat_view::l10n::RESOURCES).check();
}

/// Every command the view registers has a `command-<id>` title in its
/// catalogue, and every `command-*` key there names one.
#[gpui::test]
fn commands_and_command_keys_match(cx: &mut gpui::TestAppContext) {
    let ids = cx.update(|cx| {
        fe_gpui::init(cx);
        fe_chat_view::init(cx);
        kiln::platform::command_actions::typed_commands(cx)
            .into_iter()
            .map(|typed| typed.id.to_string())
            .filter(|id| id.starts_with("chat."))
            .collect::<Vec<_>>()
    });
    assert_eq!(ids.len(), 3, "{ids:?}");
    let errors = kiln_command::l10n::check_command_keys(&fe_chat_view::l10n::RESOURCES, ids);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
