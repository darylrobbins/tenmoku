//! The installer's writing and its query helpers: what it writes is an
//! extension the real loader registers, and a failure leaves the extensions
//! directory as it was. (The index route's offers are in
//! `grammar_install_index_tests.rs`.)

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use fe_config::SettingsStore;
use fe_net::GrammarFetchError;
use fe_net::sha256_hex;
use gpui::TestAppContext;
use kiln::{Config, platform::config::batch_channel, ui::ThemeMode};

use crate::grammar_install::{
    GRANTED_HOSTS, InstallError, Staged, grants, injected_languages, publish, stage_extension,
    write_staged,
};

/// The tiny JSON grammar fe-syntax's runtime grammar tests load.
const WASM: &[u8] = include_bytes!("../../fe-syntax/tests/fixtures/tree-sitter-json.wasm");

/// The directories under `extensions/grammars/`, sorted, as they are now.
fn directories() -> Vec<String> {
    // Read at run time: a path compiled in can name a deleted snapshot.
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    let grammars = Path::new(&manifest).join("../../extensions/grammars");
    let mut languages: Vec<String> = fs::read_dir(&grammars)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_dir())
        .map(|entry| entry.file_name().into_string().unwrap())
        .collect();
    languages.sort();
    languages
}

/// The fixture grammar's files, by name, with `wasm` as its wasm, under a
/// language nothing else registers: `kotlin` and the rest may be built in, and
/// their queries are for their own grammars.
fn files(wasm: &[u8]) -> Vec<(&str, &[u8])> {
    vec![
        ("tree-sitter-json.wasm", wasm),
        ("highlights.scm", b"(string) @string\n"),
        ("injections.scm", b"; no injections\n"),
        ("extension.toml", FIXTURE_TOML.as_bytes()),
    ]
}

const FIXTURE_TOML: &str = "\
id = \"tenmoku.grammar_jsoni\"
name = \"JSONI syntax\"
version = \"1.0.0\"
license = \"MIT\"

[engine]
tenmoku = \"1.0\"

