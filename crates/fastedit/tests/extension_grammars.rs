//! Tree-sitter grammars an extension contributes, `[[contributes.grammars]]`
//! compiled to WebAssembly, are registered with fe-syntax after
//! `kiln::init`; one naming a file outside the extension isn't.

#[allow(dead_code)]
#[path = "../src/config.rs"]
mod config;
#[allow(dead_code)]
#[path = "../src/theme.rs"]
mod theme;

use std::{fs, path::Path, sync::Mutex};

use fe_config::SettingsStore;
use gpui::TestAppContext;
use kiln::{
    Config,
    core::Severity,
    platform::config::{batch_channel, diagnostics},
    ui::ThemeMode,
};

/// The tests share the process-wide grammar registration, where a newer
/// batch stops an older one, so they run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

/// An extension `jsonx` in a fresh state directory, registered through
/// `kiln::init`, with the batch sender that tells the app its files changed.
fn jsonx(
    cx: &mut TestAppContext,
    language: &str,
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    kiln::platform::config::BatchSender,
) {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    let dir = extensions.join("jsonx");
    fs::create_dir_all(dir.join("grammars")).unwrap();
    fs::create_dir_all(dir.join("queries")).unwrap();
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    let fixture = Path::new(&manifest).join("../fe-syntax/tests/fixtures/tree-sitter-json.wasm");
    fs::copy(&fixture, dir.join("grammars/tree-sitter-json.wasm")).unwrap();
    fs::write(dir.join("queries/highlights.scm"), "(string) @string\n").unwrap();
    fs::write(
        dir.join("extension.toml"),
        format!(
            "id = \"acme.jsonx\"\nname = \"JSONX\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
             [engine]\ntenmoku = \"1.0\"\n\n\
             [[contributes.grammars]]\nlanguage = \"{language}\"\n\
             wasm = \"grammars/tree-sitter-json.wasm\"\nextensions = [\"{language}\"]\n\
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
    (state, extensions, sender)
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

/// The byte ranges of `{"a": 1}` that `language` highlights.
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
fn extension_grammar_reregisters_when_only_its_wasm_changes(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    const NAME: &str = "jsonx_wasm";
    let (_state, extensions, sender) = jsonx(cx, NAME);
    let first = language_after(cx, NAME, None);
    assert_eq!(highlighted(first), vec![1..4]);

    // A custom section, which a module may carry: same grammar, new bytes.
    let wasm = extensions.join("jsonx/grammars/tree-sitter-json.wasm");
    let mut bytes = fs::read(&wasm).unwrap();
    bytes.extend_from_slice(&[0, 3, 1, b'x', 0]);
    fs::write(&wasm, bytes).unwrap();
    sender.send(vec![extensions]);
    let second = language_after(cx, NAME, Some(first));
    assert_eq!(highlighted(second), vec![1..4]);
}

#[gpui::test]
fn extension_grammar_reregisters_when_only_its_query_changes(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    const NAME: &str = "jsonx_query";
    let (_state, extensions, sender) = jsonx(cx, NAME);
    let first = language_after(cx, NAME, None);
    assert_eq!(highlighted(first), vec![1..4]);

    let query = extensions.join("jsonx/queries/highlights.scm");
    fs::write(&query, "(number) @number\n").unwrap();
    sender.send(vec![extensions]);
    let second = language_after(cx, NAME, Some(first));
    assert_eq!(highlighted(second), vec![6..8]);
}

#[gpui::test]
fn extension_grammars_register(cx: &mut TestAppContext) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
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
         [engine]\ntenmoku = \"1.0\"\n\n\
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
    // Registering compiles on a background thread.
    for _ in 0..600 {
        if fe_syntax::language_by_name("jsonx").is_some() {
            break;
        }
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
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
