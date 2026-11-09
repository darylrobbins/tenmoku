//! `workbench.locale` (i18n design § 6): `auto` or a language tag, the
//! user's only. A tag no translation ships is dropped, so negotiation goes on
//! with the system's languages, and its warning names the language chosen,
//! never assuming English.

use kiln_l10n::LanguageIdentifier;

use super::write;
use crate::{
    LocaleSetting, SettingsModel, SettingsStore, Severity, USER_SETTINGS_TEMPLATE,
    settings::project_locale_warning, settings_schema,
};

fn ids(tags: &[&str]) -> Vec<LanguageIdentifier> {
    tags.iter().map(|tag| tag.parse().unwrap()).collect()
}

/// `text` as the user's settings file: the setting it loads, and its
/// choice and warning for a system preferring `system` with `shipped`.
fn choose(
    text: &str,
    system: &[&str],
    shipped: &[&str],
) -> (crate::LocaleChoice, Option<crate::Diagnostic>) {
    let dir = tempfile::tempdir().unwrap();
    let user = write(&dir, "settings.toml", text);
    let store = SettingsStore::load(Some(user.clone()), None);
    assert_eq!(store.diagnostics(), [], "any tag loads");
    let setting = &store.settings().workbench.locale;
    let choice = setting.choose(&ids(system), &ids(shipped));
    let diagnostic = LocaleSetting::diagnostic(&choice, &user, text);
    (choice, diagnostic)
}

#[test]
fn unshipped_locale_on_a_shipped_system_language_uses_it_and_says_so() {
    let text = "[workbench]\nstart_page = true\nlocale = \"xx\"\n";
    let (choice, diagnostic) = choose(text, &["de-AT", "fr"], &["en-US", "de"]);
    assert_eq!(choice.requested, ids(&["xx", "de-AT", "fr"]));
    assert_eq!(choice.chain, ids(&["de", "en-US"]));
    let diagnostic = diagnostic.expect("xx ships no translation");
    assert_eq!(diagnostic.severity, Severity::Warning);
    assert_eq!(diagnostic.key.as_deref(), Some("workbench.locale"));
    assert_eq!((diagnostic.line, diagnostic.column), (3, 1));
    assert_eq!(
        diagnostic.message, "No `xx` translation; using `de`",
        "names the language chosen"
    );
}

#[test]
fn unshipped_locale_on_an_unshipped_system_language_uses_en_us_and_says_so() {
    let (choice, diagnostic) = choose("workbench.locale = \"xx\"\n", &["fr-FR"], &["en-US", "de"]);
    assert_eq!(choice.chain, ids(&["en-US"]));
    let diagnostic = diagnostic.expect("xx ships no translation");
    assert_eq!(diagnostic.message, "No `xx` translation; using `en-US`");
    assert_eq!(diagnostic.line, 1);
}

#[test]
fn shipped_locale_comes_first_with_no_diagnostic() {
    let (choice, diagnostic) = choose(
        "[workbench]\nlocale = \"de\"\n",
        &["fr", "en-GB"],
        &["en-US", "de", "fr"],
    );
    assert_eq!(choice.chain, ids(&["de", "fr", "en-US"]));
    assert_eq!(choice.problem, None);
    assert_eq!(diagnostic, None);

    // A region or script the shipped locale lacks still picks its language.
    let (choice, diagnostic) = choose("workbench.locale = \"de-CH\"\n", &[], &["en-US", "de"]);
    assert_eq!(choice.chain, ids(&["de", "en-US"]));
    assert_eq!(diagnostic, None);
}

