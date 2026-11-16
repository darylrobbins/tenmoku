//! [`crate::grammar_registry`]: the signed index verified before it's parsed,
//! cached and checked again, rollbacks and races, the engine's compatibility
//! and detection. Fetches go through a closure over a map of URL to bytes,
//! and keys are made for each test.

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap},
    fs, io,
    path::Path,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use fe_net::{GrammarFetchError, NetError};
use fe_registry::grammars::{self, Engine, FileEntry, Index, Language, Pack};
use futures::executor::block_on;

use crate::grammar_registry::{
    Cache, Catalog, EngineSupport, IndexError, MAX_AGE, Policy, RETRY_PAUSE, baked, refresh,
    sig_url, verify_index,
};
use crate::update_signature::{self, PrivateKey, PublicKey, SignatureError};

const URL: &str = "https://github.com/tenmoku-editor/registry/releases/download/index/index.json";

/// A throwaway key pair.
fn keys() -> (PublicKey, PrivateKey) {
    let (public, private) = update_signature::generate();
    (
        PublicKey::from_base64(&public).unwrap(),
        PrivateKey::from_base64(&private).unwrap(),
    )
}

/// An engine this app loads.
fn engine() -> Engine {
    Engine {
        tenmoku: "1.0".to_owned(),
        abi: tree_sitter::LANGUAGE_VERSION as u32,
        captures: grammars::CAPTURES,
    }
}

/// A language entry whose files hold together, in the packs `packs`.
fn language(key: &str, packs: &[&str], engine: Engine) -> Language {
    let files = vec![
        FileEntry::of("extension.toml", key.as_bytes()),
        FileEntry::of(&format!("tree-sitter-{key}.wasm"), b"\0asm"),
    ];
    let digest = grammars::digest(&files);
    Language {
        name: key.to_owned(),
        version: "0.1.0".to_owned(),
        tag: grammars::tag(key, &digest),
        digest,
        extensions: Vec::new(),
        file_names: Vec::new(),
        shebangs: Vec::new(),
        packs: packs.iter().map(|&pack| pack.to_owned()).collect(),
        license: "MIT".to_owned(),
        engine,
        files,
    }
}

/// An index of `languages`, each pack listing the languages that list it.
fn index(serial: u64, languages: Vec<(&str, Language)>) -> Index {
    let languages: BTreeMap<String, Language> = languages
        .into_iter()
        .map(|(key, language)| (key.to_owned(), language))
        .collect();
    let mut packs: BTreeMap<String, Pack> = BTreeMap::new();
    for (key, language) in &languages {
        for pack in &language.packs {
            packs
                .entry(pack.clone())
                .or_insert_with(|| Pack {
                    name: pack.clone(),
                    description: String::new(),
                    languages: Vec::new(),
                    tarball: None,
                })
                .languages
                .push(key.clone());
        }
    }
    let index = Index {
        schema: grammars::SCHEMA,
        serial,
        url_template: "https://github.com/tenmoku-editor/registry/releases/download/{tag}/{file}"
            .to_owned(),
        packs,
        languages,
    };
    index.check().unwrap();
    index
}

/// A small index with `serial`.
fn simple(serial: u64) -> Index {
    index(
        serial,
        vec![("kotlin", language("kotlin", &["jvm"], engine()))],
    )
}

/// `index`'s bytes and their signature file with `key`.
fn signed(index: &Index, key: &PrivateKey) -> (Vec<u8>, Vec<u8>) {
    let bytes = index.to_json().into_bytes();
    let sig = format!("{}\n", update_signature::sign(key, &bytes)).into_bytes();
    (bytes, sig)
}

/// One answer to a fetch.
type Answer = Result<Vec<u8>, GrammarFetchError>;

/// What a fetcher serves: each URL's answers in turn, the last one kept.
#[derive(Default)]
struct Served {
    answers: RefCell<HashMap<String, Vec<Answer>>>,
    fetched: RefCell<Vec<(String, u64)>>,
    pauses: Cell<usize>,
}

