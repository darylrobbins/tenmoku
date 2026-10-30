//! [`SettingKind::Choice`]: the settings whose values only the host knows,
//! the themes and fonts installed, or a list picks from, the Claude models,
//! by key, their schema left a string's.

use fe_config::{ChoiceSource, SettingKind, SettingsModel, settings_schema};
use kiln_core::theme::Appearance;
use serde_json::json;

/// The kind of `model`'s row for dotted `key`.
fn kind(model: &SettingsModel, key: &str) -> SettingKind {
    let entry = model.entries().iter().find(|entry| entry.key_path() == key);
    entry
        .unwrap_or_else(|| panic!("no row for {key}"))
        .kind
        .clone()
}

#[test]
fn theme_keys_are_choices() {
    let model = SettingsModel::new(None, None);
    let kind = |key: &str| kind(&model, key);
    let themes = |appearance| SettingKind::Choice(ChoiceSource::Themes { appearance });
    assert_eq!(kind("theme.dark"), themes(Some(Appearance::Dark)));
    assert_eq!(kind("theme.light"), themes(Some(Appearance::Light)));
    assert_eq!(kind("theme.name"), themes(None));
    assert!(matches!(kind("theme.mode"), SettingKind::Enum(_)));

    // Those three, the model and the fonts only: every other string is
    // still a text box.
    let choices: Vec<String> = (model.entries().iter())
        .filter(|entry| matches!(entry.kind, SettingKind::Choice(_)))
        .map(|entry| entry.key_path())
        .collect();
    assert_eq!(
        choices,
        [
            "chat.model",
            "editor.font_family",
            "terminal.font_family",
            "theme.dark",
            "theme.light",
            "theme.name",
        ]
    );
    assert_eq!(kind("vim.leader"), SettingKind::Text { min_length: None });

    // The schema doesn't say so: they're strings there, as before.
    let schema = settings_schema();
    let theme = &schema["$defs"]["ThemeSettings"]["properties"];
    for name in ["dark", "light", "name"] {
        assert_eq!(theme[name]["type"], json!(["string", "null"]), "{name}");
        assert_eq!(theme[name]["minLength"], json!(1), "{name}");
    }
}

#[test]
fn model_and_fonts_are_choices() {
    let model = SettingsModel::new(None, None);
    assert_eq!(
        kind(&model, "chat.model"),
        SettingKind::Choice(ChoiceSource::ChatModels)
    );

    // Each font_family the schema has, wherever it is.
    let fonts: Vec<String> = (model.entries().iter())
        .filter(|entry| entry.key.last().is_some_and(|name| name == "font_family"))
        .map(|entry| entry.key_path())
        .collect();
    assert_eq!(fonts, ["editor.font_family", "terminal.font_family"]);
    for key in &fonts {
        let fonts = SettingKind::Choice(ChoiceSource::Fonts);
        assert_eq!(kind(&model, key), fonts, "{key}");
    }

    // Still strings in the schema: the model any id, a font any name.
    let schema = settings_schema();
    let chat = &schema["$defs"]["ChatSettings"]["properties"]["model"];
    assert_eq!(chat["type"], json!("string"));
    assert_eq!(chat["minLength"], json!(1));
    let editor = &schema["$defs"]["EditorSettings"]["properties"]["font_family"];
    assert_eq!(editor["type"], json!(["string", "null"]));
}
