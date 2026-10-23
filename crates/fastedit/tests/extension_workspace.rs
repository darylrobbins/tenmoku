//! A trusted folder's own extensions, `<root>/.fastedit/extensions`, load
//! through fastedit's kinds beside the user's (extensions record §3), and
//! a workspace extension's language server needs consent of its own: the
//! user's grant of a server of the same name doesn't cover it.

#[allow(dead_code)]
#[path = "../src/config.rs"]
mod config;
#[allow(dead_code)]
#[path = "../src/theme.rs"]
mod theme;
#[allow(dead_code)]
#[path = "../src/workspace_extensions.rs"]
mod workspace_extensions;

use std::{fs, path::Path};

use fe_config::SettingsStore;
use gpui::TestAppContext;
use kiln::{
    Config,
    core::{extension::MANIFEST_FILE, trust::canonical},
    platform::config::{BatchSender, batch_channel},
    shell::trust,
    ui::ThemeMode,
};

/// An extension `id` in `extensions/<dir>`, its `zls` reading `zls.json`.
fn install(extensions: &Path, dir: &str, id: &str) {
    let dir = extensions.join(dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("zls.json"), "{}\n").unwrap();
    fs::write(
        dir.join(MANIFEST_FILE),
        format!(
            "id = \"{id}\"\nname = \"Zig\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
             [engine]\nfastedit = \"1.0\"\n\n\
             [contributes.lsp.zls]\ncommand = \"zls\"\nargs = [\"--config-path\", \"zls.json\"]\n\
             languages = [\"zig\"]\n"
        ),
    )
    .unwrap();
}

fn pending(cx: &mut TestAppContext) -> Vec<fe_workbench::PendingConsent> {
    cx.update(|cx| fe_workbench::pending_extension_consents(cx))
}

/// The names of the servers the workbench has.
fn servers(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| fe_workbench::extension_servers(cx))
        .into_iter()
        .map(|spec| spec.name)
        .collect()
}

