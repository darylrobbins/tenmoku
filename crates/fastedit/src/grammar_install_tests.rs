//! The installer, with fake fetchers: what it writes is an extension the real
//! loader registers, and a failure leaves the extensions directory as it was.

use std::{
    fs,
    future::{pending, ready},
    path::{Path, PathBuf},
    pin::pin,
    time::Duration,
};

use fe_config::SettingsStore;
use fe_net::{GrammarFetchError, NetError, sha256_hex};
use futures::{FutureExt as _, executor::block_on};
use gpui::TestAppContext;
use kiln::{Config, platform::config::batch_channel, ui::ThemeMode};

use crate::grammar_install::{
    ExtensionFiles, GRANTED_HOSTS, Grammar, InstallError, Pack, fetch_from_release,
    first_party_languages, grants, install_fetched, install_first_party, install_from_pack_dir,
    publish, stage_extension,
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

/// The fixture grammar, as the manifest would describe it, under a language
/// nothing else registers: `kotlin` and the rest are built in under the
/// default features, and their queries are for their own grammars.
fn fixture(language: &'static str, extension_toml: &'static str) -> Grammar {
    Grammar {
        language: language.to_owned(),
        url: "https://github.com/acme/grammars/releases/download/g1/tree-sitter-json.wasm".into(),
        file: "tree-sitter-json.wasm".into(),
        size: WASM.len() as u64,
        sha256: sha256_hex(WASM),
        files: ExtensionFiles {
            extension_toml,
            highlights: "(string) @string\n",
            injections: "; no injections\n",
        },
    }
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

fn grammar() -> Grammar {
    fixture("jsoni", FIXTURE_TOML)
}

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

fn install(grammar: &Grammar, extensions: &Path, bytes: &[u8]) -> Result<PathBuf, InstallError> {
    block_on(install_fetched(grammar, extensions, |_| {
        ready(Ok(bytes.to_vec()))
    }))
}

fn install_failing(
    grammar: &Grammar,
    extensions: &Path,
    error: GrammarFetchError,
) -> Result<PathBuf, InstallError> {
    block_on(install_fetched(grammar, extensions, |_| ready(Err(error))))
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

    let grammar = grammar();
    let dir = install(&grammar, &extensions, WASM).unwrap();

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
    let grammar = grammar();
    let refusals = [
        GrammarFetchError::HashMismatch {
            expected: grammar.sha256.clone(),
            actual: "0".repeat(64),
        },
        GrammarFetchError::Net(NetError::Connect("offline".into())),
        GrammarFetchError::Status(404),
    ];

    // Failures leave nothing, in a directory with nothing in it yet.
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    for refusal in &refusals {
        let error = install_failing(&grammar, &extensions, refusal.clone()).unwrap_err();
        assert!(matches!(error, InstallError::Fetch(e) if e == *refusal));
        assert_eq!(entries(&extensions), Vec::<String>::new());
    }

    // The installer checks the bytes itself, whatever the fetcher did.
    let error = install(&grammar, &extensions, b"not the wasm").unwrap_err();
    assert!(matches!(
        error,
        InstallError::Fetch(GrammarFetchError::HashMismatch { expected, actual })
            if expected == grammar.sha256 && actual == sha256_hex(b"not the wasm")
    ));
    assert_eq!(entries(&extensions), Vec::<String>::new());

    // Cancelled: the fetch is dropped while it's running.
    {
        let mut installing = pin!(install_fetched(&grammar, &extensions, |_| pending()));
        assert_eq!(
            (&mut installing).now_or_never().map(|_| ()),
            None,
            "the fetch hasn't finished"
        );
    }
    assert_eq!(entries(&extensions), Vec::<String>::new());

    // Over an existing directory, it's replaced whole and leaves nothing else.
    fs::create_dir_all(extensions.join("tenmoku.grammar_jsoni")).unwrap();
    let stale = extensions.join("tenmoku.grammar_jsoni/stale.wasm");
    fs::write(&stale, "stale").unwrap();
    let dir = install(&grammar, &extensions, WASM).unwrap();
    assert!(!stale.exists());
    assert_eq!(fs::read(dir.join("tree-sitter-json.wasm")).unwrap(), WASM);
    assert_eq!(entries(&extensions), ["tenmoku.grammar_jsoni"]);

    // A failure over an existing directory leaves it as it was.
    for refusal in &refusals {
        install_failing(&grammar, &extensions, refusal.clone()).unwrap_err();
        assert_eq!(entries(&extensions), ["tenmoku.grammar_jsoni"]);
        assert_eq!(fs::read(dir.join("tree-sitter-json.wasm")).unwrap(), WASM);
    }
    install(&grammar, &extensions, b"not the wasm").unwrap_err();
    assert_eq!(entries(&extensions), ["tenmoku.grammar_jsoni"]);
    assert_eq!(fs::read(dir.join("tree-sitter-json.wasm")).unwrap(), WASM);

    // Writing failing: the extensions directory can't be made.
    let file = state.path().join("a file");
    fs::write(&file, "").unwrap();
    assert!(matches!(
        install(&grammar, &file.join("extensions"), WASM),
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

    let staged = stage_extension(&grammar(), WASM, staging.path()).unwrap();

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
    let staged = stage_extension(&grammar(), WASM, staging.path()).unwrap();

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

    install(&grammar(), &extensions, WASM).unwrap();

    assert!(!target.symlink_metadata().unwrap().is_symlink());
    assert_eq!(entries(&elsewhere), ["kept"]);
    assert_eq!(entries(&extensions), ["tenmoku.grammar_jsoni"]);
}

#[test]
fn offline_fallback_reads_a_pack_dir() {
    let grammar = grammar();
    let state = tempfile::tempdir().unwrap();
    let pack = state.path().join("pack");
    let unpacked = pack.join("tenmoku.grammar_jsoni");
    fs::create_dir_all(&unpacked).unwrap();
    // A pack's other files don't matter: the app's own are written.
    fs::write(unpacked.join("extension.toml"), "not the manifest").unwrap();
    fs::write(unpacked.join("tree-sitter-json.wasm"), WASM).unwrap();

    let extensions = state.path().join("extensions");
    let dir = install_from_pack_dir(&grammar, &pack, &extensions).unwrap();
    assert_eq!(dir, extensions.join("tenmoku.grammar_jsoni"));
    assert_eq!(fs::read(dir.join("tree-sitter-json.wasm")).unwrap(), WASM);
    assert_eq!(
        fs::read_to_string(dir.join("extension.toml")).unwrap(),
        FIXTURE_TOML
    );

    // A wasm whose hash differs from the manifest's is refused.
    let extensions = state.path().join("elsewhere");
    fs::write(unpacked.join("tree-sitter-json.wasm"), b"another wasm").unwrap();
    let error = install_from_pack_dir(&grammar, &pack, &extensions).unwrap_err();
    assert!(matches!(
        error,
        InstallError::Fetch(GrammarFetchError::HashMismatch { expected, actual })
            if expected == grammar.sha256 && actual == sha256_hex(b"another wasm")
    ));
    assert_eq!(entries(&extensions), Vec::<String>::new());

    // One too big to be it isn't even read whole.
    let mut oversized = grammar.clone();
    oversized.size = 4;
    let error = install_from_pack_dir(&oversized, &pack, &extensions).unwrap_err();
    assert!(matches!(
        error,
        InstallError::Fetch(GrammarFetchError::TooLarge { max_bytes: 4 })
    ));
    assert_eq!(entries(&extensions), Vec::<String>::new());

    // Nor is a language the pack doesn't have an install.
    fs::remove_file(unpacked.join("tree-sitter-json.wasm")).unwrap();
    assert!(matches!(
        install_from_pack_dir(&grammar, &pack, &extensions),
        Err(InstallError::Io { .. })
    ));
    assert_eq!(entries(&extensions), Vec::<String>::new());
}

#[test]
fn an_unreleased_grammar_is_refused_everywhere() {
    let mut grammar = grammar();
    grammar.sha256 = String::new();
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");

    let mut fetched = false;
    let error = block_on(install_fetched(&grammar, &extensions, |_| {
        fetched = true;
        ready(Ok(WASM.to_vec()))
    }))
    .unwrap_err();
    assert!(matches!(
        error,
        InstallError::Fetch(GrammarFetchError::NotReleased)
    ));
    assert!(!fetched, "the fetcher isn't called");

    let error = install_from_pack_dir(&grammar, state.path(), &extensions).unwrap_err();
    assert!(matches!(
        error,
        InstallError::Fetch(GrammarFetchError::NotReleased)
    ));
    assert_eq!(entries(&extensions), Vec::<String>::new());
}

/// Each directory is compiled in, and its manifest and files agree with the
/// pack's; a recorded entry is for a directory, and names its wasm.
#[test]
fn every_first_party_grammar_is_complete() {
    let languages: Vec<&str> = first_party_languages().collect();
    assert!(!languages.is_empty());
    assert_eq!(
        languages,
        directories(),
        "build.rs compiles in each directory under extensions/grammars"
    );
    let pack = Pack::embedded().unwrap();
    for recorded in pack.recorded() {
        assert!(
            languages.contains(&recorded),
            "pack.toml records {recorded}, which has no directory"
        );
    }

    for language in languages {
        let grammar = pack.grammar(language).unwrap();
        let manifest: toml::Table = toml::from_str(grammar.files.extension_toml).unwrap();
        assert_eq!(
            manifest["id"].as_str(),
            Some(grammar.extension_id().as_str())
        );
        let contributed = &manifest["contributes"]["grammars"][0];
        assert_eq!(contributed["language"].as_str(), Some(language));
        assert_eq!(
            contributed["wasm"].as_str(),
            Some(grammar.file.as_str()),
            "{language}: pack.toml's file is its extension.toml's wasm"
        );
        assert_eq!(contributed["highlights"].as_str(), Some("highlights.scm"));
        assert_eq!(contributed["injections"].as_str(), Some("injections.scm"));
        assert_eq!(
            grammar.url.rsplit_once('/').map(|(_, file)| file),
            Some(grammar.file.as_str())
        );
        assert!(grammar.url.starts_with("https://github.com/"));
        assert!(!grammar.files.highlights.is_empty());

        // Their real manifests and files go in the layout the loader reads.
        let mut stubbed = grammar.clone();
        stubbed.sha256 = sha256_hex(b"stub");
        let state = tempfile::tempdir().unwrap();
        let dir = install(&stubbed, state.path(), b"stub").unwrap();
        assert_eq!(dir, state.path().join(grammar.extension_id()));
        assert_eq!(
            fs::read_to_string(dir.join("extension.toml")).unwrap(),
            grammar.files.extension_toml
        );
        assert_eq!(fs::read(dir.join(&grammar.file)).unwrap(), b"stub");
    }
    // No directory can have this name.
    assert!(matches!(
        pack.grammar("no/such-language"),
        Err(InstallError::UnknownLanguage(_))
    ));
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

/// Each compiled-in `extension.toml` is one the extension loader takes
/// without a word, so a language card's unknown key fails here.
#[test]
fn every_first_party_manifest_parses() {
    let pack = Pack::embedded().unwrap();
    for language in first_party_languages() {
        let grammar = pack.grammar(language).unwrap();
        let problems = manifest_problems(grammar.files.extension_toml);
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

/// A directory pack.toml has no entry for is offered, and refused, as an entry
/// with no hash is.
#[test]
fn a_language_with_no_entry_is_not_released() {
    let base = "base_url = \"https://github.com/a/b/releases/download/t\"\n";
    let language = first_party_languages().next().unwrap();
    let unrecorded = Pack::parse(base).unwrap();
    let file = unrecorded.grammar(language).unwrap().file;
    let empty = Pack::parse(&format!(
        "{base}[languages.{language}]\nfile = {file:?}\nsize = 0\nsha256 = \"\"\n"
    ))
    .unwrap();
    for pack in [&unrecorded, &empty] {
        let grammar = pack.grammar(language).unwrap();
        assert_eq!(grammar.file, file);
        assert_eq!(
            grammar.url,
            format!("https://github.com/a/b/releases/download/t/{file}")
        );
        assert_eq!((grammar.size, grammar.sha256.as_str()), (0, ""));
        let offer = (pack.catalog().into_iter())
            .find(|offer| offer.language == language)
            .expect("it's offered");
        assert_eq!((offer.size, offer.sha256.as_str()), (0, ""));

        let state = tempfile::tempdir().unwrap();
        let mut fetched = false;
        let error = block_on(install_fetched(&grammar, state.path(), |_| {
            fetched = true;
            ready(Ok(WASM.to_vec()))
        }))
        .unwrap_err();
        assert!(matches!(
            error,
            InstallError::Fetch(GrammarFetchError::NotReleased)
        ));
        assert!(!fetched);
        assert_eq!(entries(state.path()), Vec::<String>::new());
    }
}

#[test]
fn a_manifest_file_outside_the_directory_is_refused() {
    for file in [
        "../evil.wasm",
        "a/b.wasm",
        "a\\b.wasm",
        ".wasm",
        ".hidden.wasm",
        "extension.toml",
    ] {
        let text = format!(
            "base_url = \"https://github.com/a/b/releases/download/t\"\n\
             [languages.kotlin]\nfile = {file:?}\nsize = 0\nsha256 = \"\"\n"
        );
        assert!(
            matches!(Pack::parse(&text), Err(InstallError::Manifest(_))),
            "{file}"
        );
    }
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

/// Nothing is released yet, so the app's own path refuses without a socket;
/// and its future can go to a background task.
#[test]
fn the_real_install_refuses_while_nothing_is_released() {
    fn is_send<T: Send>(value: T) -> T {
        value
    }
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    let pack = Pack::embedded().unwrap();
    for language in first_party_languages().filter(|language| {
        // A release recorded later makes the check vacuous for that language.
        pack.grammar(language).unwrap().sha256.is_empty()
    }) {
        let installing = is_send(install_first_party(language, &extensions));
        assert!(matches!(
            block_on(installing),
            Err(InstallError::Fetch(GrammarFetchError::NotReleased))
        ));
    }
    assert_eq!(entries(&extensions), Vec::<String>::new());

    let grammar = pack.grammar("kotlin").unwrap();
    let mut unreleased = grammar.clone();
    unreleased.sha256 = String::new();
    let client = fe_net::Client::new(&grants());
    assert_eq!(
        block_on(fetch_from_release(&client, &unreleased)),
        Err(GrammarFetchError::NotReleased)
    );
}
