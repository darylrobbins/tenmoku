//! `[editor.minimap]`: VS Code's `editor.minimap.*` names and defaults.

use fe_config::settings::{MinimapSettings, MinimapShowSlider, MinimapSide, MinimapSize};
use fe_config::{Settings, SettingsStore};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

fn minimap(contents: &str) -> MinimapSettings {
    let store = load(contents);
    assert_eq!(store.diagnostics(), [], "{contents}");
    store.settings().editor.minimap.clone()
}

#[test]
fn minimap_defaults_are_vscode_s() {
    let defaults = Settings::default().editor.minimap;
    assert_eq!(
        defaults,
        MinimapSettings {
            enabled: true,
            side: MinimapSide::Right,
            render_characters: true,
            max_column: 120,
            scale: 1,
            show_slider: MinimapShowSlider::Mouseover,
            size: MinimapSize::Proportional,
        }
    );
    assert_eq!(minimap(""), defaults);
    assert_eq!(minimap("[editor.minimap]\n"), defaults);
}

#[test]
fn minimap_parses_every_value() {
    let set = minimap(
        "[editor.minimap]\nenabled = false\nrender_characters = false\nmax_column = 80\n\
         scale = 3\n",
    );
    assert!(!set.enabled);
    assert!(!set.render_characters);
    assert_eq!(set.max_column, 80);
    assert_eq!(set.scale, 3);
    // The dotted form a user may write under [editor].
    assert_eq!(minimap("[editor]\nminimap.scale = 2\n").scale, 2);

    for (name, want) in [("right", MinimapSide::Right), ("left", MinimapSide::Left)] {
        let got = minimap(&format!("[editor.minimap]\nside = \"{name}\"\n")).side;
        assert_eq!(got, want, "{name}");
    }
    for (name, want) in [
        ("mouseover", MinimapShowSlider::Mouseover),
        ("always", MinimapShowSlider::Always),
    ] {
        let got = minimap(&format!("[editor.minimap]\nshow_slider = \"{name}\"\n")).show_slider;
        assert_eq!(got, want, "{name}");
    }
    for (name, want) in [
        ("proportional", MinimapSize::Proportional),
        ("fill", MinimapSize::Fill),
        ("fit", MinimapSize::Fit),
    ] {
        let got = minimap(&format!("[editor.minimap]\nsize = \"{name}\"\n")).size;
        assert_eq!(got, want, "{name}");
    }
}

#[test]
fn minimap_rejects_bad_values_with_a_diagnostic() {
    for bad in [
        "scale = 0",
        "scale = 4",
        "max_column = 0",
        "side = \"top\"",
        "show_slider = \"never\"",
        "size = \"huge\"",
        "enabled = \"yes\"",
    ] {
        let store = load(&format!("[editor.minimap]\n{bad}\n"));
        let diagnostics = store.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{bad}: {diagnostics:#?}");
        assert!(diagnostics[0].is_error(), "{bad}");
        assert_eq!(
            store.settings().editor.minimap,
            MinimapSettings::default(),
            "{bad}"
        );
    }
    let store = load("[editor.minimap]\nscale = 4\n");
    let message = store.diagnostics()[0].to_string();
    assert!(message.contains("scale"), "{message}");
}
