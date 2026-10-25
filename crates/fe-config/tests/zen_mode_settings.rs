//! `[workbench.zen_mode]`: VS Code's `zenMode.*` subset, with its defaults,
//! in the template and the schema.

use fe_config::settings::ZenModeSettings;
use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE, settings_schema};

const KEYS: [&str; 3] = ["hide_tabs", "hide_status_bar", "center_layout"];

fn zen_mode(contents: &str) -> ZenModeSettings {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    let store = SettingsStore::load(Some(user), None);
    assert_eq!(store.diagnostics(), [], "{contents}");
    store.settings().workbench.zen_mode.clone()
}

#[test]
fn zen_mode_hides_and_centers_by_default() {
    let defaults = Settings::default().workbench.zen_mode;
    let all_on = ZenModeSettings {
        hide_tabs: true,
        hide_status_bar: true,
        center_layout: true,
    };
    assert_eq!(defaults, all_on);
    assert_eq!(zen_mode(""), all_on);
    assert_eq!(zen_mode("[workbench.zen_mode]\n"), all_on);

    let set = zen_mode("[workbench.zen_mode]\nhide_tabs = false\ncenter_layout = false\n");
    assert!(!set.hide_tabs && set.hide_status_bar && !set.center_layout);
    // The dotted form a user may write under [workbench].
    assert!(!zen_mode("[workbench]\nzen_mode.hide_status_bar = false\n").hide_status_bar);
}

#[test]
fn the_template_and_the_schema_list_them() {
    let (_, section) = USER_SETTINGS_TEMPLATE
        .split_once("[workbench.zen_mode]\n")
        .expect("a [workbench.zen_mode] section");
    let section = section.split("\n[").next().unwrap();
    for key in KEYS {
        assert!(section.contains(&format!("# {key} = true\n")), "{key}");
    }

    let schema = settings_schema();
    let zen = &schema["$defs"]["ZenModeSettings"]["properties"];
    for key in KEYS {
        assert_eq!(zen[key]["type"], "boolean", "{key}");
        assert_eq!(zen[key]["default"], true, "{key}");
    }
    let workbench = &schema["$defs"]["WorkbenchSettings"]["properties"];
    assert!(workbench["zen_mode"].is_object(), "{workbench:#}");
}