impl Served {
    /// Serves `bytes` and `sig` at [`URL`] and its `.sig`.
    fn pair((bytes, sig): (Vec<u8>, Vec<u8>)) -> Served {
        let served = Served::default();
        served.answer(URL, vec![Ok(bytes)]);
        served.answer(&sig_url(URL), vec![Ok(sig)]);
        served
    }

    fn answer(&self, url: &str, answers: Vec<Answer>) {
        self.answers.borrow_mut().insert(url.to_owned(), answers);
    }

    /// [`refresh`] into `cache` with `key`, against what's served.
    fn refresh(&self, cache: &Cache, key: &PublicKey) -> Result<Index, IndexError> {
        block_on(refresh(
            cache,
            URL,
            key,
            |url: String, max_bytes| {
                self.fetched.borrow_mut().push((url.clone(), max_bytes));
                let mut answers = self.answers.borrow_mut();
                let answer = match answers.get_mut(&url) {
                    Some(list) if list.len() > 1 => list.remove(0),
                    Some(list) => list[0].clone(),
                    None => Err(GrammarFetchError::Status(404)),
                };
                std::future::ready(answer)
            },
            |pause| {
                assert_eq!(pause, RETRY_PAUSE);
                self.pauses.set(self.pauses.get() + 1);
                std::future::ready(())
            },
        ))
    }

    fn fetches(&self) -> usize {
        self.fetched.borrow().len()
    }
}

/// A cache in a new temporary state directory.
fn cache() -> (tempfile::TempDir, Cache) {
    let state = tempfile::tempdir().unwrap();
    let cache = Cache::new(state.path());
    (state, cache)
}

/// A cache holding `index` signed with `key`.
fn seeded(index: &Index, key: &PrivateKey, public: &PublicKey) -> (tempfile::TempDir, Cache) {
    let (state, cache) = cache();
    Served::pair(signed(index, key))
        .refresh(&cache, public)
        .unwrap();
    (state, cache)
}

fn is_signature_error(result: &Result<Index, IndexError>) -> bool {
    matches!(result, Err(IndexError::Signature(_)))
}

