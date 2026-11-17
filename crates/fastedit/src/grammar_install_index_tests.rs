//! Installing from the grammar index: every file of an entry fetched, checked
//! against the entry and its manifest, and written whole or not at all, with
//! fake fetches over a map of URL to bytes; and, ignored, one real language
//! from the live index.

use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use fe_config::SettingsStore;
use fe_net::GrammarFetchError;
use fe_registry::grammars::{self, Engine, FileEntry, Index, Language, Pack};
use futures::{FutureExt as _, executor::block_on};
use gpui::TestAppContext;
use kiln::{Config, platform::config::batch_channel, ui::ThemeMode};

use crate::grammar_install::{
    FetchFile, Grammar, IndexHooks, InstallError, fetch_entry_set, fetch_file, index_offers_with,
    install_entry, write_entry_bundle,
};
use crate::grammar_registry::{Catalog, EngineSupport};

/// The tiny JSON grammar fe-syntax's runtime grammar tests load.
const WASM: &[u8] = include_bytes!("../../fe-syntax/tests/fixtures/tree-sitter-json.wasm");

const TEMPLATE: &str = "https://github.com/acme/registry/releases/download/{tag}/{file}";

/// The fixture's manifest under `language`: its id, its grammar's language
/// and wasm, and the extension `language` itself.
fn manifest(id: &str, language: &str, wasm: &str) -> String {
    format!(
        "id = \"{id}\"\nname = \"{language} syntax\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
         [engine]\ntenmoku = \"1.0\"\n\n[[contributes.grammars]]\nlanguage = \"{language}\"\n\
         wasm = \"{wasm}\"\nextensions = [\"{language}\"]\nhighlights = \"highlights.scm\"\n\
         injections = \"injections.scm\"\n"
    )
}

/// The files of the fixture grammar as `language`, with `injections` as its
/// injections query: sorted by name, as an entry lists them.
fn fixture_files(language: &str, injections: &str) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("LICENSE".into(), b"MIT\n".to_vec()),
        (
            "extension.toml".into(),
            manifest(
                &format!("tenmoku.grammar_{language}"),
                language,
                "tree-sitter-json.wasm",
            )
            .into_bytes(),
        ),
        ("highlights.scm".into(), b"(string) @string\n".to_vec()),
        ("injections.scm".into(), injections.as_bytes().to_vec()),
        ("tree-sitter-json.wasm".into(), WASM.to_vec()),
    ];
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

/// An engine this app loads.
fn engine() -> Engine {
    Engine {
        tenmoku: "1.0".to_owned(),
        abi: tree_sitter::LANGUAGE_VERSION as u32,
        captures: grammars::CAPTURES,
    }
}

/// An entry for `files`, claiming `extensions`, in `packs`.
fn entry(
    language: &str,
    files: &[(String, Vec<u8>)],
    extensions: &[&str],
    packs: &[&str],
    engine: Engine,
) -> Language {
    let files: Vec<FileEntry> = (files.iter())
        .map(|(name, bytes)| FileEntry::of(name, bytes))
        .collect();
    let digest = grammars::digest(&files);
    Language {
        name: language.to_uppercase(),
        version: "1.0.0".to_owned(),
        tag: grammars::tag(language, &digest),
        digest,
        extensions: extensions.iter().map(|e| (*e).to_owned()).collect(),
        file_names: Vec::new(),
        shebangs: Vec::new(),
        packs: packs.iter().map(|p| (*p).to_owned()).collect(),
        license: "MIT".to_owned(),
        engine,
        files,
    }
}

/// A catalog of `languages`, each pack listing the languages that list it.
fn catalog(languages: Vec<(&str, Language)>) -> Catalog {
    let languages: BTreeMap<String, Language> = (languages.into_iter())
        .map(|(key, language)| (key.to_owned(), language))
        .collect();
    let mut packs: BTreeMap<String, Pack> = BTreeMap::new();
    for (key, language) in &languages {
        for pack in &language.packs {
            let pack = packs.entry(pack.clone()).or_insert_with(|| Pack {
                name: pack.to_uppercase(),
                description: String::new(),
                languages: Vec::new(),
                tarball: None,
            });
            pack.languages.push(key.clone());
        }
    }
    let index = Index {
        schema: grammars::SCHEMA,
        serial: 1,
        url_template: TEMPLATE.to_owned(),
        packs,
        languages,
    };
    Catalog::new(index, &EngineSupport::running())
}

