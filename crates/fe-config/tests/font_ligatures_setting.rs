use fe_config::{EditorSettings, Settings, USER_SETTINGS_TEMPLATE};

#[test]
fn ligatures_are_off_by_default() {
    assert!(!EditorSettings::default().font_ligatures);
}

#[test]
fn font_ligatures_parses() {
    let settings: Settings = toml::from_str("[editor]\nfont_ligatures = true\n").unwrap();
    assert!(settings.editor.font_ligatures);
}

#[test]
fn the_template_documents_it() {
    assert!(USER_SETTINGS_TEMPLATE.contains("# font_ligatures = false"));
}
