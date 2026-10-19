//! The `chat` settings: defaults, ranges, and a project file unable to set
//! them.

use fe_config::{ChatEffort, PROJECT_CHAT, Settings, SettingsStore};

fn load(user: &str, project: Option<&str>) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user_path = dir.path().join("settings.toml");
    std::fs::write(&user_path, user).unwrap();
    let project_path = project.map(|contents| {
        let path = dir.path().join("project.toml");
        std::fs::write(&path, contents).unwrap();
        path
    });
    SettingsStore::load(Some(user_path), project_path)
}

#[test]
fn chat_defaults_match_the_record() {
    let chat = Settings::default().chat;
    assert_eq!(chat.model, "claude-opus-5-5");
    assert_eq!(chat.effort, ChatEffort::Medium);
    assert!(!chat.show_thinking);
    assert_eq!(chat.max_tokens, 64_000);
    assert_eq!(chat.context_budget, 32_000);
    assert!(chat.fallbacks);

    let store = load(
        "[chat]\nmodel = \"claude-sonnet-5-5\"\neffort = \"xhigh\"\nshow_thinking = true\nmax_tokens = 2048\ncontext_budget = 0\nfallbacks = false\n",
        None,
    );
    assert_eq!(store.diagnostics(), []);
    let chat = &store.settings().chat;
    assert_eq!(chat.model, "claude-sonnet-5-5");
    assert_eq!(chat.effort, ChatEffort::Xhigh);
    assert!(chat.show_thinking);
    assert_eq!(chat.max_tokens, 2048);
    assert_eq!(chat.context_budget, 0);
    assert!(!chat.fallbacks);
}

#[test]
fn chat_values_out_of_range_are_diagnostics() {
    for (source, key) in [
        ("[chat]\neffort = \"none\"\n", "chat.effort"),
        ("[chat]\nmax_tokens = 100\n", "chat.max_tokens"),
        ("[chat]\ncontext_budget = -1\n", "chat.context_budget"),
        ("[chat]\nmodel = \"\"\n", "chat.model"),
    ] {
        let store = load(source, None);
        let diagnostics = store.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:#?}");
        assert!(diagnostics[0].is_error(), "{:?}", diagnostics[0]);
        assert_eq!(diagnostics[0].key.as_deref(), Some(key), "{source}");
        assert_eq!(store.settings().chat, Settings::default().chat, "{source}");
    }
}

#[test]
fn a_project_cannot_set_chat() {
    let store = load(
        "[chat]\nmodel = \"claude-sonnet-5-5\"\nfallbacks = false\n",
        Some("[chat]\nmodel = \"claude-haiku-4-5-20251001\"\nfallbacks = true\n"),
    );
    let chat = &store.settings().chat;
    assert_eq!(chat.model, "claude-sonnet-5-5");
    assert!(!chat.fallbacks);
    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(!diagnostics[0].is_error(), "{:?}", diagnostics[0]);
    assert_eq!(diagnostics[0].key.as_deref(), Some("chat"));
    assert_eq!(diagnostics[0].message, PROJECT_CHAT);
    assert!(diagnostics[0].message.contains("chat"));
}