#[test]
fn unshipped_script_is_reported_though_the_system_language_ships() {
    // zh-Hans ships and the system prefers it, but the override asks for
    // another script: the system's language is used, and the warning says so.
    let (choice, diagnostic) = choose(
        "workbench.locale = \"zh-Hant\"\n",
        &["zh-Hans"],
        &["en-US", "zh-Hans"],
    );
    assert_eq!(choice.chain, ids(&["zh-Hans", "en-US"]));
    assert_eq!(
        diagnostic.map(|d| d.message),
        Some("No `zh-Hant` translation; using `zh-Hans`".to_owned())
    );

    // Likewise a region nothing ships, on a system whose language ships
    // another of the same language's: negotiation matches it loosely.
    let (choice, diagnostic) = choose(
        "workbench.locale = \"pt-BR\"\n",
        &["fr"],
        &["en-US", "pt-PT"],
    );
    assert_eq!(choice.chain, ids(&["pt-PT", "en-US"]));
    assert_eq!(diagnostic, None, "pt-BR negotiates to the shipped pt-PT");
}

#[test]
fn auto_follows_the_system_and_a_bad_tag_is_dropped() {
    let (choice, diagnostic) = choose("", &["de-AT", "fr"], &["en-US", "de"]);
    assert_eq!(choice.requested, ids(&["de-AT", "fr"]));
    assert_eq!(choice.chain, ids(&["de", "en-US"]));
    assert_eq!(diagnostic, None, "auto never warns");
    let (choice, _) = choose("workbench.locale = \"auto\"\n", &[], &["en-US", "de"]);
    assert_eq!(choice.chain, ids(&["en-US"]));

    let (choice, diagnostic) = choose(
        "workbench.locale = \"not a tag\"\n",
        &["de"],
        &["en-US", "de"],
    );
    assert_eq!(choice.requested, ids(&["de"]));
    assert_eq!(choice.chain, ids(&["de", "en-US"]));
    assert_eq!(
        diagnostic.map(|d| d.message),
        Some("No `not a tag` translation; using `de`".to_owned())
    );
}

#[test]
fn locale_is_the_users_only_and_documented() {
    let dir = tempfile::tempdir().unwrap();
    let project = write(
        &dir,
        "project.toml",
        "[workbench]\npreview_tabs = false\nlocale = \"de\"\n",
    );
    let store = SettingsStore::load(None, Some(project));
    assert_eq!(store.settings().workbench.locale, LocaleSetting::Auto);
    assert!(
        !store.settings().workbench.preview_tabs,
        "its other keys stay"
    );
    let found: Vec<_> = (store.diagnostics().iter())
        .map(|d| (d.severity, d.key.as_deref(), d.line, d.message.clone()))
        .collect();
    assert_eq!(
        found,
        [(
            Severity::Warning,
            Some("workbench.locale"),
            3,
            project_locale_warning()
        )]
    );
    assert_eq!(
        project_locale_warning(),
        "workbench.locale in project files is ignored: the language is the same in every window"
    );

    let model = SettingsModel::new(None, Some("[workbench]\nlocale = \"de\"\n"));
    let entry = (model.entries().iter())
        .find(|entry| entry.key_path() == "workbench.locale")
        .expect("listed");
    assert!(!model.workspace_allowed(entry));
    assert_eq!(model.value(entry).value, "auto");

    assert!(USER_SETTINGS_TEMPLATE.contains("\n# locale = \"auto\"\n"));
    let schema = settings_schema();
    let prop = &schema["$defs"]["WorkbenchSettings"]["properties"]["locale"];
    assert_eq!(prop["default"], "auto");
    assert_eq!(prop["type"], "string");
    assert_eq!(prop["enum"], serde_json::json!(["auto", "en-US"]));

    // Round trips as written.
    let tag: LocaleSetting = toml::Value::from("pt-BR").try_into().unwrap();
    assert_eq!(tag, LocaleSetting::Tag("pt-BR".into()));
    assert_eq!(toml::Value::try_from(&tag).unwrap().as_str(), Some("pt-BR"));
    assert_eq!(
        toml::Value::try_from(LocaleSetting::Auto).unwrap().as_str(),
        Some("auto")
    );
}
