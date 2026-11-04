//! The setup wizard's model: what each page asks, what it shows from the
//! settings there are, and what each answer does to a real `settings.toml`.

use std::path::{Path, PathBuf};

use fe_config::{
    SettingValue, Settings, SettingsStore, ThemeModeSetting,
    settings::{AutoSave, InlineDiagnostics, LineNumbersSetting},
    write_settings,
};

use crate::setup_wizard_model::{Choices, Control, ControlId, Step, Write, steps};

const FONTS: &[&str] = &["Fira Code", "Menlo"];

fn all_controls(choices: &Choices) -> Vec<Control> {
    (steps().into_iter())
        .flat_map(|step| step.controls(choices))
        .collect()
}

fn control(choices: &Choices, id: ControlId) -> Control {
    (all_controls(choices).into_iter())
        .find(|control| control.id == id)
        .unwrap_or_else(|| panic!("{id:?} isn't listed"))
}

/// The label of what `id` shows.
fn shown(choices: &Choices, id: ControlId) -> String {
    let control = control(choices, id);
    control.options[control.selected].label.clone()
}

fn labels(choices: &Choices, id: ControlId) -> Vec<String> {
    (control(choices, id).options.into_iter())
        .map(|option| option.label)
        .collect()
}

/// Chooses the option of `id` labelled `label`.
fn pick<'a>(choices: &'a mut Choices, id: ControlId, label: &str) -> Vec<Write<'a>> {
    let at = (control(choices, id).options.iter())
        .position(|option| option.label == label)
        .unwrap_or_else(|| panic!("{id:?} has no {label}"));
    choices.choose(id, at)
}

/// Chooses as [`pick`] does, and writes that to `path`.
fn pick_into(path: &Path, choices: &mut Choices, id: ControlId, label: &str) {
    write_settings(path, &pick(choices, id, label)).unwrap();
}

fn settings_file(text: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, text).unwrap();
    (dir, path)
}

fn table(path: &Path) -> toml::Table {
    std::fs::read_to_string(path).unwrap().parse().unwrap()
}

fn in_file(path: &Path, key: &[&str]) -> Option<toml::Value> {
    let (name, tables) = key.split_last().unwrap();
    let mut table = table(path);
    for name in tables {
        table = table.get(*name)?.as_table()?.clone();
    }
    table.get(*name).cloned()
}

/// Loads the file as the app does, finding nothing wrong with it.
fn load_cleanly(path: &Path) -> Settings {
    let store = SettingsStore::load(Some(path.to_owned()), None);
    assert_eq!(store.diagnostics(), []);
    Settings::clone(store.settings())
}

fn ids(step: Step, choices: &Choices) -> Vec<ControlId> {
    (step.controls(choices).into_iter())
        .map(|control| control.id)
        .collect()
}

#[test]
fn steps_in_order() {
    assert_eq!(
        steps(),
        [
            Step::Appearance,
            Step::Font,
            Step::Keyboard,
            Step::Editing,
            Step::Ready
        ]
    );
    let mut choices = Choices::from_settings(&Settings::default(), FONTS);
    assert_eq!(Step::Ready.controls(&choices), []);
    assert_eq!(
        ids(Step::Font, &choices),
        [
            ControlId::FontFamily,
            ControlId::FontSize,
            ControlId::FontLigatures
        ]
    );
    assert_eq!(
        ids(Step::Editing, &choices),
        [
            ControlId::FormatOnSave,
            ControlId::AutoSave,
            ControlId::InlineDiagnostics,
            ControlId::Minimap,
            ControlId::InlayHints,
            ControlId::SoftWrap,
        ]
    );

    assert_eq!(ids(Step::Keyboard, &choices), [ControlId::Keyboard]);
    pick(&mut choices, ControlId::Keyboard, "Vim");
    assert_eq!(
        ids(Step::Keyboard, &choices),
        [
            ControlId::Keyboard,
            ControlId::VimPreset,
            ControlId::LineNumbers
        ]
    );
    pick(&mut choices, ControlId::Keyboard, "Standard");
    assert_eq!(ids(Step::Keyboard, &choices), [ControlId::Keyboard]);

    assert_eq!(
        ids(Step::Appearance, &choices),
        [ControlId::ThemeMode, ControlId::DarkTheme]
    );
    pick(&mut choices, ControlId::ThemeMode, "Light");
    assert_eq!(
        ids(Step::Appearance, &choices),
        [ControlId::ThemeMode, ControlId::LightTheme]
    );
    pick(&mut choices, ControlId::ThemeMode, "Match System");
    assert_eq!(
        ids(Step::Appearance, &choices),
        [
            ControlId::ThemeMode,
            ControlId::DarkTheme,
            ControlId::LightTheme
        ]
    );
    // A list is the bundled themes of its appearance, and the Tenmoku one
    // shows while its slot is unset.
    assert_eq!(
        labels(&choices, ControlId::LightTheme),
        ["Light 2026", "Catppuccin Latte", "Tenmoku Light"]
    );
    assert_eq!(shown(&choices, ControlId::LightTheme), "Tenmoku Light");
    assert_eq!(shown(&choices, ControlId::DarkTheme), "Tenmoku Dark");
}