#[gpui::test]
fn a_trusted_workspace_server_needs_its_own_consent(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let user_extensions = state.path().join("extensions");
    install(&user_extensions, "zig", "acme.zig");
    let root = state.path().join("project");
    install(&root.join(".fastedit/extensions"), "zig", "team.zig");
    let root = canonical(&root);
    let workspace_zig = root.join(".fastedit/extensions/zig");

    let user = state.path().join("settings.toml");
    fs::write(&user, "").unwrap();
    let (_sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .settings_store(SettingsStore::load(Some(user), None).into_inner())
        .theme_mode(ThemeMode::Dark)
        .watch_with(batches);
    let config = config::with_extensions(config, user_extensions.clone());
    let consent = state.path().join(fe_workbench::CONSENT_FILE);
    cx.update(|cx| {
        trust::init(cx);
        fe_workbench::set_extension_consent_file(cx, consent);
        kiln::init(cx, config);
        workspace_extensions::follow(Some(root.clone()), cx);
    });
    cx.run_until_parked();

    // The user's server is asked about; the untrusted folder's isn't.
    let asked = pending(cx);
    assert_eq!(asked.len(), 1, "{asked:#?}");
    assert_eq!(asked[0].extension_id, "acme.zig");
    cx.update(|cx| fe_workbench::grant_extension_server(cx, &asked[0]))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(servers(cx), ["zls"]);
    assert_eq!(pending(cx), []);

    // Trusted, the folder's server of the same name is pending, keyed by
    // its own directory: the user's grant doesn't cover it.
    cx.update(|cx| trust::decide(&root, true, cx));
    cx.run_until_parked();
    let asked = pending(cx);
    assert_eq!(asked.len(), 1, "{asked:#?}");
    let asked = &asked[0];
    assert_eq!(
        (asked.extension_id.as_str(), asked.server.as_str()),
        ("team.zig", "zls")
    );
    assert_eq!(asked.extension_dir, workspace_zig.canonicalize().unwrap());
    assert_eq!(asked.previous, None);
    assert!(!asked.declined);
    assert_eq!(servers(cx), ["zls"], "only the user's runs");

    // Untrusted again, the folder's server is withdrawn, and asked about
    // no more; trusted, it's asked about again.
    cx.update(|cx| trust::decide(&root, false, cx));
    cx.run_until_parked();
    assert_eq!(pending(cx), []);
    cx.update(|cx| trust::decide(&root, true, cx));
    cx.run_until_parked();
    assert_eq!(pending(cx), std::slice::from_ref(asked));

    // Granted, it's decided; the managers run the first of a name, the
    // user's.
    cx.update(|cx| fe_workbench::grant_extension_server(cx, asked))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(pending(cx), []);
    assert_eq!(servers(cx), ["zls"]);
}

/// Waits until `done`, as registering compiles on a background thread.
fn until(cx: &mut TestAppContext, what: &str, done: impl Fn() -> bool) {
    for _ in 0..600 {
        cx.run_until_parked();
        if done() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("{what}");
}

/// An extension `id` in `extensions/<dir>` with a grammar for `language`,
/// files `*.<language>`, highlighting strings: the JSON grammar's module,
/// or, unless `valid`, bytes that aren't a module, which fe-syntax refuses.
fn grammar_extension(extensions: &Path, id: &str, language: &str, valid: bool) {
    grammar_extension_highlighting(extensions, id, language, valid, "(string) @string\n");
}

/// [`grammar_extension`], with `highlights` as its highlights query.
fn grammar_extension_highlighting(
    extensions: &Path,
    id: &str,
    language: &str,
    valid: bool,
    highlights: &str,
) {
    let dir = extensions.join(id);
    fs::create_dir_all(&dir).unwrap();
    let wasm = dir.join("tree-sitter-json.wasm");
    if valid {
        let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
        let fixture =
            Path::new(&manifest).join("../fe-syntax/tests/fixtures/tree-sitter-json.wasm");
        fs::copy(&fixture, wasm).unwrap();
    } else {
        fs::write(wasm, "not a module").unwrap();
    }
    fs::write(dir.join("highlights.scm"), highlights).unwrap();
    fs::write(
        dir.join(MANIFEST_FILE),
        format!(
            "id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
             [engine]\nfastedit = \"1.0\"\n\n\
             [[contributes.grammars]]\nlanguage = \"{language}\"\n\
             wasm = \"tree-sitter-json.wasm\"\nextensions = [\"{language}\"]\n\
             highlights = \"highlights.scm\"\n"
        ),
    )
    .unwrap();
}

/// The byte ranges of `{"a": 12}` that `language` highlights.
fn highlighted(language: &'static fe_syntax::Language) -> Vec<std::ops::Range<usize>> {
    let buffer = fe_buffer::Buffer::new("{\"a\": 12}");
    let mut syntax = fe_syntax::Syntax::new(language, &buffer.snapshot());
    assert!(syntax.parse_within(std::time::Duration::from_secs(10)));
    (syntax.highlights(0..9).into_iter())
        .map(|span| span.range)
        .collect()
}

/// fastedit's config with the user's extensions in `state/extensions`,
/// following the workspace root `root`, with the batch sender that tells
/// it files changed.
fn load_with_root(cx: &mut TestAppContext, state: &Path, root: &Path) -> BatchSender {
    let user = state.join("settings.toml");
    fs::write(&user, "").unwrap();
    let (sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .settings_store(SettingsStore::load(Some(user), None).into_inner())
        .theme_mode(ThemeMode::Dark)
        .watch_with(batches);
    let config = config::with_extensions(config, state.join("extensions"));
    cx.update(|cx| {
        trust::init(cx);
        kiln::init(cx, config);
        workspace_extensions::follow(Some(root.to_owned()), cx);
    });
    cx.run_until_parked();
    sender
}

fn registered(language: &str) -> bool {
    fe_syntax::language_by_name(language).is_some()
}

/// The language a file `a.<extension>` opened now gets.
fn for_file(extension: &str) -> Option<&'static str> {
    fe_syntax::language_name_for_path(Path::new(&format!("a.{extension}")))
}

#[gpui::test]
fn a_workspace_grammar_goes_with_trust(cx: &mut TestAppContext) {
    const NAME: &str = "jsonws";
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("project");
    grammar_extension(
        &root.join(".fastedit/extensions"),
        "team.jsonws",
        NAME,
        true,
    );
    let root = canonical(&root);
    load_with_root(cx, state.path(), &root);
    std::thread::sleep(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    assert!(!registered(NAME), "untrusted, it isn't registered");

    cx.update(|cx| trust::decide(&root, true, cx));
    until(cx, "registered once trusted", || registered(NAME));
    assert_eq!(for_file(NAME), Some(NAME));

    // Revoking trust withdraws it: a file opened after doesn't get it.
    cx.update(|cx| trust::decide(&root, false, cx));
    until(cx, "withdrawn once untrusted", || !registered(NAME));
    assert_eq!(for_file(NAME), None);

    // Trusted again, then the root changes: withdrawn again.
    cx.update(|cx| trust::decide(&root, true, cx));
    until(cx, "registered once trusted again", || registered(NAME));
    let other = state.path().join("other");
    fs::create_dir_all(&other).unwrap();
    cx.update(|cx| workspace_extensions::follow(Some(other), cx));
    until(cx, "withdrawn once the root changed", || !registered(NAME));
    assert_eq!(for_file(NAME), None);
}

#[gpui::test]
fn a_users_broken_grammar_of_the_name_doesnt_keep_the_workspaces(cx: &mut TestAppContext) {
    const NAME: &str = "jsonshared";
    let state = tempfile::tempdir().unwrap();
    grammar_extension(&state.path().join("extensions"), "acme.broken", NAME, false);
    let root = state.path().join("project");
    grammar_extension(
        &root.join(".fastedit/extensions"),
        "team.jsonshared",
        NAME,
        true,
    );
    let root = canonical(&root);
    load_with_root(cx, state.path(), &root);

    // The user's is refused; the trusted folder's registers.
    cx.update(|cx| trust::decide(&root, true, cx));
    until(cx, "the workspace's registered", || registered(NAME));

    // Revoked, the user's still names the language, and is refused again:
    // the workspace's goes all the same.
    cx.update(|cx| trust::decide(&root, false, cx));
    until(cx, "the workspace's withdrawn", || !registered(NAME));
    assert_eq!(for_file(NAME), None);
}

#[gpui::test]
fn another_roots_broken_grammar_of_the_name_doesnt_keep_the_first(cx: &mut TestAppContext) {
    const NAME: &str = "jsonroots";
    let state = tempfile::tempdir().unwrap();
    let first = state.path().join("first");
    grammar_extension(&first.join(".fastedit/extensions"), "team.json", NAME, true);
    let second = state.path().join("second");
    grammar_extension(
        &second.join(".fastedit/extensions"),
        "team.json",
        NAME,
        false,
    );
    let (first, second) = (canonical(&first), canonical(&second));
    load_with_root(cx, state.path(), &first);
    cx.update(|cx| {
        trust::decide(&first, true, cx);
        trust::decide(&second, true, cx);
    });
    until(cx, "the first root's registered", || registered(NAME));

    // The second root's, of the same extension ID and language, is refused:
    // the first's goes.
    cx.update(|cx| workspace_extensions::follow(Some(second), cx));
    until(cx, "the first root's withdrawn", || !registered(NAME));
    assert_eq!(for_file(NAME), None);
}

#[gpui::test]
fn a_broken_edit_keeps_its_own_last_good_grammar(cx: &mut TestAppContext) {
    const NAME: &str = "jsonedit";
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join("project");
    let extensions = root.join(".fastedit/extensions");
    grammar_extension(&extensions, "team.jsonedit", NAME, true);
    let root = canonical(&root);
    let sender = load_with_root(cx, state.path(), &root);
    cx.update(|cx| trust::decide(&root, true, cx));
    until(cx, "registered once trusted", || registered(NAME));
    let first = fe_syntax::language_by_name(NAME).unwrap();

    // Its own module, broken while trusted, is refused: it keeps what it
    // registered.
    fs::write(
        extensions.join("team.jsonedit/tree-sitter-json.wasm"),
        "broken",
    )
    .unwrap();
    sender.send(vec![root.join(".fastedit/extensions")]);
    cx.run_until_parked();
    std::thread::sleep(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    let kept = fe_syntax::language_by_name(NAME).expect("kept");
    assert!(std::ptr::eq(first, kept));
}

#[gpui::test]
fn withdrawing_trust_keeps_the_users_grammar_of_the_name(cx: &mut TestAppContext) {
    const NAME: &str = "jsonboth";
    let state = tempfile::tempdir().unwrap();
    // The user's highlights strings; the workspace's, numbers.
    let user = state.path().join("extensions");
    grammar_extension_highlighting(&user, "acme.json", NAME, true, "(string) @string\n");
    let root = state.path().join("project");
    let workspace = root.join(".fastedit/extensions");
    grammar_extension_highlighting(&workspace, "team.json", NAME, true, "(number) @number\n");
    let root = canonical(&root);
    load_with_root(cx, state.path(), &root);
    let strings: Vec<_> = std::iter::once(1..4).collect();
    let numbers: Vec<_> = std::iter::once(6..8).collect();
    let current = || fe_syntax::language_by_name(NAME).map(highlighted);
    until(cx, "the user's registered", || {
        current() == Some(strings.clone())
    });

    // Trusted, the workspace's registers after the user's.
    cx.update(|cx| trust::decide(&root, true, cx));
    until(cx, "the workspace's registered", || {
        current() == Some(numbers.clone())
    });

    // Withdrawn, the workspace's goes and the user's stays.
    cx.update(|cx| trust::decide(&root, false, cx));
    until(cx, "the user's back", || current() == Some(strings.clone()));
    assert_eq!(for_file(NAME), Some(NAME));
    std::thread::sleep(std::time::Duration::from_millis(300));
    cx.run_until_parked();
    assert_eq!(current(), Some(strings), "and it stays");
}
