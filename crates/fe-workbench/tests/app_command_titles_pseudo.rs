//! The app commands' titles (`app.hide`, `app.hide_others`, `app.show_all`,
//! `app.quit`) come from kiln's catalogue under the pseudo-locale, as the
//! palette and the shortcuts editor show them (`CommandMeta::label`), not
//! from the English the registry registers: T588, from the T1720 audit.
//!
//! The catalogue's locale is the process's, so this is the only test in
//! its binary.

use gpui::TestAppContext;
// Links kiln, whose catalogue holds the app commands' titles.
use kiln as _;

fn label(id: &str) -> String {
    let registry = kiln_platform::commands::app_commands("Tenmoku");
    let command = registry
        .commands()
        .find(|meta| meta.id.as_str() == id)
        .cloned()
        .unwrap_or_else(|| panic!("{id} is registered"));
    command.label().to_string()
}

const IDS: [&str; 4] = ["app.hide", "app.hide_others", "app.show_all", "app.quit"];

#[gpui::test]
fn app_command_titles_come_from_the_catalogue(cx: &mut TestAppContext) {
    let english: Vec<String> = IDS.iter().map(|id| label(id)).collect();
    assert_eq!(english[0], "Hide Tenmoku");
    assert_eq!(english[1], "Hide Others");

    let pseudo: kiln_l10n::LanguageIdentifier = kiln_l10n::PSEUDO_LOCALE.parse().unwrap();
    // SAFETY: nothing else in this binary reads the environment meanwhile.
    unsafe { std::env::set_var(kiln_platform::locale::PSEUDO_ENV, "1") };
    cx.update(|cx| kiln_platform::locale::set_locale(cx, &[pseudo]));
    for (id, english) in IDS.iter().zip(&english) {
        assert_ne!(&label(id), english, "{id} shows English under en-XA");
    }
    assert!(label("app.hide").contains("Tenmoku"));

    // SAFETY: as above.
    unsafe { std::env::remove_var(kiln_platform::locale::PSEUDO_ENV) };
    cx.update(|cx| kiln_platform::locale::set_locale(cx, &["en-US".parse().unwrap()]));
    assert_eq!(label("app.hide"), "Hide Tenmoku");
}