#[test]
fn starts_from_current_settings() {
    let mut settings = Settings::default();
    settings.theme.name = Some("Ember".to_owned());
    settings.vim.enabled = true;
    settings.editor.font_size = 15.0;
    settings.editor.auto_save = AutoSave::OnWindowChange;
    let choices = Choices::from_settings(&settings, FONTS);

    assert_eq!(shown(&choices, ControlId::DarkTheme), "Tenmoku Ember");
    assert_eq!(shown(&choices, ControlId::Keyboard), "Vim");
    assert_eq!(shown(&choices, ControlId::FontSize), "15");
    assert_eq!(shown(&choices, ControlId::AutoSave), "Custom");
    assert_eq!(shown(&choices, ControlId::FontFamily), "Default (Menlo)");
    assert_eq!(choices.changed_count(), 0);
}

/// Settings a hand-edited file might hold, with the wizard's curated lists
/// offering none of them.
fn beyond_the_lists() -> Settings {
    let mut settings = Settings::default();
    settings.theme.mode = ThemeModeSetting::System;
    settings.theme.dark = Some("acme.solarized/solarized-dark".to_owned());
    settings.theme.name = Some("Catppuccin Latte".to_owned());
    settings.vim.enabled = true;
    settings.editor.font_family = Some("Not Installed".to_owned());
    settings.editor.font_size = 30.0;
    settings.editor.auto_save = AutoSave::OnInsertLeave;
    settings.editor.inline_diagnostics = InlineDiagnostics::All;
    settings.editor.line_numbers = LineNumbersSetting::Off;
    settings
}

#[test]
fn unchanged_choices_write_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("settings.toml");
    for settings in [Settings::default(), beyond_the_lists()] {
        let mut choices = Choices::from_settings(&settings, FONTS);
        for control in all_controls(&choices) {
            let writes = choices.choose(control.id, control.selected);
            assert_eq!(writes, [], "{}", control.dotted_key());
            write_settings(&missing, &writes).unwrap();
        }
        assert_eq!(choices.changed_count(), 0);
    }
    assert!(!missing.exists());
}

#[test]
fn changed_choices_write_only_their_keys() {
    let (_dir, path) = settings_file("# Mine, hands off.\n[editor]\n# Two.\ntab_size = 2\n");
    let mut choices = Choices::from_settings(&load_cleanly(&path), FONTS);

    pick_into(&path, &mut choices, ControlId::ThemeMode, "Light");
    pick_into(
        &path,
        &mut choices,
        ControlId::LightTheme,
        "Catppuccin Latte",
    );
    pick_into(&path, &mut choices, ControlId::Keyboard, "Vim");
    pick_into(&path, &mut choices, ControlId::FontSize, "15");

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# Mine, hands off.\n"), "{text}");
    assert!(text.contains("# Two.\ntab_size = 2\n"), "{text}");
    let expected: toml::Table = r#"
        [editor]
        tab_size = 2
        font_size = 15.0
        [theme]
        mode = "light"
        light = "builtin/catppuccin-latte"
        [vim]
        enabled = true
    "#
    .parse()
    .unwrap();
    assert_eq!(table(&path), expected);

    let loaded = load_cleanly(&path);
    assert_eq!(loaded.editor.font_size, 15.0);
    assert_eq!(loaded.theme.mode, ThemeModeSetting::Light);
    assert_eq!(choices.changed_count(), 4);
    assert!(choices.summary().starts_with("4 settings changed."));
}