/// A fetch serving `served`, by URL, recording each URL asked for.
fn fetcher(served: HashMap<String, Vec<u8>>) -> (FetchFile, Arc<Mutex<Vec<String>>>) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let fetch: FetchFile = {
        let asked = asked.clone();
        Arc::new(move |url: String, _sha256: String, _max_bytes: u64| {
            asked.lock().unwrap().push(url.clone());
            let served = served
                .get(&url)
                .cloned()
                .ok_or(GrammarFetchError::Status(404));
            Box::pin(async move { served })
        })
    };
    (fetch, asked)
}

/// What `language`'s entry in `catalog` serves: each of `files` at its URL.
fn served(
    catalog: &Catalog,
    language: &str,
    files: &[(String, Vec<u8>)],
) -> HashMap<String, Vec<u8>> {
    let grammar = Grammar::from_entry(TEMPLATE, catalog.get(language).unwrap());
    (files.iter())
        .map(|(name, bytes)| (grammar.url(name), bytes.clone()))
        .collect()
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

/// The real extension loader, watching `extensions` for the batches `send`s.
fn loader(state: &Path, cx: &mut TestAppContext) -> (PathBuf, kiln::platform::config::BatchSender) {
    let extensions = state.join("extensions");
    let user = state.join("settings.toml");
    fs::write(&user, "").unwrap();
    let (sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .settings_store(SettingsStore::load(Some(user), None).into_inner())
        .theme_mode(ThemeMode::Dark)
        .watch_with(batches);
    let config = crate::config::with_extensions(config, extensions.clone());
    cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();
    (extensions, sender)
}

/// Waits for the loader to register `language`.
fn registers(language: &str, cx: &mut TestAppContext) -> &'static fe_syntax::Language {
    (0..600)
        .find_map(|_| {
            cx.run_until_parked();
            let found = fe_syntax::language_by_name(language);
            if found.is_none() {
                std::thread::sleep(Duration::from_millis(50));
            }
            found
        })
        .unwrap_or_else(|| panic!("the loader registers {language}"))
}

#[gpui::test]
fn install_from_an_entry_writes_every_file_and_the_loader_registers_it(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let (extensions, sender) = loader(state.path(), cx);
    assert!(fe_syntax::language_by_name("jsonidx").is_none());
    let files = fixture_files("jsonidx", "; none\n");
    let catalog = catalog(vec![(
        "jsonidx",
        entry("jsonidx", &files, &["jsonidx"], &[], engine()),
    )]);
    let (fetch, asked) = fetcher(served(&catalog, "jsonidx", &files));

    let dir = block_on(install_entry(
        &catalog,
        TEMPLATE,
        "jsonidx",
        &extensions,
        &fetch,
    ))
    .unwrap();

    assert_eq!(dir, extensions.join("tenmoku.grammar_jsonidx"));
    assert_eq!(entries(&extensions), ["tenmoku.grammar_jsonidx"]);
    for (name, bytes) in &files {
        assert_eq!(&fs::read(dir.join(name)).unwrap(), bytes, "{name}");
    }
    assert_eq!(entries(&dir).len(), files.len());
    // From the entry's release, its manifest first.
    let tag = &catalog.get("jsonidx").unwrap().entry.tag;
    let asked = asked.lock().unwrap().clone();
    assert_eq!(
        asked[0],
        format!("https://github.com/acme/registry/releases/download/{tag}/extension.toml")
    );
    assert_eq!(asked.len(), files.len(), "{asked:?}");

    sender.send(vec![extensions]);
    let language = registers("jsonidx", cx);
    assert_eq!(
        fe_syntax::language_name_for_path(Path::new("a.jsonidx")),
        Some("jsonidx")
    );
    let buffer = fe_buffer::Buffer::new("{\"a\": 12}");
    let mut syntax = fe_syntax::Syntax::new(language, &buffer.snapshot());
    assert!(syntax.parse_within(Duration::from_secs(10)));
    let highlighted: Vec<_> = (syntax.highlights(0..9).into_iter())
        .map(|span| span.range)
        .collect();
    assert_eq!(
        highlighted,
        vec![1..4],
        "the installed highlights.scm is used"
    );
}

#[test]
fn any_file_hash_mismatch_writes_nothing() {
    let files = fixture_files("jsonhash", "; none\n");
    let catalog = catalog(vec![(
        "jsonhash",
        entry("jsonhash", &files, &["jsonhash"], &[], engine()),
    )]);
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");

    for (ix, (name, _)) in files.iter().enumerate() {
        // The same length, another byte: only the hash tells.
        let mut tampered = files.clone();
        tampered[ix].1[0] ^= 1;
        let (fetch, _) = fetcher(served(&catalog, "jsonhash", &tampered));
        let error = block_on(install_entry(
            &catalog,
            TEMPLATE,
            "jsonhash",
            &extensions,
            &fetch,
        ))
        .unwrap_err();
        assert!(
            matches!(
                &error,
                InstallError::Fetch(GrammarFetchError::HashMismatch { .. })
            ),
            "{name}: {error}"
        );
        assert_eq!(entries(&extensions), Vec::<String>::new(), "{name}");
    }

    // The write checks again, whatever came back from the fetch step.
    let (fetch, _) = fetcher(served(&catalog, "jsonhash", &files));
    let set = block_on(fetch_entry_set(
        &catalog,
        TEMPLATE,
        "jsonhash",
        &extensions,
        &fetch,
    ))
    .unwrap();
    let mut bundled = Vec::new();
    for (language, files) in &set {
        let mut put = |part: &[u8]| {
            bundled.extend((part.len() as u64).to_le_bytes());
            bundled.extend(part);
        };
        put(language.as_bytes());
        put(&(files.len() as u64).to_le_bytes());
        for (name, bytes) in files {
            put(name.as_bytes());
            let mut bytes = bytes.clone();
            if name == "highlights.scm" {
                bytes[0] ^= 1;
            }
            put(&bytes);
        }
    }
    let error =
        write_entry_bundle(&catalog, TEMPLATE, "jsonhash", &bundled, &extensions).unwrap_err();
    assert!(
        matches!(
            &error,
            InstallError::Fetch(GrammarFetchError::HashMismatch { .. })
        ),
        "{error}"
    );
    assert_eq!(entries(&extensions), Vec::<String>::new());
}

#[test]
fn a_file_outside_the_allowed_set_is_refused() {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    for (outside, fetches) in [
        ("build.rs", 0),
        (".hidden.scm", 0),
        ("queries\\x.scm", 0),
        ("LICENSE-", 0),
        // A second wasm: refused once the manifest names the other.
        ("other.wasm", 1),
    ] {
        let mut files = fixture_files("jsonset", "; none\n");
        files.push((outside.to_owned(), b"x".to_vec()));
        files.sort_by(|a, b| a.0.cmp(&b.0));
        let catalog = catalog(vec![(
            "jsonset",
            entry("jsonset", &files, &["jsonset"], &[], engine()),
        )]);
        let (fetch, asked) = fetcher(served(&catalog, "jsonset", &files));

        let error = block_on(install_entry(
            &catalog,
            TEMPLATE,
            "jsonset",
            &extensions,
            &fetch,
        ))
        .unwrap_err();

        assert!(
            matches!(error, InstallError::Refused { .. }),
            "{outside}: {error}"
        );
        assert!(
            error.to_string().contains(&format!("{outside:?}")),
            "{error}"
        );
        assert_eq!(asked.lock().unwrap().len(), fetches, "{outside}");
        assert_eq!(entries(&extensions), Vec::<String>::new(), "{outside}");
    }

    // LICENSE-*, NOTICE are in the set.
    let mut files = fixture_files("jsonlic", "; none\n");
    files.push(("LICENSE-APACHE".into(), b"Apache\n".to_vec()));
    files.push(("NOTICE".into(), b"notice\n".to_vec()));
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let catalog = catalog(vec![(
        "jsonlic",
        entry("jsonlic", &files, &["jsonlic"], &[], engine()),
    )]);
    let (fetch, _) = fetcher(served(&catalog, "jsonlic", &files));
    let dir = block_on(install_entry(
        &catalog,
        TEMPLATE,
        "jsonlic",
        &extensions,
        &fetch,
    ))
    .unwrap();
    assert!(dir.join("LICENSE-APACHE").exists() && dir.join("NOTICE").exists());
}

#[test]
fn a_manifest_naming_another_language_or_wasm_is_refused() {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    let id = "tenmoku.grammar_jsonman";
    for (manifest, says) in [
        (manifest(id, "json", "tree-sitter-json.wasm"), "json"),
        (
            manifest(id, "jsonman", "tree-sitter-other.wasm"),
            "tree-sitter-other.wasm",
        ),
        (
            manifest(id, "jsonman", "../tree-sitter-json.wasm"),
            "../tree-sitter-json.wasm",
        ),
        (
            manifest("tenmoku.grammar_json", "jsonman", "tree-sitter-json.wasm"),
            "tenmoku.grammar_json",
        ),
        ("not toml [".to_owned(), "jsonman"),
    ] {
        let mut files = fixture_files("jsonman", "; none\n");
        let at = files
            .iter()
            .position(|(name, _)| name == "extension.toml")
            .unwrap();
        files[at].1 = manifest.into_bytes();
        let catalog = catalog(vec![(
            "jsonman",
            entry("jsonman", &files, &["jsonman"], &[], engine()),
        )]);
        let (fetch, asked) = fetcher(served(&catalog, "jsonman", &files));

        let error = block_on(install_entry(
            &catalog,
            TEMPLATE,
            "jsonman",
            &extensions,
            &fetch,
        ))
        .unwrap_err();

        assert!(
            matches!(error, InstallError::Refused { .. }),
            "{says}: {error}"
        );
        assert!(error.to_string().contains(says), "{error}");
        assert_eq!(
            asked.lock().unwrap().len(),
            1,
            "only the manifest is fetched"
        );
        assert_eq!(entries(&extensions), Vec::<String>::new());
    }
}

#[test]
fn incompatible_entries_are_never_offered() {
    let files = fixture_files("jsonnew", "; none\n");
    let newer = Engine {
        abi: tree_sitter::LANGUAGE_VERSION as u32 + 1,
        ..engine()
    };
    let catalog = Arc::new(catalog(vec![
        (
            "jsonnew",
            entry("jsonnew", &files, &["jsonnew"], &["bundle"], newer),
        ),
        (
            "jsonnext",
            entry(
                "jsonnext",
                &files,
                &["jsonnext"],
                &["bundle", "solo"],
                engine(),
            ),
        ),
        (
            "jsonapi",
            entry(
                "jsonapi",
                &files,
                &["jsonapi"],
                &["solo"],
                Engine {
                    tenmoku: "2.0".into(),
                    ..engine()
                },
            ),
        ),
    ]));
    let state = tempfile::tempdir().unwrap();
    let (fetch, asked) = fetcher(HashMap::new());
    let hooks = IndexHooks {
        fetch,
        registered: std::rc::Rc::new(|_: &str, _: &gpui::App| true),
    };
    let offers = index_offers_with(
        catalog,
        TEMPLATE.to_owned(),
        hooks,
        state.path().to_owned(),
        None,
    );

    let languages: Vec<&str> = offers.catalog.iter().map(|o| o.language.as_str()).collect();
    assert_eq!(languages, ["jsonnext"]);
    let packs: Vec<(&str, Vec<String>)> = (offers.packs.iter())
        .map(|pack| (pack.id.as_str(), pack.languages.clone()))
        .collect();
    assert_eq!(
        packs,
        [
            ("bundle", vec!["jsonnext".to_owned()]),
            ("solo", vec!["jsonnext".to_owned()])
        ]
    );
    let detect = offers.detect.clone().unwrap();
    assert_eq!(detect(Path::new("a.jsonnew"), None), None);
    assert_eq!(detect(Path::new("a.jsonapi"), None), None);
    assert_eq!(
        detect(Path::new("a.jsonnext"), None).as_deref(),
        Some("jsonnext")
    );
    let offer = &offers.catalog[0];
    assert_eq!(offer.packs, ["bundle", "solo"]);
    assert_eq!(
        offer.size,
        files.iter().map(|(_, b)| b.len() as u64).sum::<u64>()
    );
    assert!(asked.lock().unwrap().is_empty());
}

/// A language whose query injects an injection-only one brings it, written with
/// it; one installed already isn't fetched again, but its own injections count.
#[test]
fn a_language_brings_the_injection_only_languages_it_injects() {
    let host = fixture_files(
        "jsonhost",
        "((x) @injection.content (#set! injection.language \"jsoninner\"))\n",
    );
    let inner = fixture_files(
        "jsoninner",
        "((x) @injection.content (#set! injection.language \"jsondeep\"))\n",
    );
    let deep = fixture_files("jsondeep", "; none\n");
    let catalog = catalog(vec![
        (
            "jsonhost",
            entry("jsonhost", &host, &["jsonhost"], &[], engine()),
        ),
        ("jsoninner", entry("jsoninner", &inner, &[], &[], engine())),
        ("jsondeep", entry("jsondeep", &deep, &[], &[], engine())),
    ]);
    let mut all = served(&catalog, "jsonhost", &host);
    all.extend(served(&catalog, "jsoninner", &inner));
    all.extend(served(&catalog, "jsondeep", &deep));
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    let (fetch, _) = fetcher(all.clone());

    block_on(install_entry(
        &catalog,
        TEMPLATE,
        "jsonhost",
        &extensions,
        &fetch,
    ))
    .unwrap();
    assert_eq!(
        entries(&extensions),
        [
            "tenmoku.grammar_jsondeep",
            "tenmoku.grammar_jsonhost",
            "tenmoku.grammar_jsoninner"
        ]
    );

    // With the inner one installed, only the deep one it injects is fetched.
    fs::remove_dir_all(extensions.join("tenmoku.grammar_jsonhost")).unwrap();
    fs::remove_dir_all(extensions.join("tenmoku.grammar_jsondeep")).unwrap();
    let (fetch, asked) = fetcher(all);
    let set = block_on(fetch_entry_set(
        &catalog,
        TEMPLATE,
        "jsonhost",
        &extensions,
        &fetch,
    ))
    .unwrap();
    let fetched: Vec<&str> = set.iter().map(|(language, _)| language.as_str()).collect();
    assert_eq!(fetched, ["jsonhost", "jsondeep"]);
    assert!(
        !asked
            .lock()
            .unwrap()
            .iter()
            .any(|url| url.contains("jsoninner"))
    );
}

/// Dropping the fetch step part way writes nothing.
#[test]
fn a_cancelled_fetch_writes_nothing() {
    let files = fixture_files("jsoncancel", "; none\n");
    let catalog = catalog(vec![(
        "jsoncancel",
        entry("jsoncancel", &files, &["jsoncancel"], &[], engine()),
    )]);
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    // The manifest arrives; the next file never does.
    let manifest = served(&catalog, "jsoncancel", &files)
        .into_iter()
        .find(|(url, _)| url.ends_with("/extension.toml"))
        .unwrap();
    let fetch: FetchFile = Arc::new(move |url: String, _, _| {
        let served = (url == manifest.0).then(|| manifest.1.clone());
        Box::pin(async move {
            match served {
                Some(bytes) => Ok(bytes),
                None => std::future::pending().await,
            }
        })
    });
    {
        let mut installing = std::pin::pin!(install_entry(
            &catalog,
            TEMPLATE,
            "jsoncancel",
            &extensions,
            &fetch
        ));
        assert!((&mut installing).now_or_never().is_none());
    }
    assert_eq!(entries(&extensions), Vec::<String>::new());
}

/// One real language from the published index, through the real client under
/// the app's grants, into a temporary extensions directory, and the loader
/// registers it. Needs the network.
#[gpui::test]
#[ignore = "needs the network and the published grammar index"]
fn live_install_from_the_registry(cx: &mut TestAppContext) {
    use crate::grammar_registry::{MAX_INDEX_BYTES, MAX_SIG_BYTES, baked, sig_url, verify_index};

    let baked = baked().unwrap();
    let client = fe_net::Client::new(&crate::grammar_install::grants());
    let get = |url: String, max_bytes| {
        block_on(fe_net::grammar_fetch::fetch_bounded(
            &client, &url, max_bytes,
        ))
        .unwrap_or_else(|error| panic!("{url}: {error}"))
    };
    let sig = get(sig_url(&baked.index_url), MAX_SIG_BYTES);
    let bytes = get(baked.index_url.clone(), MAX_INDEX_BYTES);
    let index = verify_index(
        &bytes,
        std::str::from_utf8(&sig).unwrap(),
        &baked.public_key,
    )
    .unwrap();
    let catalog = Catalog::new(index, &EngineSupport::running());

    let state = tempfile::tempdir().unwrap();
    let (extensions, sender) = loader(state.path(), cx);
    // The smallest that isn't built in and claims files.
    let (language, size) = (catalog.index().languages.iter())
        .filter(|(language, entry)| {
            !fe_syntax::language_names().any(|name| name == language.as_str())
                && !entry.extensions.is_empty()
        })
        .map(|(language, entry)| {
            (
                language.clone(),
                entry.files.iter().map(|f| f.size).sum::<u64>(),
            )
        })
        .min_by_key(|(_, size)| *size)
        .expect("a language to install");
    let fetch: FetchFile = Arc::new(fetch_file);

    let dir = block_on(install_entry(
        &catalog,
        &baked.url_template,
        &language,
        &extensions,
        &fetch,
    ))
    .unwrap_or_else(|error| panic!("{language}: {error}"));
    sender.send(vec![extensions.clone()]);
    registers(&language, cx);

    println!(
        "installed {language} ({size} bytes, {} files) from index serial {} to {}; registered",
        entries(&dir).len(),
        catalog.index().serial,
        dir.display()
    );
}

/// The offers' detection, as the app's prompt asks it: a whole name in any
/// case before an extension, and a compound extension over a built-in's
/// shorter one, as the prompt offered them before the index.
#[test]
fn the_offers_detect_any_case_names_and_compound_extensions() {
    let files = fixture_files("zzgomod", "; none\n");
    let mut gomod = entry("zzgomod", &files, &[], &[], engine());
    gomod.file_names = vec!["zz.mod".to_owned()];
    let catalog = Arc::new(catalog(vec![
        ("zzgomod", gomod),
        (
            "zzmodula",
            entry("zzmodula", &files, &["mod"], &[], engine()),
        ),
        (
            "zzjsonc",
            entry("zzjsonc", &files, &["z.json"], &[], engine()),
        ),
    ]));
    // A precondition: JSON is built in.
    assert!(fe_syntax::language_name_for_path(Path::new("a.json")).is_some());
    let state = tempfile::tempdir().unwrap();
    let (fetch, _) = fetcher(HashMap::new());
    let hooks = IndexHooks {
        fetch,
        registered: std::rc::Rc::new(|_: &str, _: &gpui::App| true),
    };
    let offers = index_offers_with(
        catalog,
        TEMPLATE.to_owned(),
        hooks,
        state.path().to_owned(),
        None,
    );
    let detect = offers.detect.clone().unwrap();
    let detect = |path: &str| detect(Path::new(path), None);

    assert_eq!(detect("dir/zz.mod").as_deref(), Some("zzgomod"));
    assert_eq!(detect("dir/ZZ.MOD").as_deref(), Some("zzgomod"), "any case");
    assert_eq!(detect("x.mod").as_deref(), Some("zzmodula"));
    assert_eq!(
        detect("a.z.json").as_deref(),
        Some("zzjsonc"),
        "over JSON's"
    );
    assert_eq!(detect("a.json"), None, "JSON keeps its own");
}

/// The install step is done once it has written, though what the language
/// injects hasn't registered: that's watched for apart, so it holds up
/// nothing after it, such as the rest of a pack.
#[gpui::test]
fn an_install_is_done_once_written_whatever_it_injects(cx: &mut TestAppContext) {
    let host = fixture_files(
        "jsonhost3",
        "((x) @injection.content (#set! injection.language \"jsoninner3\"))\n",
    );
    let inner = fixture_files("jsoninner3", "; none\n");
    let catalog = Arc::new(catalog(vec![
        (
            "jsonhost3",
            entry("jsonhost3", &host, &["jsonhost3"], &[], engine()),
        ),
        (
            "jsoninner3",
            entry("jsoninner3", &inner, &[], &[], engine()),
        ),
    ]));
    let mut all = served(&catalog, "jsonhost3", &host);
    all.extend(served(&catalog, "jsoninner3", &inner));
    let (fetch, _) = fetcher(all);
    let asked = std::rc::Rc::new(std::cell::Cell::new(0));
    let registered: crate::grammar_install::Registered = {
        let asked = asked.clone();
        std::rc::Rc::new(move |language: &str, _: &gpui::App| {
            asked.set(asked.get() + 1);
            language != "jsoninner3"
        })
    };
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    let offers = index_offers_with(
        catalog,
        TEMPLATE.to_owned(),
        IndexHooks { fetch, registered },
        extensions.clone(),
        None,
    );
    let host = (offers.catalog.iter())
        .find(|offer| offer.language == "jsonhost3")
        .unwrap()
        .clone();

    let fetching = cx.update(|cx| (offers.fetch)(&host, cx));
    let bundled = cx.foreground_executor().block_test(fetching).unwrap();
    let writing = cx.update(|cx| (offers.install)(&host, bundled, cx));
    cx.foreground_executor().block_test(writing).unwrap();

    assert_eq!(
        entries(&extensions),
        ["tenmoku.grammar_jsonhost3", "tenmoku.grammar_jsoninner3"]
    );
    // And it's still watched for.
    let before = asked.get();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(asked.get() > before, "still looking for jsoninner3");
}
