//! Tree-sitter grammars an extension contributes, `[[contributes.grammars]]`
//! compiled to WebAssembly, are registered with fe-syntax after
//! `kiln::init`; one naming a file outside the extension isn't.

#[allow(dead_code)]
#[path = "../src/config.rs"]
mod config;
#[allow(dead_code)]
#[path = "../src/theme.rs"]
mod theme;

use std::{fs, path::Path};

use fe_config::SettingsStore;
use gpui::TestAppContext;
use kiln::{
    Config,
    core::Severity,
    platform::config::{batch_channel, diagnostics},
    ui::ThemeMode,
};

#[gpui::test]
fn extension_grammars_register(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    let dir = extensions.join("jsonx");
    fs::create_dir_all(dir.join("grammars")).unwrap();
    fs::create_dir_all(dir.join("queries")).unwrap();
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    let fixture = Path::new(&manifest).join("../fe-syntax/tests/fixtures/tree-sitter-json.wasm");
    fs::copy(&fixture, dir.join("grammars/tree-sitter-json.wasm")).unwrap();
    fs::copy(&fixture, state.path().join("tree-sitter-json.wasm")).unwrap();
    fs::write(dir.join("queries/highlights.scm"), "(string) @string\n").unwrap();
    fs::write(
        dir.join("extension.toml"),
        "id = \"acme.jsonx\"\nname = \"JSONX\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
         [engine]\nfastedit = \"1.0\"\n\n\
         [[contributes.grammars]]\nlanguage = \"jsonx\"\n\
         wasm = \"grammars/tree-sitter-json.wasm\"\nextensions = [\"jsonx\"]\n\
         file_names = [\"JSONXFILE\"]\nhighlights = \"queries/highlights.scm\"\n\
         license = \"MIT\"\n\n\
         [[contributes.grammars]]\nlanguage = \"escaped\"\n\
         wasm = \"../../tree-sitter-json.wasm\"\nextensions = [\"escaped\"]\n",
    )
    .unwrap();
    let user = state.path().join("settings.toml");
    fs::write(&user, "").unwrap();
    let (_sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .settings_store(SettingsStore::load(Some(user), None).into_inner())
        .theme_mode(ThemeMode::Dark)
        .watch_with(batches);
    let config = config::with_extensions(config, extensions);
    cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();

    let errors: Vec<_> = cx
        .update(|cx| diagnostics(cx).to_vec())
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(errors, []);
    assert_eq!(
        fe_syntax::language_name_for_path(Path::new("src/a.jsonx")),
        Some("jsonx")
    );
    assert_eq!(
        fe_syntax::language_name_for_path(Path::new("src/JSONXFILE")),
        Some("jsonx")
    );
    let language = fe_syntax::language_by_name("jsonx").expect("registered");
    assert_eq!(language.name(), "jsonx");
    assert!(fe_syntax::language_by_name("escaped").is_none());
    assert_eq!(
        fe_syntax::language_name_for_path(Path::new("src/a.escaped")),
        None
    );
}