[[contributes.grammars]]
language = \"jsoni\"
wasm = \"tree-sitter-json.wasm\"
extensions = [\"jsoni\"]
highlights = \"highlights.scm\"
injections = \"injections.scm\"
";

/// `dir`'s entries, by name; none if it's missing.
fn entries(dir: &Path) -> Vec<String> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = read
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// Installs the fixture under `extensions` with `bytes` as its wasm, which
/// must hash as the fixture's, as an index entry's would be checked.
fn install(extensions: &Path, bytes: &[u8]) -> Result<PathBuf, InstallError> {
    let staged = Staged {
        language: "jsoni".to_owned(),
        files: files(bytes),
        verify: Box::new(move || {
            let actual = sha256_hex(bytes);
            let expected = sha256_hex(WASM);
            if actual == expected {
                return Ok(());
            }
            Err(GrammarFetchError::HashMismatch { expected, actual }.into())
        }),
    };
    write_staged(staged, Vec::new(), &[], extensions, &|from, to| {
        fs::rename(from, to)
    })
}

#[gpui::test]
fn installs_a_loadable_extension(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    let user = state.path().join("settings.toml");
    fs::write(&user, "").unwrap();
    let (sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .settings_store(SettingsStore::load(Some(user), None).into_inner())
        .theme_mode(ThemeMode::Dark)
        .watch_with(batches);
    let config = crate::config::with_extensions(config, extensions.clone());
    cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();
    assert!(fe_syntax::language_by_name("jsoni").is_none());

    let dir = install(&extensions, WASM).unwrap();

    assert_eq!(dir, extensions.join("tenmoku.grammar_jsoni"));
    assert_eq!(entries(&extensions), ["tenmoku.grammar_jsoni"]);
    assert_eq!(
        entries(&dir),
        [
            "extension.toml",
            "highlights.scm",
            "injections.scm",
            "tree-sitter-json.wasm"
        ]
    );
    assert_eq!(
        fs::read_to_string(dir.join("extension.toml")).unwrap(),
        FIXTURE_TOML
    );
    assert_eq!(
        fs::read_to_string(dir.join("highlights.scm")).unwrap(),
        "(string) @string\n"
    );
    assert_eq!(
        fs::read_to_string(dir.join("injections.scm")).unwrap(),
        "; no injections\n"
    );
    assert_eq!(fs::read(dir.join("tree-sitter-json.wasm")).unwrap(), WASM);

    // What the watcher does when the directory appears. The loader's
    // extension_grammars makes the GrammarSpec, and registering it succeeds.
    sender.send(vec![extensions]);
    let language = (0..600)
        .find_map(|_| {
            cx.run_until_parked();
            let language = fe_syntax::language_by_name("jsoni");
            if language.is_none() {
                std::thread::sleep(Duration::from_millis(50));
            }
            language
        })
        .expect("the loader registers the installed grammar");
    assert_eq!(
        fe_syntax::language_name_for_path(Path::new("a.jsoni")),
        Some("jsoni")
    );

    let buffer = fe_buffer::Buffer::new("{\"a\": 12}");
    let mut syntax = fe_syntax::Syntax::new(language, &buffer.snapshot());
    assert!(syntax.parse_within(Duration::from_secs(10)));
    let highlighted: Vec<_> = (syntax.highlights(0..9).into_iter())
        .map(|span| span.range)
        .collect();
    assert_eq!(
        highlighted,
        [std::ops::Range { start: 1, end: 4 }],
        "the installed highlights.scm is used"
    );
}

#[test]
fn install_is_atomic() {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");

    // The installer checks the bytes itself, and a failure leaves nothing.
    let error = install(&extensions, b"not the wasm").unwrap_err();
    assert!(matches!(
        error,
        InstallError::Fetch(GrammarFetchError::HashMismatch { expected, actual })
            if expected == sha256_hex(WASM) && actual == sha256_hex(b"not the wasm")
    ));
    assert_eq!(entries(&extensions), Vec::<String>::new());

    // Over an existing directory, it's replaced whole and leaves nothing else.
    fs::create_dir_all(extensions.join("tenmoku.grammar_jsoni")).unwrap();
    let stale = extensions.join("tenmoku.grammar_jsoni/stale.wasm");
    fs::write(&stale, "stale").unwrap();
    let dir = install(&extensions, WASM).unwrap();
    assert!(!stale.exists());
    assert_eq!(fs::read(dir.join("tree-sitter-json.wasm")).unwrap(), WASM);
    assert_eq!(entries(&extensions), ["tenmoku.grammar_jsoni"]);

    // A failure over an existing directory leaves it as it was.
    install(&extensions, b"not the wasm").unwrap_err();
    assert_eq!(entries(&extensions), ["tenmoku.grammar_jsoni"]);
    assert_eq!(fs::read(dir.join("tree-sitter-json.wasm")).unwrap(), WASM);

    // Writing failing: the extensions directory can't be made.
    let file = state.path().join("a file");
    fs::write(&file, "").unwrap();
    assert!(matches!(
        install(&file.join("extensions"), WASM),
        Err(InstallError::Io { .. })
    ));
}

/// While it's staged, the loader's scan finds nothing to load: it looks for a
/// manifest directly in each directory under the extensions directory
/// (`extension_dirs` in kiln-core), and the staged one is a level further down.
#[test]
fn nothing_staged_looks_like_an_extension() {
    let extensions = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir_in(extensions.path()).unwrap();

    let staged = stage_extension("jsoni", &files(WASM), staging.path()).unwrap();

    assert_eq!(staged.parent(), Some(staging.path()));
    assert!(staged.join("extension.toml").exists());
    assert!(!staging.path().join("extension.toml").exists());
}

/// An installed extension and a staged replacement, with `rename` failing on
/// the calls `fails` names: 0 moves the old one aside, 1 the new one in, and 2
/// the old one back.
fn publish_failing(
    fails: &'static [usize],
) -> (tempfile::TempDir, PathBuf, Result<(), InstallError>) {
    let extensions = tempfile::tempdir().unwrap();
    let target = extensions.path().join("tenmoku.grammar_jsoni");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("old.wasm"), "old").unwrap();
    let staging = tempfile::Builder::new()
        .prefix(".staging-")
        .tempdir_in(extensions.path())
        .unwrap();
    let staged = stage_extension("jsoni", &files(WASM), staging.path()).unwrap();

    let calls = std::cell::Cell::new(0);
    let rename = |from: &Path, to: &Path| {
        let call = calls.replace(calls.get() + 1);
        if fails.contains(&call) {
            return Err(std::io::Error::other(format!("call {call} failed")));
        }
        fs::rename(from, to)
    };
    let result = publish(staging, &staged, &target, &rename);
    (extensions, target, result)
}

#[test]
fn a_failed_publish_puts_the_previous_extension_back() {
    for fails in [&[0][..], &[1]] {
        let (extensions, target, result) = publish_failing(fails);
        assert!(matches!(result, Err(InstallError::Io { .. })), "{fails:?}");
        assert_eq!(entries(&target), ["old.wasm"], "{fails:?}");
        assert_eq!(
            entries(extensions.path()),
            ["tenmoku.grammar_jsoni"],
            "{fails:?}: the temporary directory goes"
        );
    }
}

/// Neither rename back nor forward works: deleting the temporary directory
/// would lose the only copy of what was installed.
#[test]
fn a_failed_restore_keeps_the_previous_extension_and_says_where() {
    let (extensions, target, result) = publish_failing(&[1, 2]);
    let error = result.unwrap_err();
    let message = error.to_string();
    let InstallError::NotRestored {
        target: named,
        previous,
        source,
        restoring,
    } = error
    else {
        panic!("{message}");
    };
    assert_eq!(named, target);
    assert!(!target.exists());
    assert_eq!(entries(&previous), ["old.wasm"]);
    assert_eq!(fs::read(previous.join("old.wasm")).unwrap(), b"old");
    assert_eq!(source.to_string(), "call 1 failed");
    assert_eq!(restoring.to_string(), "call 2 failed");
    assert!(
        message.contains(previous.to_str().unwrap()),
        "the user can find it: {message}"
    );
    // The kept directory is still nothing the loader would load.
    let kept = previous.parent().unwrap();
    assert_eq!(kept.parent(), Some(extensions.path()));
    assert!(!kept.join("extension.toml").exists());
}

/// Replacing a linked directory, as a developer might have, replaces the link
/// and leaves what it points at.
#[cfg(unix)]
#[test]
fn a_symlinked_extension_is_replaced_not_followed() {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    let elsewhere = state.path().join("elsewhere");
    fs::create_dir_all(&extensions).unwrap();
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("kept"), "kept").unwrap();
    let target = extensions.join("tenmoku.grammar_jsoni");
    std::os::unix::fs::symlink(&elsewhere, &target).unwrap();

    install(&extensions, WASM).unwrap();

    assert!(!target.symlink_metadata().unwrap().is_symlink());
    assert_eq!(entries(&elsewhere), ["kept"]);
    assert_eq!(entries(&extensions), ["tenmoku.grammar_jsoni"]);
}

