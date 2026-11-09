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
    first_party_languages, grants, injected_languages, install_fetched, install_first_party,
    install_first_party_with, install_from_pack_dir, offers_with, publish, stage_extension,
    write_set,
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

/// A pack of `first_party`, whose release has the fixture wasm, recorded as
/// `size` bytes, for each of `released`.
fn fixture_pack(
    first_party: Vec<(&'static str, ExtensionFiles)>,
    released: &[&str],
    size: u64,
) -> Pack {
    let mut text =
        "base_url = \"https://github.com/acme/grammars/releases/download/g1\"\n".to_owned();
    for language in released {
        text.push_str(&format!(
            "[languages.{language}]\nfile = \"tree-sitter-json.wasm\"\nsize = {size}\n\
             sha256 = \"{}\"\n",
            sha256_hex(WASM)
        ));
    }
    Pack::parse(&text)
        .unwrap()
        .with_first_party(first_party.leak())
}

/// The pack with just the fixture grammar, released as `size` bytes or not.
fn jsoni_pack(released: bool, size: u64) -> Pack {
    let files = grammar().files;
    let released: &[&str] = if released { &["jsoni"] } else { &[] };
    fixture_pack(vec![("jsoni", files)], released, size)
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
    let manifest = jsoni_pack(true, WASM.len() as u64);
    let dir = install_from_pack_dir(&manifest, "jsoni", &pack, &extensions).unwrap();
    assert_eq!(dir, extensions.join("tenmoku.grammar_jsoni"));
    assert_eq!(fs::read(dir.join("tree-sitter-json.wasm")).unwrap(), WASM);
    assert_eq!(
        fs::read_to_string(dir.join("extension.toml")).unwrap(),
        FIXTURE_TOML
    );

    // A wasm whose hash differs from the manifest's is refused.
    let extensions = state.path().join("elsewhere");
    fs::write(unpacked.join("tree-sitter-json.wasm"), b"another wasm").unwrap();
    let error = install_from_pack_dir(&manifest, "jsoni", &pack, &extensions).unwrap_err();
    assert!(matches!(
        error,
        InstallError::Fetch(GrammarFetchError::HashMismatch { expected, actual })
            if expected == grammar.sha256 && actual == sha256_hex(b"another wasm")
    ));
    assert_eq!(entries(&extensions), Vec::<String>::new());

    // One too big to be it isn't even read whole.
    let oversized = jsoni_pack(true, 4);
    let error = install_from_pack_dir(&oversized, "jsoni", &pack, &extensions).unwrap_err();
    assert!(matches!(
        error,
        InstallError::Fetch(GrammarFetchError::TooLarge { max_bytes: 4 })
    ));
    assert_eq!(entries(&extensions), Vec::<String>::new());

    // Nor is a language the pack doesn't have an install.
    fs::remove_file(unpacked.join("tree-sitter-json.wasm")).unwrap();
    assert!(matches!(
        install_from_pack_dir(&manifest, "jsoni", &pack, &extensions),
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

    let unreleased = jsoni_pack(false, 0);
    let error = install_from_pack_dir(&unreleased, "jsoni", state.path(), &extensions).unwrap_err();
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
        let offer = (pack.catalog(&|_| false).into_iter())
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

/// Injection-only languages: a fixture catalog where `hostq`, which has an
/// extension, injects `onlyq`, which has none and injects `deepq`, which has
/// none either; `hostq` also injects `pendingq`, which has none and isn't
/// released, and `jsoni`, which has an extension.
mod injection_only {
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
    };

    use std::time::Instant;

    use fe_workbench::GrammarOffers;
    use gpui::VisualTestContext;

    use super::*;
    use crate::grammar_install::{Download, OfferHooks};

    /// A manifest for `language`, the fixture wasm, claiming `extensions`.
    fn manifest(language: &str, extensions: &str) -> &'static str {
        format!(
            "id = \"tenmoku.grammar_{language}\"\nname = \"{language} syntax\"\n\
             version = \"1.0.0\"\nlicense = \"MIT\"\n\n[engine]\ntenmoku = \"1.0\"\n\n\
             [[contributes.grammars]]\nlanguage = \"{language}\"\n\
             wasm = \"tree-sitter-json.wasm\"\nextensions = {extensions}\n\
             highlights = \"highlights.scm\"\ninjections = \"injections.scm\"\n"
        )
        .leak()
    }

    fn files(language: &str, extensions: &str, injections: &'static str) -> ExtensionFiles {
        ExtensionFiles {
            extension_toml: manifest(language, extensions),
            highlights: "(string) @string\n",
            injections,
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

    /// The fixture catalog, released but for `pendingq` and whatever
    /// `unreleased` names.
    fn pack_without(unreleased: &[&str]) -> Pack {
        let first_party = vec![
            ("deepq", files("deepq", "[]", "; none\n")),
            ("hostq", files("hostq", "[\"hostq\"]", HOST_INJECTIONS)),
            (
                "hostr",
                files(
                    "hostr",
                    "[\"hostr\"]",
                    "((string) @injection.content (#set! injection.language \"onlyq\" @injection.content))\n",
                ),
            ),
            ("jsoni", grammar().files),
            (
                "onlyq",
                files(
                    "onlyq",
                    "[]",
                    "((string) @injection.content (#set! injection.language \"deepq\"))\n",
                ),
            ),
            ("pendingq", files("pendingq", "[]", "; none\n")),
        ];
        let released: Vec<&str> = ["deepq", "hostq", "hostr", "jsoni", "onlyq"]
            .into_iter()
            .filter(|language| !unreleased.contains(language))
            .collect();
        fixture_pack(first_party, &released, WASM.len() as u64)
    }

    fn pack() -> Pack {
        pack_without(&[])
    }

    const ALL_THREE: [&str; 3] = [
        "tenmoku.grammar_deepq",
        "tenmoku.grammar_hostq",
        "tenmoku.grammar_onlyq",
    ];

    fn languages(set: &[Grammar]) -> Vec<&str> {
        set.iter()
            .map(|grammar| grammar.language.as_str())
            .collect()
    }

    #[test]
    fn a_query_names_the_languages_it_sets() {
        assert_eq!(
            injected_languages(HOST_INJECTIONS),
            ["onlyq", "pendingq", "jsoni", "betweenq"]
        );
        assert_eq!(injected_languages("; none\n"), Vec::<&str>::new());
    }

    #[test]
    fn a_language_brings_the_released_injection_only_languages_it_injects() {
        let pack = pack();
        let none = |_: &str| false;
        assert_eq!(
            languages(&pack.with_injected("hostq", &none).unwrap()),
            ["hostq", "onlyq", "deepq"]
        );
        // An installed one isn't brought, but what it injects still is.
        let onlyq = |language: &str| language == "onlyq";
        assert_eq!(
            languages(&pack.with_injected("hostq", &onlyq).unwrap()),
            ["hostq", "deepq"]
        );
        assert_eq!(
            languages(&pack.with_injected("jsoni", &none).unwrap()),
            ["jsoni"]
        );
        // hostr names onlyq with its capture last.
        assert_eq!(
            languages(&pack.with_injected("hostr", &none).unwrap()),
            ["hostr", "onlyq", "deepq"]
        );
    }

    #[test]
    fn an_injection_only_language_is_not_offered_and_its_size_is_its_injectors() {
        let pack = pack();
        let size = WASM.len() as u64;
        let catalog = pack.catalog(&|_| false);
        let offered: Vec<(&str, u64)> = (catalog.iter())
            .map(|offer| (offer.language.as_str(), offer.size))
            .collect();
        assert_eq!(
            offered,
            [("hostq", 3 * size), ("hostr", 3 * size), ("jsoni", size)]
        );

        let catalog = pack.catalog(&|language| language == "deepq");
        assert_eq!(catalog[0].size, 2 * size);
    }

    /// What a fixture fetch gives.
    type Fetched = std::future::Ready<Result<Vec<u8>, GrammarFetchError>>;

    /// The languages a fetcher was asked for.
    type Asked = std::rc::Rc<RefCell<Vec<String>>>;

    /// A fetcher of the fixture wasm, but `wrong` for `bad`, keeping the
    /// languages it was asked for.
    fn fetcher(
        bad: &'static str,
        wrong: Result<Vec<u8>, GrammarFetchError>,
    ) -> (impl Fn(&Grammar) -> Fetched, Asked) {
        let asked = std::rc::Rc::new(RefCell::new(Vec::new()));
        let log = asked.clone();
        let fetch = move |grammar: &Grammar| {
            log.borrow_mut().push(grammar.language.clone());
            ready(if grammar.language == bad {
                wrong.clone()
            } else {
                Ok(WASM.to_vec())
            })
        };
        (fetch, asked)
    }

    #[test]
    fn install_first_party_installs_what_the_language_brings() {
        let pack = pack();
        let state = tempfile::tempdir().unwrap();
        let extensions = state.path().join("extensions");
        let (fetch, asked) = fetcher("", Ok(Vec::new()));

        let dir = block_on(install_first_party_with(
            &pack,
            "hostq",
            &extensions,
            &fetch,
        ))
        .unwrap();
        assert_eq!(dir, extensions.join("tenmoku.grammar_hostq"));
        assert_eq!(entries(&extensions), ALL_THREE);
        assert_eq!(*asked.borrow(), ["hostq", "onlyq", "deepq"]);
        let onlyq = extensions.join("tenmoku.grammar_onlyq");
        assert_eq!(fs::read(onlyq.join("tree-sitter-json.wasm")).unwrap(), WASM);
        assert_eq!(
            fs::read_to_string(onlyq.join("extension.toml")).unwrap(),
            manifest("onlyq", "[]")
        );

        // Installed, they aren't fetched again.
        asked.borrow_mut().clear();
        block_on(install_first_party_with(
            &pack,
            "hostq",
            &extensions,
            &fetch,
        ))
        .unwrap();
        assert_eq!(*asked.borrow(), ["hostq"]);
        assert_eq!(entries(&extensions), ALL_THREE);
    }

    #[test]
    fn an_unreleased_injection_only_language_is_left_out() {
        let pack = pack_without(&["deepq"]);
        let state = tempfile::tempdir().unwrap();
        let (fetch, asked) = fetcher("", Ok(Vec::new()));
        block_on(install_first_party_with(
            &pack,
            "hostq",
            state.path(),
            &fetch,
        ))
        .unwrap();
        assert_eq!(*asked.borrow(), ["hostq", "onlyq"]);
        assert_eq!(
            entries(state.path()),
            ["tenmoku.grammar_hostq", "tenmoku.grammar_onlyq"]
        );
    }

    #[test]
    fn a_failing_injection_only_language_fails_the_install_with_nothing_written() {
        let pack = pack();
        let state = tempfile::tempdir().unwrap();
        let extensions = state.path().join("extensions");
        let (fetch, _) = fetcher("deepq", Err(GrammarFetchError::Status(404)));
        let error = block_on(install_first_party_with(
            &pack,
            "hostq",
            &extensions,
            &fetch,
        ))
        .unwrap_err();
        assert!(matches!(
            error,
            InstallError::Fetch(GrammarFetchError::Status(404))
        ));
        assert_eq!(entries(&extensions), Vec::<String>::new());

        let (fetch, _) = fetcher("onlyq", Ok(b"not the wasm".to_vec()));
        let error = block_on(install_first_party_with(
            &pack,
            "hostq",
            &extensions,
            &fetch,
        ))
        .unwrap_err();
        assert!(matches!(
            error,
            InstallError::Fetch(GrammarFetchError::HashMismatch { actual, .. })
                if actual == sha256_hex(b"not the wasm")
        ));
        assert_eq!(entries(&extensions), Vec::<String>::new());

        // The language's own extension can't go in after theirs did: they come
        // out again.
        let set = pack.with_injected("hostq", &|_| false).unwrap();
        let injected: Vec<(Grammar, Vec<u8>)> = (set[1..].iter())
            .map(|g| (g.clone(), WASM.to_vec()))
            .collect();
        let rename = |from: &Path, to: &Path| {
            if to.ends_with("tenmoku.grammar_hostq") {
                return Err(std::io::Error::other("the disk is full"));
            }
            fs::rename(from, to)
        };
        let needed = languages(&set[1..])
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let error = write_set(&set[0], WASM, &injected, &needed, &extensions, &rename).unwrap_err();
        assert!(matches!(error, InstallError::Io { .. }), "{error}");
        assert_eq!(entries(&extensions), Vec::<String>::new());
    }

    #[test]
    fn the_language_pack_installs_what_the_language_brings() {
        let pack = pack();
        let state = tempfile::tempdir().unwrap();
        let unpacked = state.path().join("pack");
        for language in ["hostq", "onlyq", "deepq"] {
            let dir = unpacked.join(format!("tenmoku.grammar_{language}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("tree-sitter-json.wasm"), WASM).unwrap();
        }

        // One missing from the pack fails the install.
        let extensions = state.path().join("elsewhere");
        let deepq = unpacked.join("tenmoku.grammar_deepq/tree-sitter-json.wasm");
        fs::rename(&deepq, state.path().join("aside.wasm")).unwrap();
        assert!(matches!(
            install_from_pack_dir(&pack, "hostq", &unpacked, &extensions),
            Err(InstallError::Io { path, .. }) if path == deepq
        ));
        assert_eq!(entries(&extensions), Vec::<String>::new());
        fs::rename(state.path().join("aside.wasm"), &deepq).unwrap();

        let extensions = state.path().join("extensions");
        install_from_pack_dir(&pack, "hostq", &unpacked, &extensions).unwrap();
        assert_eq!(entries(&extensions), ALL_THREE);
    }

    /// Hooks fetching with `fetch`, with the languages registered once
    /// `registered` holds, keeping the offers each install republishes.
    fn hooks(
        fetch: impl Fn(&Grammar) -> Result<Vec<u8>, GrammarFetchError> + Send + Sync + 'static,
        registered: Arc<AtomicBool>,
    ) -> (OfferHooks, Rc<RefCell<Vec<GrammarOffers>>>) {
        let republished = Rc::new(RefCell::new(Vec::new()));
        let kept = republished.clone();
        let hooks = OfferHooks {
            fetch: Arc::new(move |grammar: &Grammar| -> Download {
                Box::pin(ready(fetch(grammar)))
            }),
            registered: Rc::new(move |_: &str, _: &gpui::App| registered.load(Ordering::SeqCst)),
            republish: Rc::new(move |offers: GrammarOffers, _: &mut gpui::App| {
                kept.borrow_mut().push(offers);
            }),
        };
        (hooks, republished)
    }

    fn fs_rename(from: &Path, to: &Path) -> std::io::Result<()> {
        fs::rename(from, to)
    }

    fn size_of(offers: &GrammarOffers, language: &str) -> u64 {
        (offers.catalog.iter())
            .find(|offer| offer.language == language)
            .unwrap()
            .size
    }

    #[gpui::test]
    fn the_prompts_install_installs_what_the_language_brings(cx: &mut TestAppContext) {
        let state = tempfile::tempdir().unwrap();
        let extensions = state.path().join("extensions");
        let size = WASM.len() as u64;
        let registered = Arc::new(AtomicBool::new(true));
        let (failing, _) = hooks(
            |grammar| match grammar.language.as_str() {
                "deepq" => Err(GrammarFetchError::Status(404)),
                _ => Ok(WASM.to_vec()),
            },
            registered.clone(),
        );
        let failing = offers_with(Arc::new(pack()), failing, extensions.clone(), None);
        let host = (failing.catalog.iter())
            .find(|offer| offer.language == "hostq")
            .unwrap();
        assert_eq!(host.size, 3 * size);
        let fetching = cx.update(|cx| (failing.fetch)(host, cx));
        let failure = cx.foreground_executor().block_test(fetching).unwrap_err();
        assert!(failure.reason.contains("404"), "{failure:?}");
        assert_eq!(entries(&extensions), Vec::<String>::new());

        let (hooks, republished) = hooks(|_| Ok(WASM.to_vec()), registered);
        let offers = offers_with(Arc::new(pack()), hooks, extensions.clone(), None);
        let host = (offers.catalog.iter())
            .find(|offer| offer.language == "hostq")
            .unwrap();
        let fetching = cx.update(|cx| (offers.fetch)(host, cx));
        let bundled = cx.foreground_executor().block_test(fetching).unwrap();
        assert_eq!(
            entries(&extensions),
            Vec::<String>::new(),
            "fetching writes nothing"
        );
        let writing = cx.update(|cx| (offers.install)(host, bundled, cx));
        cx.foreground_executor().block_test(writing).unwrap();
        assert_eq!(entries(&extensions), ALL_THREE);
        assert!((offers.installed)("onlyq"));

        // hostr injects onlyq too: the offers the install put in place of
        // these no longer count it, nor what it injects.
        assert_eq!(size_of(&offers, "hostr"), 3 * size);
        let republished = republished.borrow();
        assert_eq!(republished.len(), 1);
        assert_eq!(size_of(&republished[0], "hostr"), size);
    }

    /// The loader registers each extension on its own, perhaps the language
    /// before what it injects: the install isn't done, and the workbench
    /// doesn't highlight its tabs, until all it injects has registered, that
    /// it found installed too.
    #[gpui::test]
    fn the_prompts_install_waits_for_what_it_injects_to_register(cx: &mut TestAppContext) {
        let state = tempfile::tempdir().unwrap();
        let extensions = state.path().join("extensions");
        let registered = Arc::new(AtomicBool::new(false));
        let (hooks, _) = hooks(|_| Ok(WASM.to_vec()), registered.clone());
        let offers = offers_with(Arc::new(pack()), hooks, extensions.clone(), None);
        let installs = install_both(cx, &offers);
        let mut installs = installs.into_iter().map(Box::pin).collect::<Vec<_>>();
        // hostr's found onlyq and deepq there, written by hostq's.
        assert_eq!(
            entries(&extensions),
            [
                "tenmoku.grammar_deepq",
                "tenmoku.grammar_hostq",
                "tenmoku.grammar_hostr",
                "tenmoku.grammar_onlyq"
            ]
        );
        cx.executor().advance_clock(Duration::from_secs(50));
        cx.run_until_parked();
        for install in &mut installs {
            assert!(install.as_mut().now_or_never().is_none(), "still waiting");
        }

        registered.store(true, Ordering::SeqCst);
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        for install in &mut installs {
            let done = install.as_mut().now_or_never().expect("done");
            assert!(done.is_ok(), "{done:?}");
        }
    }

    /// Installs hostq, then hostr, through the prompt's steps: their install
    /// tasks, once each has written.
    fn install_both(
        cx: &mut TestAppContext,
        offers: &GrammarOffers,
    ) -> Vec<gpui::Task<Result<(), fe_workbench::GrammarInstallError>>> {
        install_each(cx, offers, &["hostq", "hostr"])
    }

    /// Installs each of `languages` in turn through the prompt's steps: their
    /// install tasks, once each has written.
    fn install_each(
        cx: &mut TestAppContext,
        offers: &GrammarOffers,
        languages: &[&str],
    ) -> Vec<gpui::Task<Result<(), fe_workbench::GrammarInstallError>>> {
        let mut installs = Vec::new();
        for language in languages {
            let host = (offers.catalog.iter())
                .find(|offer| offer.language == *language)
                .unwrap()
                .clone();
            let fetching = cx.update(|cx| (offers.fetch)(&host, cx));
            let bundled = cx.foreground_executor().block_test(fetching).unwrap();
            installs.push(cx.update(|cx| (offers.install)(&host, bundled, cx)));
            cx.run_until_parked();
        }
        installs
    }

    /// A workbench over `root`, in a window of its own.
    fn window(
        cx: &mut TestAppContext,
        root: &Path,
    ) -> (gpui::Entity<fe_workbench::Workbench>, VisualTestContext) {
        let launch = fe_workbench::Launch {
            root: Some(root.to_owned()),
            file: None,
        };
        let window = cx.add_window(move |window, cx| {
            let theme = fe_gpui::EditorTheme::default_dark(cx);
            let settings = fe_gpui::EditorSettings::default();
            fe_workbench::Workbench::new(launch, theme, settings, window, cx)
        });
        let workbench = window.root(cx).unwrap();
        (workbench, VisualTestContext::from_window(*window, cx))
    }

    fn draw(cx: &mut VisualTestContext) {
        for _ in 0..2 {
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    }

    /// The editor of `path`, opened in `workbench`, once its language has
    /// loaded, with that language's name.
    fn open(
        workbench: &gpui::Entity<fe_workbench::Workbench>,
        path: &Path,
        cx: &mut VisualTestContext,
    ) -> (gpui::Entity<fe_gpui::EditorView>, Option<&'static str>) {
        let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
        let path = path.to_owned();
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            draw(cx);
            let editor = pane.read_with(cx, |pane, _| pane.active_editor().cloned());
            if let Some(editor) = editor
                && !editor.read_with(cx, |editor, _| editor.is_loading_language())
            {
                let language = editor.read_with(cx, |editor, _| {
                    editor.syntax().map(|syntax| syntax.language().name())
                });
                return (editor, language);
            }
            assert!(Instant::now() < deadline, "the file didn't open");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// `["[1]"]`: the string's text is JSON too, its number at 3..4.
    const INJECTING: &str = "[\"[1]\"]\n";

    /// Whether `editor` highlights the number in [`INJECTING`]'s string,
    /// which only the language it injects does.
    fn highlights_injected(
        editor: &gpui::Entity<fe_gpui::EditorView>,
        cx: &mut VisualTestContext,
    ) -> bool {
        editor.read_with(cx, |editor, _| {
            (editor.syntax()).is_some_and(|syntax| {
                (syntax.highlights(0..INJECTING.len()).iter()).any(|span| span.range == (3..4))
            })
        })
    }

    /// Two languages injecting one that registers only after the deadline:
    /// each install warns and goes on waiting, then finishes, and the tabs of
    /// both, which opened while it hadn't registered, highlight what they
    /// inject.
    #[gpui::test]
    fn tabs_highlight_what_they_inject_once_it_registers_after_the_deadline(
        cx: &mut TestAppContext,
    ) {
        // The project's watcher thread wakes the event loop.
        cx.executor().allow_parking();
        let state = tempfile::tempdir().unwrap();
        let extensions = state.path().join("extensions");
        // The app, its extension loader on a directory of its own: the test
        // registers what's installed, when it chooses, as the loader would.
        let user = state.path().join("settings.toml");
        fs::write(&user, "").unwrap();
        let (_sender, batches) = batch_channel();
        let config = Config::new("dev.fastedit.test", "fastedit")
            .commands(crate::config::register_commands)
            .settings_store(SettingsStore::load(Some(user), None).into_inner())
            .theme_mode(ThemeMode::Dark)
            .watch_with(batches);
        let config = crate::config::with_extensions(config, state.path().join("loaded"));
        cx.update(|cx| kiln::init(cx, config));
        cx.run_until_parked();

        let host = |language: &'static str| ExtensionFiles {
            extension_toml: manifest(language, &format!("[\"{language}\"]")),
            highlights: "(number) @number\n",
            injections: "((string_content) @injection.content \
                         (#set! injection.language \"onlya\"))\n",
        };
        let first_party = vec![
            ("hosta", host("hosta")),
            ("hostb", host("hostb")),
            (
                "onlya",
                ExtensionFiles {
                    extension_toml: manifest("onlya", "[]"),
                    highlights: "(number) @number\n",
                    injections: "; none\n",
                },
            ),
        ];
        let released = ["hosta", "hostb", "onlya"];
        let pack = fixture_pack(first_party, &released, WASM.len() as u64);
        let hooks = OfferHooks {
            fetch: Arc::new(|_: &Grammar| -> Download { Box::pin(ready(Ok(WASM.to_vec()))) }),
            registered: Rc::new(|language: &str, _: &gpui::App| {
                fe_syntax::language_names().any(|name| name == language)
            }),
            republish: Rc::new(|_: GrammarOffers, _: &mut gpui::App| {}),
        };
        let offers = offers_with(Arc::new(pack), hooks, extensions.clone(), None);
        let installs = install_each(cx, &offers, &["hosta", "hostb"]);
        let mut installs: Vec<_> = installs.into_iter().map(Box::pin).collect();
        let register = |language: &str, claims: Vec<String>| {
            let dir = extensions.join(format!("tenmoku.grammar_{language}"));
            let spec = fe_syntax::GrammarSpec {
                language: language.to_owned(),
                wasm: dir.join("tree-sitter-json.wasm"),
                extensions: claims,
                highlights: Some(dir.join("highlights.scm")),
                injections: Some(dir.join("injections.scm")),
                ..fe_syntax::GrammarSpec::default()
            };
            fe_syntax::register_language(&spec).unwrap();
        };
        // The loader registers the two first.
        register("hosta", vec!["hosta".into()]);
        register("hostb", vec!["hostb".into()]);

        let project = tempfile::tempdir().unwrap();
        let root = project.path().canonicalize().unwrap();
        let files = [("a.hosta", "hosta"), ("b.hostb", "hostb")];
        for (file, _) in files {
            fs::write(root.join(file), INJECTING).unwrap();
        }
        let (workbench, mut window_cx) = window(cx, &root);
        let vcx = &mut window_cx;
        let editors = files.map(|(file, language)| {
            let (editor, opened) = open(&workbench, &root.join(file), vcx);
            assert_eq!(opened, Some(language));
            editor
        });
        for editor in &editors {
            assert!(!highlights_injected(editor, vcx), "onlya isn't registered");
        }

        // Past the deadline, they go on waiting.
        vcx.executor().advance_clock(Duration::from_secs(90));
        draw(vcx);
        for install in &mut installs {
            assert!(install.as_mut().now_or_never().is_none(), "still waiting");
        }

        register("onlya", Vec::new());
        vcx.executor().advance_clock(Duration::from_secs(2));
        draw(vcx);
        for install in &mut installs {
            let done = install.as_mut().now_or_never().expect("done");
            assert!(done.is_ok(), "{done:?}");
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        for ((file, _), editor) in files.iter().zip(&editors) {
            // A tab parses once it's shown.
            let (shown, _) = open(&workbench, &root.join(file), vcx);
            assert_eq!(&shown, editor);
            while !highlights_injected(editor, vcx) {
                assert!(
                    Instant::now() < deadline,
                    "{file}: the injection never highlighted"
                );
                draw(vcx);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    /// Two installs of languages injecting the same one: what the second
    /// found installed, the first may have taken out again by the time it
    /// writes, though the loader registered it meanwhile; it waits for the
    /// first, finds that, and fails.
    #[test]
    fn an_install_never_relies_on_what_another_takes_back() {
        let pack = pack();
        let state = tempfile::tempdir().unwrap();
        let extensions = state.path().join("extensions");
        let set = pack.with_injected("hostq", &|_| false).unwrap();
        let needed: Vec<String> = languages(&set[1..])
            .into_iter()
            .map(str::to_owned)
            .collect();
        let injected: Vec<(Grammar, Vec<u8>)> = (set[1..].iter())
            .map(|g| (g.clone(), WASM.to_vec()))
            .collect();
        let hostr = pack.grammar("hostr").unwrap();

        // On its own: what it needs has gone, and it didn't download it.
        let error = write_set(&hostr, WASM, &[], &needed, &extensions, &fs_rename).unwrap_err();
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
            let (hostr, needed, extensions) = (hostr.clone(), needed.clone(), extensions.clone());
            let sender = sender.clone();
            *other.borrow_mut() = Some(std::thread::spawn(move || {
                let written = write_set(&hostr, WASM, &[], &needed, &extensions, &fs_rename);
                sender.send(written).unwrap();
            }));
            assert!(
                results.recv_timeout(Duration::from_millis(100)).is_err(),
                "hostr's waits for hostq's"
            );
            Err(std::io::Error::other("the disk is full"))
        };
        let error =
            write_set(&set[0], WASM, &injected, &needed, &extensions, &failing).unwrap_err();
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

    /// A link to nothing isn't installed: it's downloaded and replaced.
    #[cfg(unix)]
    #[test]
    fn a_dangling_link_is_replaced() {
        let pack = pack();
        let state = tempfile::tempdir().unwrap();
        let extensions = state.path().join("extensions");
        fs::create_dir_all(&extensions).unwrap();
        let link = extensions.join("tenmoku.grammar_onlyq");
        std::os::unix::fs::symlink(state.path().join("gone"), &link).unwrap();
        let (fetch, asked) = fetcher("", Ok(Vec::new()));
        block_on(install_first_party_with(
            &pack,
            "hostq",
            &extensions,
            &fetch,
        ))
        .unwrap();
        assert_eq!(*asked.borrow(), ["hostq", "onlyq", "deepq"]);
        assert_eq!(entries(&extensions), ALL_THREE);
        assert!(!link.symlink_metadata().unwrap().is_symlink());
        assert_eq!(fs::read(link.join("tree-sitter-json.wasm")).unwrap(), WASM);
    }
}