#[test]
fn back_to_default_removes_the_key() {
    let (_dir, path) = settings_file("# Mine.\n");
    let mut choices = Choices::from_settings(&Settings::default(), FONTS);

    pick_into(&path, &mut choices, ControlId::FontSize, "15");
    assert_eq!(
        in_file(&path, &["editor", "font_size"]),
        Some(toml::Value::Float(15.0))
    );
    let writes = pick(&mut choices, ControlId::FontSize, "13");
    assert_eq!(writes, [(&["editor", "font_size"][..], None)]);
    write_settings(&path, &writes).unwrap();
    assert_eq!(in_file(&path, &["editor", "font_size"]), None);

    pick_into(&path, &mut choices, ControlId::DarkTheme, "Dark Modern");
    assert!(in_file(&path, &["theme", "dark"]).is_some());
    pick_into(&path, &mut choices, ControlId::DarkTheme, "Tenmoku Dark");
    assert_eq!(in_file(&path, &["theme", "dark"]), None);

    pick_into(&path, &mut choices, ControlId::FontFamily, "Fira Code");
    assert!(in_file(&path, &["editor", "font_family"]).is_some());
    pick_into(
        &path,
        &mut choices,
        ControlId::FontFamily,
        "Default (Menlo)",
    );
    assert_eq!(in_file(&path, &["editor", "font_family"]), None);

    assert_eq!(choices.changed_count(), 0);
    assert!(choices.summary().starts_with("No settings changed."));
    load_cleanly(&path);
}

#[test]
fn theme_name_gives_way_to_the_slot() {
    let (_dir, path) = settings_file("[theme]\nname = \"Ember\"\n");
    let mut choices = Choices::from_settings(&load_cleanly(&path), FONTS);
    assert_eq!(shown(&choices, ControlId::DarkTheme), "Tenmoku Ember");

    let writes = pick(&mut choices, ControlId::DarkTheme, "Dark 2026");
    assert_eq!(
        writes,
        [
            (
                &["theme", "dark"][..],
                Some(SettingValue::Str("builtin/vscode-2026-dark"))
            ),
            (&["theme", "name"][..], None),
        ]
    );
    write_settings(&path, &writes).unwrap();
    assert_eq!(in_file(&path, &["theme", "name"]), None);
    let loaded = load_cleanly(&path);
    assert_eq!(
        loaded.theme.dark.as_deref(),
        Some("builtin/vscode-2026-dark")
    );
    assert_eq!(loaded.theme.name, None);
    assert_eq!(choices.changed_count(), 2);
}

#[test]
fn theme_name_only_answers_for_its_own_appearance() {
    let mut settings = Settings::default();
    settings.theme.name = Some("Ember".to_owned());
    settings.theme.mode = ThemeModeSetting::System;

    // The Tenmoku tile over a name: the slot has no key to remove.
    let mut choices = Choices::from_settings(&settings, FONTS);
    assert_eq!(
        pick(&mut choices, ControlId::DarkTheme, "Tenmoku Dark"),
        [(&["theme", "name"][..], None)]
    );
    assert_eq!(shown(&choices, ControlId::DarkTheme), "Tenmoku Dark");

    // A light theme leaves the dark one the name gives.
    let mut choices = Choices::from_settings(&settings, FONTS);
    assert_eq!(
        pick(&mut choices, ControlId::LightTheme, "Light 2026"),
        [(
            &["theme", "light"][..],
            Some(SettingValue::Str("builtin/vscode-2026-light"))
        )]
    );
    assert_eq!(shown(&choices, ControlId::DarkTheme), "Tenmoku Ember");

    // A name no bundled theme has chooses nothing, and goes with any choice.
    settings.theme.name = Some("Nothing Bundled".to_owned());
    let mut choices = Choices::from_settings(&settings, FONTS);
    assert_eq!(shown(&choices, ControlId::LightTheme), "Tenmoku Light");
    let writes = pick(&mut choices, ControlId::LightTheme, "Light 2026");
    assert_eq!(writes.last(), Some(&(&["theme", "name"][..], None)));
}

#[test]
fn changing_the_mode_writes_only_the_mode() {
    let mut settings = Settings::default();
    settings.theme.name = Some("Ember".to_owned());
    let mut choices = Choices::from_settings(&settings, FONTS);
    assert_eq!(
        pick(&mut choices, ControlId::ThemeMode, "Light"),
        [(&["theme", "mode"][..], Some(SettingValue::Str("light")))]
    );
    assert_eq!(
        pick(&mut choices, ControlId::ThemeMode, "Dark"),
        [(&["theme", "mode"][..], None)]
    );
}

