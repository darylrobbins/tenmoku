//! The `Config` fastedit starts from, without the user's files, sets up
//! cleanly on every platform: its keymap and menus name registered
//! commands, and nothing in them hides or shadows anything else.

#[allow(dead_code)]
#[path = "../src/config.rs"]
mod config;
#[allow(dead_code)]
#[path = "../src/theme.rs"]
mod theme;

use fe_config::SettingsStore;

#[gpui::test]
fn the_shipped_config_is_clean(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        config::register_commands(cx);
        kiln::testing::assert_clean_config(&config::shipped(SettingsStore::load(None, None)), cx);
    });
}