/// Each directory under `extensions/grammars/` has the files an install
/// writes, and an `extension.toml` naming its own language, wasm file and
/// queries.
#[test]
fn every_first_party_grammar_is_complete() {
    let directories = directories();
    assert!(!directories.is_empty());
    let root = Path::new(&std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../extensions/grammars");
    for language in directories {
        let dir = root.join(&language);
        let text = fs::read_to_string(dir.join("extension.toml")).unwrap();
        let manifest: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(
            manifest["id"].as_str(),
            Some(format!("tenmoku.grammar_{language}").as_str())
        );
        let contributed = manifest["contributes"]["grammars"]
            .as_array()
            .unwrap()
            .iter()
            .find(|grammar| grammar["language"].as_str() == Some(language.as_str()))
            .unwrap_or_else(|| panic!("{language}: its manifest contributes it"));
        let wasm = contributed["wasm"].as_str().unwrap();
        assert!(
            wasm.ends_with(".wasm") && !wasm.contains(['/', '\\']),
            "{language}: {wasm}"
        );
        assert_eq!(contributed["highlights"].as_str(), Some("highlights.scm"));
        assert_eq!(contributed["injections"].as_str(), Some("injections.scm"));
        assert!(
            !fs::read_to_string(dir.join("highlights.scm"))
                .unwrap()
                .is_empty()
        );
        assert!(dir.join("injections.scm").exists());
    }
}

/// What the extension loader says of an `extension.toml`: kiln's checks,
/// warnings too (an unknown key is one), and the grammars' own type, which
/// takes no key it doesn't know.
fn manifest_problems(extension_toml: &str) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("extension.toml"), extension_toml).unwrap();
    let api = kiln::core::ExtensionApi {
        app: fe_config::paths::app_id().short,
        version: kiln::core::ApiVersion { major: 1, minor: 1 },
        reserved: Vec::new(),
        kinds: vec![kiln::core::AppKind {
            name: "grammars",
            schema: schemars::schema_for!(Vec<crate::config::ExtensionGrammar>).to_value(),
        }],
        allowed_licenses: None,
        only_kinds: None,
    };
    let (manifest, diagnostics) = kiln::core::load_manifest(dir.path(), &api);
    let mut problems: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
    match manifest
        .as_ref()
        .and_then(|manifest| manifest.app.get("grammars"))
    {
        Some(grammars) => {
            if let Err(error) = grammars
                .clone()
                .try_into::<Vec<crate::config::ExtensionGrammar>>()
            {
                problems.push(error.to_string());
            }
        }
        None if manifest.is_some() => problems.push("it contributes no grammars".to_owned()),
        None => {}
    }
    problems
}