#[test]
fn good_signature_loads_the_index() {
    let (public, private) = keys();
    let index = simple(7);
    let (bytes, sig) = signed(&index, &private);
    let sig_text = String::from_utf8(sig.clone()).unwrap();
    assert_eq!(verify_index(&bytes, &sig_text, &public).unwrap(), index);
    // Trailing whitespace on the signature, as a file's last newline.
    assert!(verify_index(&bytes, sig_text.trim_end(), &public).is_ok());

    let (state, cache) = cache();
    let served = Served::pair((bytes.clone(), sig.clone()));
    assert_eq!(served.refresh(&cache, &public).unwrap(), index);
    // The signature first, then the index, each within its bound.
    assert_eq!(
        *served.fetched.borrow(),
        [(sig_url(URL), 4096), (URL.to_owned(), 4 * 1024 * 1024)]
    );
    assert_eq!(served.pauses.get(), 0);
    assert_eq!(fs::read(cache.index_path()).unwrap(), bytes);
    assert_eq!(fs::read(cache.sig_path()).unwrap(), sig);
    assert_eq!(cache.load(&public), Some(index));
    assert!(
        cache
            .index_path()
            .starts_with(state.path().join("registry")),
        "the cache is <state dir>/registry/"
    );
    // No temporary or kept file is left beside them, only the lock.
    let mut left: Vec<_> = fs::read_dir(cache.index_path().parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    assert_eq!(left, ["index.json", "index.json.lock", "index.json.sig"]);
}

#[test]
fn tampered_index_is_refused_and_the_cache_is_kept() {
    let (public, private) = keys();
    let (_state, cache) = seeded(&simple(1), &private, &public);
    let kept = (
        fs::read(cache.index_path()).unwrap(),
        fs::read(cache.sig_path()).unwrap(),
    );

    let (mut bytes, sig) = signed(&simple(2), &private);
    // One byte changed after signing: the serial's digit.
    let at = bytes.windows(2).position(|pair| pair == b"2,").unwrap();
    bytes[at] = b'3';
    assert!(is_signature_error(&verify_index(
        &bytes,
        std::str::from_utf8(&sig).unwrap(),
        &public
    )));
    let served = Served::pair((bytes, sig));
    let result = served.refresh(&cache, &public);
    assert!(
        matches!(result, Err(IndexError::Signature(SignatureError::Mismatch))),
        "{result:?}"
    );
    assert_eq!(
        (
            fs::read(cache.index_path()).unwrap(),
            fs::read(cache.sig_path()).unwrap()
        ),
        kept
    );
    assert_eq!(cache.load(&public).map(|index| index.serial), Some(1));
}

#[test]
fn wrong_key_is_refused() {
    let (_, private) = keys();
    let (other, _) = keys();
    let (bytes, sig) = signed(&simple(1), &private);
    let result = verify_index(&bytes, std::str::from_utf8(&sig).unwrap(), &other);
    assert!(
        matches!(result, Err(IndexError::Signature(SignatureError::Mismatch))),
        "{result:?}"
    );
    let (_state, cache) = cache();
    assert!(is_signature_error(
        &Served::pair((bytes, sig)).refresh(&cache, &other)
    ));
    assert!(!cache.index_path().exists() && !cache.sig_path().exists());
}

#[test]
fn missing_signature_is_refused() {
    let (public, private) = keys();
    let (bytes, _) = signed(&simple(1), &private);
    for sig in ["", "\n"] {
        assert!(matches!(
            verify_index(&bytes, sig, &public),
            Err(IndexError::Signature(SignatureError::SignatureLength(0)))
        ));
    }

    // No .sig published: a 404, retried once as a file being replaced, and
    // the index itself never fetched.
    let (_state, cache) = cache();
    let served = Served::default();
    served.answer(URL, vec![Ok(bytes)]);
    let result = served.refresh(&cache, &public);
    assert!(
        matches!(
            &result,
            Err(IndexError::Fetch { url, error: GrammarFetchError::Status(404) }) if *url == sig_url(URL)
        ),
        "{result:?}"
    );
    assert_eq!((served.pauses.get(), served.fetches()), (1, 2));
    assert!(!cache.index_path().exists());
}

#[test]
fn tampered_cache_is_ignored() {
    let (public, private) = keys();
    let (_state, cache) = seeded(&simple(5), &private, &public);
    assert!(cache.load(&public).is_some());

    // Edited on disk: the signature no longer matches, so it's no cache at all.
    let text = fs::read_to_string(cache.index_path()).unwrap();
    fs::write(cache.index_path(), text.replace("kotlin\"", "kotlim\"")).unwrap();
    assert_eq!(cache.load(&public), None);
    // A signature edited on disk is the same.
    let (_state, other) = seeded(&simple(5), &private, &public);
    fs::write(other.sig_path(), "not a signature\n").unwrap();
    assert_eq!(other.load(&public), None);

    // And its serial doesn't count: an older index than it claims is taken.
    let served = Served::pair(signed(&simple(3), &private));
    assert_eq!(served.refresh(&cache, &public).unwrap().serial, 3);
    assert_eq!(cache.load(&public).map(|index| index.serial), Some(3));
}

#[test]
fn lower_serial_is_refused() {
    let (public, private) = keys();
    let (_state, cache) = seeded(&simple(5), &private, &public);
    let kept = fs::read(cache.index_path()).unwrap();

    let served = Served::pair(signed(&simple(4), &private));
    let result = served.refresh(&cache, &public);
    assert!(
        matches!(
            result,
            Err(IndexError::Rollback {
                found: 4,
                cached: 5
            })
        ),
        "{result:?}"
    );
    assert_eq!(served.pauses.get(), 0, "a signed rollback isn't a race");
    assert_eq!(fs::read(cache.index_path()).unwrap(), kept);

    // The same serial is a retry of the same publish, and a higher one new.
    for serial in [5, 6] {
        let served = Served::pair(signed(&simple(serial), &private));
        assert_eq!(served.refresh(&cache, &public).unwrap().serial, serial);
    }
    assert_eq!(cache.load(&public).map(|index| index.serial), Some(6));
}

#[test]
fn unknown_schema_is_refused() {
    let (public, private) = keys();
    let json = simple(1)
        .to_json()
        .replacen("\"schema\": 1", "\"schema\": 2", 1);
    assert!(json.contains("\"schema\": 2"));
    let bytes = json.into_bytes();
    let sig = update_signature::sign(&private, &bytes);
    let result = verify_index(&bytes, &sig, &public);
    assert!(
        matches!(&result, Err(IndexError::Invalid(message)) if message.contains("schema 2")),
        "{result:?}"
    );

    // Signed, so not a race: refused at once, nothing cached.
    let (_state, cache) = cache();
    let served = Served::pair((bytes, sig.into_bytes()));
    assert!(matches!(
        served.refresh(&cache, &public),
        Err(IndexError::Invalid(_))
    ));
    assert_eq!(served.pauses.get(), 0);
    assert!(!cache.index_path().exists());

    // A key the schema doesn't have is refused the same way.
    let bytes = simple(1)
        .to_json()
        .replacen("\"serial\"", "\"extra\": 1,\n  \"serial\"", 1)
        .into_bytes();
    let sig = update_signature::sign(&private, &bytes);
    assert!(matches!(
        verify_index(&bytes, &sig, &public),
        Err(IndexError::Invalid(_))
    ));
}

#[test]
fn signature_race_retries_once() {
    let (public, private) = keys();
    let (_state, cache) = cache();
    let (old, _) = signed(&simple(1), &private);
    let (new, new_sig) = signed(&simple(2), &private);

    // The new signature is up, the old index not yet replaced: after the
    // pause, both are new.
    let served = Served::default();
    served.answer(&sig_url(URL), vec![Ok(new_sig.clone())]);
    served.answer(URL, vec![Ok(old.clone()), Ok(new.clone())]);
    assert_eq!(served.refresh(&cache, &public).unwrap().serial, 2);
    assert_eq!((served.pauses.get(), served.fetches()), (1, 4));
    assert_eq!(fs::read(cache.index_path()).unwrap(), new);

    // Still mismatched after the pause: refused, with one retry only.
    let (_state, cache) = self::cache();
    let served = Served::default();
    served.answer(&sig_url(URL), vec![Ok(new_sig)]);
    served.answer(URL, vec![Ok(old)]);
    assert!(is_signature_error(&served.refresh(&cache, &public)));
    assert_eq!((served.pauses.get(), served.fetches()), (1, 4));
    assert!(!cache.index_path().exists());
}

#[test]
fn offline_keeps_the_cache() {
    let (public, private) = keys();
    let (_state, cache) = seeded(&simple(3), &private, &public);
    let kept = fs::read(cache.index_path()).unwrap();

    for error in [
        GrammarFetchError::Net(NetError::Connect("no route".to_owned())),
        GrammarFetchError::Net(NetError::Timeout),
        GrammarFetchError::HostNotAllowed("example.com".to_owned()),
    ] {
        let served = Served::default();
        served.answer(&sig_url(URL), vec![Err(error.clone())]);
        served.answer(URL, vec![Err(error.clone())]);
        let result = served.refresh(&cache, &public);
        assert!(
            matches!(&result, Err(IndexError::Fetch { error: found, .. }) if *found == error),
            "{result:?}"
        );
        assert_eq!(served.pauses.get(), 0, "offline isn't waited out");
        assert_eq!(fs::read(cache.index_path()).unwrap(), kept);
        assert_eq!(cache.load(&public).map(|index| index.serial), Some(3));
    }
}

#[test]
fn incompatible_entries_are_dropped() {
    let running = EngineSupport::running();
    assert_eq!(running.api, crate::config::EXTENSION_API);
    assert_eq!(
        running.abi,
        tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION as u32..=tree_sitter::LANGUAGE_VERSION as u32
    );
    assert_eq!(running.captures, grammars::CAPTURES);

    let with = |change: &dyn Fn(&mut Engine)| {
        let mut engine = engine();
        change(&mut engine);
        engine
    };
    let api = running.api;
    let entries = [
        ("good", engine()),
        (
            "good-oldest-abi",
            with(&|e| e.abi = tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION as u32),
        ),
        (
            "good-this-minor",
            with(&|e| e.tenmoku = format!("{}.{}", api.major, api.minor)),
        ),
        (
            "abi-above",
            with(&|e| e.abi = tree_sitter::LANGUAGE_VERSION as u32 + 1),
        ),
        (
            "abi-below",
            with(&|e| e.abi = tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION as u32 - 1),
        ),
        (
            "captures-above",
            with(&|e| e.captures = grammars::CAPTURES + 1),
        ),
        (
            "minor-above",
            with(&|e| e.tenmoku = format!("{}.{}", api.major, api.minor + 1)),
        ),
        (
            "other-major",
            with(&|e| e.tenmoku = format!("{}.0", api.major + 1)),
        ),
        ("not-a-version", with(&|e| e.tenmoku = "one".to_owned())),
    ];
    let languages = entries
        .iter()
        .map(|(key, engine)| {
            // "solo" has only incompatible languages.
            let packs: &[&str] = if key.starts_with("good") {
                &["mixed"]
            } else {
                &["mixed", "solo"]
            };
            let mut entry = language(key, packs, engine.clone());
            entry.extensions = vec![format!("zz{}", key.len())];
            (*key, entry)
        })
        .collect();
    let catalog = Catalog::new(index(1, languages), &running);

    let kept: Vec<&str> = catalog
        .index()
        .languages
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(kept, ["good", "good-oldest-abi", "good-this-minor"]);
    let mixed: Vec<&str> = catalog
        .pack_languages("mixed")
        .iter()
        .map(|entry| entry.language)
        .collect();
    assert_eq!(mixed, kept);
    assert!(catalog.pack_languages("solo").is_empty());
    assert!(!catalog.index().packs.contains_key("solo"));
    assert_eq!(catalog.packs_of("good"), ["mixed"]);
    assert!(catalog.packs_of("abi-above").is_empty());
    assert!(catalog.get("other-major").is_none());

    // Not detected either: "captures-above" is the only one claiming .zz14.
    let extensions = tempfile::tempdir().unwrap();
    assert!(
        catalog
            .language_for(Path::new("a.zz14"), None, extensions.path())
            .is_none()
    );
    assert_eq!(
        catalog
            .language_for(Path::new("a.zz4"), None, extensions.path())
            .map(|entry| entry.language),
        Some("good")
    );
}

/// A language key with the extensions, file names and shebangs it claims.
type Claims<'a> = (&'a str, &'a [&'a str], &'a [&'a str], &'a [&'a str]);

