//! Snippets an extension contributes, `[[contributes.snippets]]`, reach
//! the workbench ([`fe_workbench::extension_snippets`]) after `kiln::init`,
//! from a manifest needing the extension API's 1.1, which added them, as
//! from one needing 1.0. One with an empty prefix or body, or a body that
//! doesn't parse, doesn't, while the rest of its extension's do; one of
//! tabstops alone does.

#[allow(dead_code)]
#[path = "../src/config.rs"]
mod config;
#[allow(dead_code)]
#[path = "../src/theme.rs"]
mod theme;

use std::{fs, path::Path};

use fe_config::SettingsStore;
use fe_workbench::ExtensionSnippet;
use gpui::TestAppContext;
use kiln::{
    Config,
    platform::config::{batch_channel, diagnostics},
    ui::ThemeMode,
};

/// Writes extension `name`'s manifest, needing API `api`, with `contributes`
/// after it.
fn extension(extensions: &Path, name: &str, api: &str, contributes: &str) {
    let dir = extensions.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("extension.toml"),
        format!(
            "id = \"acme.{name}\"\nname = \"Acme {name}\"\nversion = \"1.0.0\"\n\
             license = \"MIT\"\n\n[engine]\nfastedit = \"{api}\"\n\n{contributes}"
        ),
    )
    .unwrap();
}

#[gpui::test]
fn a_manifest_declares_snippets(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    extension(
        &extensions,
        "current",
        "1.1",
        "[[contributes.snippets]]\nprefix = \"fn\"\n\
         body = [\"fn ${1:name}(${2}) {\", \"\\t$0\", \"}\"]\n\
         description = \"Function\"\nlanguages = [\"rust\"]\n\n\
         [[contributes.snippets]]\nprefix = \"todo\"\nbody = \"TODO(${1:who}): $0\"\n\n\
         [[contributes.snippets]]\nprefix = \"stops\"\nbody = \"$1$0\"\n\n\
         [[contributes.snippets]]\nprefix = \"broken\"\nbody = \"fn ${1:name\"\n\n\
         [[contributes.snippets]]\nprefix = \"\"\nbody = \"no prefix\"\n\n\
         [[contributes.snippets]]\nprefix = \"empty\"\nbody = []\n",
    );
    extension(
        &extensions,
        "older",
        "1.0",
        "[[contributes.snippets]]\nprefix = \"main\"\n\
         body = \"func main() {\\n\\t$0\\n}\"\nlanguages = [\"go\"]\n",
    );
    // A later API than fastedit's isn't loaded.
    extension(
        &extensions,
        "later",
        "1.2",
        "[[contributes.snippets]]\nprefix = \"later\"\nbody = \"later\"\n",
    );
    let user = state.path().join("settings.toml");
    fs::write(&user, "").unwrap();
    let (_sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .settings_store(SettingsStore::load(Some(user), None).into_inner())
        .theme_mode(ThemeMode::Dark)
        .watch_with(batches);
    let config = config::with_extensions(config, extensions.clone());
    cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();

    let errors: Vec<_> = (cx.update(|cx| diagnostics(cx).to_vec()).into_iter())
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| diagnostic.file)
        .collect();
    assert_eq!(errors, [extensions.join("later/extension.toml")]);
    let mut snippets = cx.update(|cx| fe_workbench::extension_snippets(cx));
    snippets.sort_by(|a, b| a.prefix.cmp(&b.prefix));
    let snippet = |id: &str, prefix: &str, body: &str, description: Option<&str>, languages| {
        ExtensionSnippet {
            extension_id: format!("acme.{id}"),
            extension_name: format!("Acme {id}"),
            prefix: prefix.to_owned(),
            body: body.to_owned(),
            description: description.map(str::to_owned),
            languages,
        }
    };
    assert_eq!(
        snippets,
        [
            snippet(
                "current",
                "fn",
                "fn ${1:name}(${2}) {\n\t$0\n}",
                Some("Function"),
                vec!["rust".to_owned()],
            ),
            snippet(
                "older",
                "main",
                "func main() {\n\t$0\n}",
                None,
                vec!["go".to_owned()],
            ),
            snippet("current", "stops", "$1$0", None, vec![]),
            snippet("current", "todo", "TODO(${1:who}): $0", None, vec![]),
        ]
    );
}
