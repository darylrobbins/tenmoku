//! Replacing a registered grammar extension's `.wasm` with a module that
//! highlights differently serves the new highlights without a restart.

#[allow(dead_code)]
#[path = "../src/config.rs"]
mod config;
#[allow(dead_code)]
#[path = "../src/theme.rs"]
mod theme;

use std::{fs, path::Path};

use fe_config::SettingsStore;
use gpui::TestAppContext;
use kiln::{Config, platform::config::batch_channel, ui::ThemeMode};

/// The byte ranges of `{"a": 12}` that `language` highlights.
fn highlighted(language: &'static fe_syntax::Language) -> Vec<std::ops::Range<usize>> {
    let buffer = fe_buffer::Buffer::new("{\"a\": 12}");
    let mut syntax = fe_syntax::Syntax::new(language, &buffer.snapshot());
    assert!(syntax.parse_within(std::time::Duration::from_secs(10)));
    syntax
        .highlights(0..9)
        .into_iter()
        .map(|span| span.range)
        .collect()
}

#[gpui::test]
fn extension_grammar_swap_serves_the_new_highlights(cx: &mut TestAppContext) {
    const NAME: &str = "jsonx_swap";
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    let dir = extensions.join("jsonx");
    fs::create_dir_all(dir.join("grammars")).unwrap();
    fs::create_dir_all(dir.join("queries")).unwrap();
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    let manifest = Path::new(&manifest);
    let wasm = dir.join("grammars/tree-sitter-json.wasm");
    fs::copy(
        manifest.join("../fe-syntax/tests/fixtures/tree-sitter-json.wasm"),
        &wasm,
    )
    .unwrap();
    // A query both modules accept: each defines a `string` node.
    fs::write(dir.join("queries/highlights.scm"), "(string) @string\n").unwrap();
    fs::write(
        dir.join("extension.toml"),
        format!(
            "id = \"acme.jsonx\"\nname = \"JSONX\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
             [engine]\nfastedit = \"1.0\"\n\n\
             [[contributes.grammars]]\nlanguage = \"{NAME}\"\n\
             wasm = \"grammars/tree-sitter-json.wasm\"\nextensions = [\"{NAME}\"]\n\
             highlights = \"queries/highlights.scm\"\n"
        ),
    )
    .unwrap();
    let user = state.path().join("settings.toml");
    fs::write(&user, "").unwrap();
    let (sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .settings_store(SettingsStore::load(Some(user), None).into_inner())
        .theme_mode(ThemeMode::Dark)
        .watch_with(batches);
    let config = config::with_extensions(config, extensions.clone());
    cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();

    let first = language_after(cx, NAME, None);
    assert_eq!(highlighted(first), vec![1..4]);

    fs::copy(
        manifest.join("tests/fixtures/tree-sitter-json-words.wasm"),
        &wasm,
    )
    .unwrap();
    sender.send(vec![extensions]);
    let second = language_after(cx, NAME, Some(first));
    assert_eq!(highlighted(second), vec![2..3]);
}

/// The `name` language once it differs from `previous`: registering
/// compiles on a background thread.
fn language_after(
    cx: &mut TestAppContext,
    name: &str,
    previous: Option<&fe_syntax::Language>,
) -> &'static fe_syntax::Language {
    for _ in 0..600 {
        cx.run_until_parked();
        if let Some(language) = fe_syntax::language_by_name(name)
            && previous.is_none_or(|previous| !std::ptr::eq(previous, language))
        {
            return language;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("{name} not (re)registered");
}