#[test]
fn values_not_offered_show_as_they_are_until_left() {
    let mut choices = Choices::from_settings(&beyond_the_lists(), FONTS);
    assert_eq!(
        shown(&choices, ControlId::DarkTheme),
        "acme.solarized/solarized-dark"
    );
    assert_eq!(
        shown(&choices, ControlId::LightTheme),
        "Catppuccin Latte",
        "the name gives the light slot"
    );
    assert_eq!(shown(&choices, ControlId::FontFamily), "Not Installed");
    assert_eq!(shown(&choices, ControlId::FontSize), "30");
    assert_eq!(shown(&choices, ControlId::AutoSave), "Custom");
    assert_eq!(shown(&choices, ControlId::InlineDiagnostics), "Custom");
    assert_eq!(shown(&choices, ControlId::LineNumbers), "Custom");

    let sizes = labels(&choices, ControlId::FontSize);
    assert_eq!(sizes.first().map(String::as_str), Some("10"));
    assert_eq!(sizes.last().map(String::as_str), Some("30"));
    assert_eq!(sizes.len(), 16);

    // Left, it can't be written back, so it isn't offered.
    assert_eq!(
        pick(&mut choices, ControlId::AutoSave, "Off"),
        [(&["editor", "auto_save"][..], None)]
    );
    assert_eq!(
        labels(&choices, ControlId::AutoSave),
        ["Off", "After a pause", "When focus leaves"]
    );
    assert_eq!(
        pick(&mut choices, ControlId::FontSize, "14"),
        [(
            &["editor", "font_size"][..],
            Some(SettingValue::Float(14.0))
        )]
    );
    assert_eq!(labels(&choices, ControlId::FontSize).len(), 15);
    assert_eq!(choices.changed_count(), 2);
}

#[test]
fn a_control_not_listed_writes_nothing() {
    let mut choices = Choices::from_settings(&Settings::default(), FONTS);
    assert_eq!(choices.choose(ControlId::VimPreset, 1), []);
    assert_eq!(choices.choose(ControlId::LightTheme, 0), []);
    assert_eq!(choices.choose(ControlId::SoftWrap, 99), []);
    assert_eq!(choices.changed_count(), 0);
}

/// Whether the settings schema has `key`, following its references.
fn in_schema(key: &str) -> bool {
    let schema = fe_config::settings_schema();
    let mut node = &schema;
    for name in key.split('.') {
        while let Some(reference) = node.get("$ref").and_then(|reference| reference.as_str()) {
            let pointer = reference.trim_start_matches('#');
            node = schema.pointer(pointer).unwrap();
        }
        match node.get("properties").and_then(|names| names.get(name)) {
            Some(next) => node = next,
            None => return false,
        }
    }
    true
}

#[test]
fn every_control_is_labelled() {
    assert!(!in_schema("editor.no_such_key"));
    for step in steps() {
        assert!(!step.title().is_empty());
    }
    let mut states = Vec::new();
    for base in [Settings::default(), beyond_the_lists()] {
        for mode in [
            ThemeModeSetting::Dark,
            ThemeModeSetting::Light,
            ThemeModeSetting::System,
        ] {
            for vim in [false, true] {
                let mut settings = base.clone();
                settings.theme.mode = mode;
                settings.vim.enabled = vim;
                states.push(settings);
            }
        }
    }
    let mut seen = Vec::new();
    for settings in &states {
        for fonts in [FONTS, &[]] {
            let choices = Choices::from_settings(settings, fonts);
            assert!(!choices.summary().is_empty());
            for control in all_controls(&choices) {
                let key = control.dotted_key();
                assert!(!control.title.is_empty(), "{key}");
                assert!(!control.description.is_empty(), "{key}");
                assert!(!control.options.is_empty(), "{key}");
                assert!(control.selected < control.options.len(), "{key}");
                for option in &control.options {
                    assert!(!option.label.is_empty(), "{key}");
                }
                assert!(in_schema(&key), "{key} isn't in the schema");
                if !seen.contains(&control.id) {
                    seen.push(control.id);
                }
            }
        }
    }
    assert_eq!(seen.len(), 15, "every control came up: {seen:?}");
}