/// A catalog whose languages claim what `entries` say.
fn claiming(entries: &[Claims<'_>]) -> Catalog {
    let languages = entries
        .iter()
        .map(|&(key, extensions, file_names, shebangs)| {
            let mut entry = language(key, &["pack"], engine());
            let owned = |list: &[&str]| list.iter().map(|&item| item.to_owned()).collect();
            entry.extensions = owned(extensions);
            entry.file_names = owned(file_names);
            entry.shebangs = owned(shebangs);
            (key, entry)
        })
        .collect();
    Catalog::new(index(1, languages), &EngineSupport::running())
}

#[test]
fn detects_by_file_name_extension_and_shebang() {
    let catalog = claiming(&[
        ("zzname", &[], &["Zzfile"], &[]),
        ("zzext", &["zzx"], &[], &[]),
        ("zzlong", &["long.zzx"], &[], &[]),
        ("zzbang", &[], &[], &["zzsh"]),
        // Both claim .zzt: the lower name is taken.
        ("zzb", &["zzt"], &[], &[]),
        ("zza", &["zzt"], &["Zzfile.zzx"], &[]),
    ]);
    let extensions = tempfile::tempdir().unwrap();
    let detect = |path: &str, first_line: Option<&str>| {
        catalog
            .language_for(Path::new(path), first_line, extensions.path())
            .map(|entry| entry.language)
    };
    assert_eq!(detect("dir/Zzfile", None), Some("zzname"));
    assert_eq!(detect("zzfile", None), None, "a whole name matches exactly");
    assert_eq!(detect("Zzfile.zzx", None), Some("zza"), "the name first");
    assert_eq!(detect("dir/a.zzx", None), Some("zzext"));
    assert_eq!(detect("A.ZZX", None), Some("zzext"), "case-insensitive");
    assert_eq!(detect("a.long.zzx", None), Some("zzlong"), "the longest");
    assert_eq!(detect("a.Long.ZZX", None), Some("zzlong"));
    assert_eq!(detect(".zzx", None), None, "a dot file has no extension");
    assert_eq!(detect("a.zzt", None), Some("zza"));
    assert_eq!(detect("a.zzy", None), None);

    for line in [
        "#!/usr/bin/zzsh",
        "#! /opt/bin/zzsh -x\nmore",
        "#!zzsh3.2",
        "#!/usr/bin/env zzsh",
        "#!/usr/bin/env -S zzsh -x",
        "#!/usr/bin/env -S -i FOO=1 zzsh3 -u",
        "#!/usr/bin/env -u FOO zzsh",
        "#!/usr/bin/env -- zzsh",
        "#!/usr/bin/env -S \"zzsh\" -x",
    ] {
        assert_eq!(detect("script", Some(line)), Some("zzbang"), "{line}");
    }
    for line in [
        "#!/usr/bin/zzshell",
        "#!/usr/bin/zzsh3.",
        "#!/usr/bin/env",
        "#!/usr/bin/env -i",
        "# zzsh",
        "zzsh",
    ] {
        assert_eq!(detect("script", Some(line)), None, "{line}");
    }
    // The path decides before the shebang.
    assert_eq!(detect("a.zzx", Some("#!/usr/bin/zzsh")), Some("zzext"));

    // fe-syntax's own `env` rules, which detection reuses.
    for (line, program) in [
        ("#!/usr/bin/env -Szzsh -x", Some("zzsh")),
        ("#!/usr/bin/env --split-string=zzsh", Some("zzsh")),
        ("#!/usr/bin/env -C /tmp zzsh", Some("zzsh")),
        ("#!/usr/bin/env -S FOO=\"a b\" zzsh", Some("zzsh")),
        ("#!/usr/bin/env -- FOO=1 zzsh", Some("zzsh")),
        ("#!/usr/bin/env -iu FOO zzsh", Some("zzsh")),
        ("#!/usr/bin/env --null zzsh", None),
        ("#!/usr/bin/env -x zzsh", None),
        ("#!", None),
    ] {
        assert_eq!(
            fe_syntax::shebang_program(line).as_deref(),
            program,
            "{line}"
        );
    }
    assert_eq!(detect("script", Some("#!/usr/bin/env --null zzsh")), None);
}

#[test]
fn installed_built_in_or_claimed_languages_are_not_detected() {
    let built_in = fe_syntax::language_names().next().unwrap();
    // Preconditions: what registered languages already claim.
    assert!(fe_syntax::language_name_for_path(Path::new("a.json")).is_some());
    assert!(fe_syntax::language_name_for_shebang("#!/usr/bin/env python3").is_some());
    assert!(fe_syntax::language_name_for_path(Path::new("a.zzrs")).is_none());

    let catalog = claiming(&[
        ("zzinstalled", &["zzi"], &[], &[]),
        (built_in, &["zzrs"], &[], &[]),
        ("zzjson", &["json"], &[], &[]),
        ("zzpython", &[], &[], &["python"]),
        ("zzfree", &["zzf"], &[], &[]),
    ]);
    let extensions = tempfile::tempdir().unwrap();
    fs::create_dir(extensions.path().join("tenmoku.grammar_zzinstalled")).unwrap();
    let detect = |path: &str, first_line: Option<&str>| {
        catalog
            .language_for(Path::new(path), first_line, extensions.path())
            .map(|entry| entry.language)
    };
    assert_eq!(detect("a.zzi", None), None, "installed");
    assert_eq!(detect("a.zzrs", None), None, "built in: {built_in}");
    assert_eq!(detect("a.json", None), None, "claimed by its extension");
    assert_eq!(
        detect("script", Some("#!/usr/bin/env python3")),
        None,
        "claimed by its interpreter"
    );
    // The one that's none of these still is.
    assert_eq!(detect("a.zzf", None), Some("zzfree"));
}

#[test]
fn policy_off_for_perf_wait_tests_and_without_automatic_checks() {
    assert_eq!(Policy::decide(false, false, false, true), Policy::Refresh);
    for (perf, wait_launch, under_test, automatic_checks) in [
        (true, false, false, true),
        (false, true, false, true),
        (false, false, true, true),
        (false, false, false, false),
    ] {
        assert_eq!(
            Policy::decide(perf, wait_launch, under_test, automatic_checks),
            Policy::Off
        );
    }
    // The setting it follows is on by default.
    assert!(fe_config::Settings::default().update.automatic_checks);

    // Refreshed when missing or a day old, and never when off.
    let refresh = Policy::Refresh;
    assert!(refresh.due(None));
    assert!(refresh.due(Some(MAX_AGE)));
    assert!(!refresh.due(Some(MAX_AGE - Duration::from_secs(60))));
    assert!(!Policy::Off.due(None));
    assert_eq!(MAX_AGE, Duration::from_secs(24 * 60 * 60));

    // A new cache is fresh; one with no index is missing.
    let (public, private) = keys();
    let (_state, cache) = seeded(&simple(1), &private, &public);
    let now = std::time::SystemTime::now();
    let age = |cache: &Cache, now| cache.usable_age(&public, now);
    assert!(age(&cache, now).unwrap() < Duration::from_secs(60));
    assert!(!refresh.due(age(&cache, now)));
    assert!(refresh.due(age(&cache, now + MAX_AGE + Duration::from_secs(1))));
    fs::remove_file(cache.index_path()).unwrap();
    assert_eq!(age(&cache, now), None);

    // A recent pair that doesn't verify is as good as missing: due.
    let (_state, cache) = seeded(&simple(1), &private, &public);
    fs::remove_file(cache.sig_path()).unwrap();
    assert!(refresh.due(age(&cache, now)));
    let (_state, cache) = seeded(&simple(1), &private, &public);
    fs::write(cache.sig_path(), "not a signature\n").unwrap();
    assert!(refresh.due(age(&cache, now)));
    let (_state, cache) = seeded(&simple(1), &private, &public);
    let text = fs::read_to_string(cache.index_path()).unwrap();
    fs::write(cache.index_path(), text.replace("kotlin\"", "kotlim\"")).unwrap();
    assert!(refresh.due(age(&cache, now)));
}

#[test]
fn concurrent_refreshes_cannot_roll_the_cache_back() {
    let (public, private) = keys();
    let (_state, cache) = seeded(&simple(5), &private, &public);
    let (newer, newer_sig) = signed(&simple(7), &private);
    let (older, older_sig) = signed(&simple(6), &private);

    // This launch read serial 5 and fetches 6; meanwhile another commits 7.
    let other = cache.clone();
    let result = block_on(refresh(
        &cache,
        URL,
        &public,
        |url: String, _| {
            let answer = if url == URL {
                other.commit(&public, 7, &newer, &newer_sig).unwrap();
                older.clone()
            } else {
                older_sig.clone()
            };
            std::future::ready(Ok(answer))
        },
        |_| std::future::ready(()),
    ));
    assert!(
        matches!(
            result,
            Err(IndexError::Rollback {
                found: 6,
                cached: 7
            })
        ),
        "{result:?}"
    );
    assert_eq!(cache.load(&public).map(|index| index.serial), Some(7));
    assert_eq!(fs::read(cache.sig_path()).unwrap(), newer_sig);
}

#[test]
fn a_commit_waits_for_another_launchs_lock() {
    let (public, private) = keys();
    let (_state, cache) = seeded(&simple(5), &private, &public);
    let lock = fs::File::open(cache.index_path().with_file_name("index.json.lock")).unwrap();
    lock.lock().unwrap();

    let (bytes, sig) = signed(&simple(6), &private);
    let (done, finished) = std::sync::mpsc::channel();
    let committer = {
        let (cache, public) = (cache.clone(), public.clone());
        std::thread::spawn(move || {
            let result = cache.commit(&public, 6, &bytes, &sig);
            done.send(()).unwrap();
            result
        })
    };
    assert!(
        finished.recv_timeout(Duration::from_millis(200)).is_err(),
        "committed while another launch held the lock"
    );
    assert_eq!(cache.load(&public).map(|index| index.serial), Some(5));
    drop(lock);
    committer.join().unwrap().unwrap();
    assert_eq!(cache.load(&public).map(|index| index.serial), Some(6));
}

#[test]
fn an_interrupted_commit_keeps_the_last_good_pair() {
    let (public, private) = keys();
    let (_state, cache) = seeded(&simple(5), &private, &public);
    let (bytes, sig) = signed(&simple(7), &private);

    // The new signature is in place, then the index's rename fails.
    let failed = cache.commit_with(&public, 7, &bytes, &sig, || {
        Err(io::Error::other("interrupted"))
    });
    assert!(matches!(failed, Err(IndexError::Io(_))), "{failed:?}");
    assert_eq!(fs::read(cache.sig_path()).unwrap(), sig);
    // The kept pair still loads, and its serial still guards a rollback.
    assert_eq!(cache.load(&public).map(|index| index.serial), Some(5));
    assert_eq!(
        cache.usable_age(&public, std::time::SystemTime::now()),
        None
    );
    let (older, older_sig) = signed(&simple(4), &private);
    assert!(matches!(
        cache.commit(&public, 4, &older, &older_sig),
        Err(IndexError::Rollback {
            found: 4,
            cached: 5
        })
    ));
    assert_eq!(cache.load(&public).map(|index| index.serial), Some(5));

    // The next commit recovers, and the kept pair goes.
    cache.commit(&public, 7, &bytes, &sig).unwrap();
    assert_eq!(cache.load(&public).map(|index| index.serial), Some(7));
    assert!(
        !cache
            .index_path()
            .with_file_name("index.json.kept")
            .exists()
    );
    assert!(
        !cache
            .sig_path()
            .with_file_name("index.json.sig.kept")
            .exists()
    );
}

#[test]
fn baked_registry_toml_parses_with_a_32_byte_key() {
    let baked = baked().unwrap();
    assert_eq!(baked.index_url, URL);
    assert!(baked.url_template.contains("{tag}") && baked.url_template.contains("{file}"));
    let registry = fe_registry::grammars::Registry::parse(
        Path::new("registry.toml"),
        include_str!("../../../extensions/grammars/registry.toml"),
    )
    .unwrap();
    let key = STANDARD.decode(registry.public_key.unwrap()).unwrap();
    assert_eq!(key.len(), 32);
    assert_eq!(sig_url(&baked.index_url), format!("{URL}.sig"));
}

/// The published index, through the real client and the app's grants,
/// verified with the committed key. Run by hand, as it needs the network:
/// `cargo nextest run -p fastedit --run-ignored only live_index_verifies`.
#[test]
#[ignore = "fetches the published index"]
fn live_index_verifies() {
    let baked = baked().unwrap();
    let client = fe_net::Client::new(&crate::grammar_install::grants());
    let fetch = |url: String, max_bytes| {
        block_on(fe_net::grammar_fetch::fetch_bounded(
            &client, &url, max_bytes,
        ))
        .unwrap_or_else(|error| panic!("{url}: {error}"))
    };
    let sig = fetch(
        sig_url(&baked.index_url),
        crate::grammar_registry::MAX_SIG_BYTES,
    );
    let bytes = fetch(
        baked.index_url.clone(),
        crate::grammar_registry::MAX_INDEX_BYTES,
    );
    let index = verify_index(
        &bytes,
        std::str::from_utf8(&sig).unwrap(),
        &baked.public_key,
    )
    .unwrap();
    let catalog = Catalog::new(index, &EngineSupport::running());
    println!(
        "index serial {}: {} languages, {} this app loads, {} packs",
        catalog.index().serial,
        verify_index(
            &bytes,
            std::str::from_utf8(&sig).unwrap(),
            &baked.public_key
        )
        .unwrap()
        .languages
        .len(),
        catalog.index().languages.len(),
        catalog.index().packs.len()
    );
    assert!(!catalog.index().languages.is_empty());
}
