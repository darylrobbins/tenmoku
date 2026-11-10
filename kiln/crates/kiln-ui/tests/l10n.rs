// SPDX-License-Identifier: Apache-2.0

//! kiln-ui's catalogue: the literal lint (kiln-l10n), no hard-coded
//! user-facing text in this crate's non-test source outside
//! `l10n-allow.toml`, and every key it uses exists; then, in en-US, the
//! tooltips it joins as they were before they were messages, and in
//! another locale, Escape on a prompt's translated Cancel.

use std::sync::{Mutex, MutexGuard, PoisonError};

use gpui::PromptButton;
use kiln_l10n::{LanguageIdentifier, Registered, ResourceFile, Resources, set_locales, tr};
use kiln_ui::{Prompt, Tab, Tooltip, UiTheme};

#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &kiln_ui::l10n::RESOURCES).check();
}

/// The process catalogue is global, and plain `cargo test` runs these
/// tests as threads of one process: each test that sets or reads it holds
/// this for its whole run.
fn locale() -> MutexGuard<'static, ()> {
    static LOCALE: Mutex<()> = Mutex::new(());
    LOCALE.lock().unwrap_or_else(PoisonError::into_inner)
}

#[test]
fn en_us_tooltips_are_unchanged() {
    let _guard = locale();
    set_locales(&[]);
    let theme = UiTheme::default();
    let tooltip = Tooltip::new("Search", Some("⇧⌘F"), &theme);
    assert_eq!(tooltip.text(), "Search ⇧⌘F");
    let tooltip = Tooltip::new("Search", None::<&str>, &theme);
    assert_eq!(tooltip.text(), "Search");

    let tab = (Tab::new("tab", "main.rs", &theme))
        .description(Some("src".into()))
        .pinned(true);
    assert_eq!(tab.tooltip_text().as_deref(), Some("main.rs — src"));
    let tab = Tab::new("tab", "main.rs", &theme).pinned(true);
    assert_eq!(tab.tooltip_text().as_deref(), Some("main.rs"));
}

/// A fake locale, `qps-ploc`, with kiln-ui's Cancel.
static FAKE_LOCALE: Resources = Resources {
    crate_name: "kiln-ui-fake-locale",
    manifest_dir: env!("CARGO_MANIFEST_DIR"),
    declared_in: file!(),
    files: &[ResourceFile {
        locale: "qps-ploc",
        path: "qps-ploc/kiln-ui.ftl",
        source: "kiln-ui-cancel = Ćáñćéł\n",
    }],
    dynamic_prefixes: &[],
};

inventory::submit! { Registered(&FAKE_LOCALE) }

/// Escape presses a button titled Cancel in the active locale, as it does
/// one titled "Cancel" in English, and Enter still presses the first.
#[test]
fn escape_presses_a_translated_cancel() {
    let _guard = locale();
    let fake: Vec<LanguageIdentifier> = vec!["qps-ploc".parse().unwrap()];
    set_locales(&fake);
    let cancel = tr!("kiln-ui-cancel");
    assert_eq!(cancel, "Ćáñćéł");
    let buttons = [PromptButton::ok(cancel), PromptButton::new("Discard")];
    assert_eq!(Prompt::cancel_button(&buttons), Some(0));
    assert_eq!(Prompt::default_button(&buttons), Some(0));
    // English "Cancel" still counts, as AppKit gives it Escape.
    let english = [PromptButton::new("Discard"), PromptButton::ok("Cancel")];
    assert_eq!(Prompt::cancel_button(&english), Some(1));
    set_locales(&[]);
}
