//! [`SettingKind::Choice`]: the settings whose values only the host knows,
//! the themes installed, by key, their schema left a string's.

use fe_config::{ChoiceSource, SettingKind, SettingsModel, settings_schema};
use kiln_core::theme::Appearance;
use serde_json::json;

#[test]
fn theme_keys_are_choices() {
    let model = SettingsModel::new(None, None);
    let kind = |key: &str| {
        let entry = model.entries().iter().find(|entry| entry.key_path() == key);
        entry
            .unwrap_or_else(|| panic!("no row for {key}"))
            .kind
            .clone()
    };
    let themes = |appearance| SettingKind::Choice(ChoiceSource::Themes { appearance });
    assert_eq!(kind("theme.dark"), themes(Some(Appearance::Dark)));
    assert_eq!(kind("theme.light"), themes(Some(Appearance::Light)));
    assert_eq!(kind("theme.name"), themes(None));
    assert!(matches!(kind("theme.mode"), SettingKind::Enum(_)));

    // Those three only: every other string is still a text box.
    let choices: Vec<String> = (model.entries().iter())
        .filter(|entry| matches!(entry.kind, SettingKind::Choice(_)))
        .map(|entry| entry.key_path())
        .collect();
    assert_eq!(choices, ["theme.dark", "theme.light", "theme.name"]);
    assert_eq!(
        kind("chat.model"),
        SettingKind::Text {
            min_length: Some(1)
        }
    );

    // The schema doesn't say so: they're strings there, as before.
    let schema = settings_schema();
    let theme = &schema["$defs"]["ThemeSettings"]["properties"];
    for name in ["dark", "light", "name"] {
        assert_eq!(theme[name]["type"], json!(["string", "null"]), "{name}");
        assert_eq!(theme[name]["minLength"], json!(1), "{name}");
    }
}