/// Each `extension.toml` is one the extension loader takes without a word, so
/// a language card's unknown key fails here.
#[test]
fn every_first_party_manifest_parses() {
    let root = Path::new(&std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../extensions/grammars");
    for language in directories() {
        let text = fs::read_to_string(root.join(&language).join("extension.toml")).unwrap();
        let problems = manifest_problems(&text);
        assert!(problems.is_empty(), "{language}: {problems:#?}");
    }
    assert_eq!(manifest_problems(FIXTURE_TOML), Vec::<String>::new());
}

#[test]
fn an_unknown_manifest_key_is_a_problem() {
    let in_grammar = FIXTURE_TOML.replace(
        "highlights = ",
        "highlight_query = \"highlights.scm\"\nhighlights = ",
    );
    let problems = manifest_problems(&in_grammar);
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("highlight_query")),
        "{problems:#?}"
    );
    let at_top = FIXTURE_TOML.replace("license = ", "licence = \"MIT\"\nlicense = ");
    let problems = manifest_problems(&at_top);
    assert!(
        problems.iter().any(|problem| problem.contains("licence")),
        "{problems:#?}"
    );
}

/// The release download's hosts, including the one GitHub redirects to now.
#[test]
fn downloads_reach_only_the_release_hosts() {
    let net = grants();
    let net = net.net_hosts();
    assert!(!net.any);
    assert_eq!(net.hosts.len(), GRANTED_HOSTS.len());
    for host in GRANTED_HOSTS {
        assert!(net.hosts.contains(&host), "{host}");
    }
}

const HOST_INJECTIONS: &str = "\
; Not this one: (#set! injection.language \"jsonc\")
((comment) @injection.content
  (#set! injection.language \"onlyq\"))
((string) @injection.content
  (#set! \"injection.language\"
\"pendingq\"))
((pair) @x (#set! @x injection.language \"jsoni\"))
((pair) @x (#set! injection.language @x \"betweenq\"))
((pair) @injection.content (#is? injection.language \"notq\"))
((pair) @injection.content (#set! injection.language \"a\" \"b\"))
((array) @injection.language @injection.content)
";

#[test]
fn a_query_names_the_languages_it_sets() {
    assert_eq!(
        injected_languages(HOST_INJECTIONS),
        ["onlyq", "pendingq", "jsoni", "betweenq"]
    );
    assert_eq!(injected_languages("; none\n"), Vec::<&str>::new());
}

/// `language`'s staged extension: the fixture's files under its name, with
/// nothing to check.
fn staged(language: &str) -> Staged<'static> {
    Staged {
        language: language.to_owned(),
        files: files(WASM),
        verify: Box::new(|| Ok(())),
    }
}

fn fs_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to)
}

/// An injected extension written before its host fails to follow comes out
/// again, and what the host found installed isn't relied on once it's gone.
#[test]
fn a_failed_host_takes_the_injected_extensions_back_out() {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    let failing = |from: &Path, to: &Path| {
        if to.ends_with("tenmoku.grammar_hostq") {
            return Err(std::io::Error::other("the disk is full"));
        }
        fs::rename(from, to)
    };
    let needed = ["onlyq".to_owned(), "deepq".to_owned()];
    let error = write_staged(
        staged("hostq"),
        vec![staged("onlyq"), staged("deepq")],
        &needed,
        &extensions,
        &failing,
    )
    .unwrap_err();
    assert!(matches!(error, InstallError::Io { .. }), "{error}");
    assert_eq!(entries(&extensions), Vec::<String>::new());
}

/// Two installs of languages injecting the same one: what the second found
/// installed, the first may have taken out again by the time it writes,
/// though the loader registered it meanwhile; it waits for the first, finds
/// that, and fails.
#[test]
fn an_install_never_relies_on_what_another_takes_back() {
    use std::sync::mpsc;

    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    let needed = vec!["onlyq".to_owned(), "deepq".to_owned()];

    // On its own: what it needs has gone, and it didn't bring it.
    let error = write_staged(
        staged("hostr"),
        Vec::new(),
        &needed,
        &extensions,
        &fs_rename,
    )
    .unwrap_err();
    assert!(
        matches!(&error, InstallError::Removed(l) if l == "onlyq"),
        "{error}"
    );
    assert_eq!(entries(&extensions), Vec::<String>::new());

    // hostq's install has put onlyq and deepq in, and fails to put itself
    // in, just as hostr's, which found them installed, writes.
    let other = std::cell::RefCell::new(None);
    let (sender, results) = mpsc::channel();
    let failing = |from: &Path, to: &Path| {
        if !to.ends_with("tenmoku.grammar_hostq") {
            return fs::rename(from, to);
        }
        assert_eq!(
            entries(&extensions).len(),
            2 + 1,
            "onlyq, deepq and staging"
        );
        // The loader may register onlyq now: it doesn't make onlyq stay.
        let spec = fe_syntax::GrammarSpec {
            language: "onlyq".into(),
            wasm: extensions.join("tenmoku.grammar_onlyq/tree-sitter-json.wasm"),
            ..fe_syntax::GrammarSpec::default()
        };
        fe_syntax::register_language(&spec).unwrap();
        assert!(fe_syntax::language_names().any(|name| name == "onlyq"));
        let (needed, extensions) = (needed.clone(), extensions.clone());
        let sender = sender.clone();
        *other.borrow_mut() = Some(std::thread::spawn(move || {
            let written = write_staged(
                staged("hostr"),
                Vec::new(),
                &needed,
                &extensions,
                &fs_rename,
            );
            sender.send(written).unwrap();
        }));
        assert!(
            results.recv_timeout(Duration::from_millis(100)).is_err(),
            "hostr's waits for hostq's"
        );
        Err(std::io::Error::other("the disk is full"))
    };
    let error = write_staged(
        staged("hostq"),
        vec![staged("onlyq"), staged("deepq")],
        &needed,
        &extensions,
        &failing,
    )
    .unwrap_err();
    assert!(matches!(error, InstallError::Io { .. }), "{error}");
    other.borrow_mut().take().unwrap().join().unwrap();
    let error = results.recv().unwrap().unwrap_err();
    assert!(
        matches!(&error, InstallError::Removed(l) if l == "onlyq"),
        "{error}"
    );
    assert_eq!(entries(&extensions), Vec::<String>::new());
    fe_syntax::unregister_language("onlyq");
}

/// A link to nothing isn't installed: it's written over.
#[cfg(unix)]
#[test]
fn a_dangling_link_is_replaced() {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    fs::create_dir_all(&extensions).unwrap();
    let link = extensions.join("tenmoku.grammar_onlyq");
    std::os::unix::fs::symlink(state.path().join("gone"), &link).unwrap();
    let needed = vec!["onlyq".to_owned()];
    write_staged(
        staged("hostq"),
        vec![staged("onlyq")],
        &needed,
        &extensions,
        &fs_rename,
    )
    .unwrap();
    assert_eq!(
        entries(&extensions),
        ["tenmoku.grammar_hostq", "tenmoku.grammar_onlyq"]
    );
    assert!(!link.symlink_metadata().unwrap().is_symlink());
    assert_eq!(fs::read(link.join("tree-sitter-json.wasm")).unwrap(), WASM);
}
