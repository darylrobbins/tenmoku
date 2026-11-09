//! `fe_registry::grammars`, run on fixture grammar directories with fake
//! modules and `.abi` files: no wasm build.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use fe_registry::grammars::{
    self, CAPTURES, Error, FileEntry, Generated, INDEX_FILE, Index, LANGUAGES_DIR, Registry, SCHEMA,
};
use sha2::{Digest, Sha256};

const REGISTRY: &str = r#"schema = 1
url_template = "https://example.com/{tag}/{file}"
index_url = "https://example.com/index/index.json"

[packs.jvm]
name = "JVM"
description = "Kotlin and friends"

[packs.mobile]
name = "Mobile"
description = "Phone apps"

[packs.functional]
name = "Functional"
description = "Functions"

[packs.shaders]
name = "Shaders"
description = "No language lists it"
"#;

const SERIAL: u64 = 1_762_430_400;

/// A grammars directory and a modules directory in a temporary directory.
struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Fixture {
        let fixture = Fixture {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(fixture.grammars()).unwrap();
        fs::create_dir_all(fixture.wasm()).unwrap();
        fs::write(fixture.grammars().join("registry.toml"), REGISTRY).unwrap();
        fs::write(fixture.grammars().join("README.md"), "not a language\n").unwrap();
        fixture
    }

    fn grammars(&self) -> PathBuf {
        self.dir.path().join("grammars")
    }

    fn wasm(&self) -> PathBuf {
        self.dir.path().join("wasm")
    }

    fn out(&self) -> PathBuf {
        self.dir.path().join("out")
    }

    fn language_dir(&self, language: &str) -> PathBuf {
        self.grammars().join(language)
    }

    fn wasm_name(language: &str) -> String {
        format!("tree-sitter-{language}.wasm")
    }

    /// A language directory as a first-party one is, its module and its
    /// `.abi` (14).
    fn language(&self, language: &str, packs: &[&str]) -> &Fixture {
        let dir = self.language_dir(language);
        fs::create_dir_all(dir.join("test")).unwrap();
        let packs = (packs.iter())
            .map(|pack| format!("{pack:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let packs = if packs.is_empty() {
            String::new()
        } else {
            format!("packs = [{packs}]\n")
        };
        let manifest = format!(
            "id = \"tenmoku.grammar_{language}\"\n\
             name = \"{} syntax\"\n\
             version = \"0.4.1\"\n\
             license = \"MIT\"\n\
             \n\
             [engine]\n\
             tenmoku = \"1.0\"\n\
             \n\
             [[contributes.grammars]]\n\
             language = \"{language}\"\n\
             wasm = \"{}\"\n\
             extensions = [\"{language}x\", \"{language}s\"]\n\
             file_names = [\"{language}file\"]\n\
             shebangs = [\"{language}sh\"]\n\
             highlights = \"highlights.scm\"\n\
             injections = \"injections.scm\"\n\
             {packs}\
             line_comment = \"//\"\n",
            language.to_uppercase(),
            Fixture::wasm_name(language),
        );
        fs::write(dir.join("extension.toml"), manifest).unwrap();
        fs::write(
            dir.join("highlights.scm"),
            format!("; {language}\n(x) @keyword\n"),
        )
        .unwrap();
        fs::write(dir.join("injections.scm"), "; none\n").unwrap();
        fs::write(dir.join("LICENSE"), format!("{language}'s licence\n")).unwrap();
        fs::write(dir.join("grammar-source.toml"), "crate = \"x\"\n").unwrap();
        fs::write(dir.join("test/sample.x"), "sample\n").unwrap();
        fs::write(dir.join("test/sample.x.golden"), "golden\n").unwrap();
        let wasm = self.wasm().join(Fixture::wasm_name(language));
        fs::write(&wasm, format!("\0asm {language}")).unwrap();
        fs::write(self.abi(language), "14\n").unwrap();
        self
    }

    fn abi(&self, language: &str) -> PathBuf {
        self.wasm()
            .join(format!("{}.abi", Fixture::wasm_name(language)))
    }

    fn generate(&self) -> Result<Generated, Error> {
        grammars::generate(&self.grammars(), &self.wasm(), SERIAL)
    }

    /// Generates and writes into `out/`.
    fn write(&self) -> Generated {
        let generated = self.generate().unwrap();
        generated.write(&self.out()).unwrap();
        generated
    }
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn names(files: &[FileEntry]) -> Vec<&str> {
    files.iter().map(|file| file.name.as_str()).collect()
}

/// The files in a directory, by name, with their bytes.
fn read_dir(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    (fs::read_dir(dir).unwrap())
        .map(|entry| {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file(), "{:?}", entry.path());
            (
                entry.file_name().into_string().unwrap(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn entry_lists_every_artifact_file_with_its_hash() {
    let fixture = Fixture::new();
    fixture.language("kotlin", &["jvm", "mobile"]);
    let dir = fixture.language_dir("kotlin");
    fs::write(dir.join("LICENSE-APACHE"), "Apache\n").unwrap();
    fs::write(dir.join("NOTICE"), "notice\n").unwrap();
    let generated = fixture.write();
    let entry = &generated.index.languages["kotlin"];

    assert_eq!(
        names(&entry.files),
        [
            "LICENSE",
            "LICENSE-APACHE",
            "NOTICE",
            "extension.toml",
            "highlights.scm",
            "injections.scm",
            "tree-sitter-kotlin.wasm",
        ]
    );
    for file in &entry.files {
        let source = if file.name.ends_with(".wasm") {
            fixture.wasm().join(&file.name)
        } else {
            dir.join(&file.name)
        };
        let bytes = fs::read(source).unwrap();
        assert_eq!(file.size, bytes.len() as u64, "{}", file.name);
        assert_eq!(file.sha256, sha256(&bytes), "{}", file.name);
    }
    // The artifact on disk is exactly those files.
    let written = read_dir(&fixture.out().join(LANGUAGES_DIR).join("kotlin"));
    assert_eq!(
        written.keys().map(String::as_str).collect::<Vec<_>>(),
        names(&entry.files)
    );
    for file in &entry.files {
        assert_eq!(sha256(&written[&file.name]), file.sha256, "{}", file.name);
    }

    assert_eq!(entry.name, "KOTLIN");
    assert_eq!(entry.version, "0.4.1");
    assert_eq!(entry.license, "MIT");
    assert_eq!(entry.extensions, ["kotlinx", "kotlins"]);
    assert_eq!(entry.file_names, ["kotlinfile"]);
    assert_eq!(entry.shebangs, ["kotlinsh"]);
    assert_eq!(entry.packs, ["jvm", "mobile"]);
    assert_eq!(
        (
            entry.engine.tenmoku.as_str(),
            entry.engine.abi,
            entry.engine.captures
        ),
        ("1.0", 14, CAPTURES)
    );
    let index = &generated.index;
    assert_eq!((index.schema, index.serial), (SCHEMA, SERIAL));
    assert_eq!(index.url_template, "https://example.com/{tag}/{file}");
}

#[test]
fn digest_and_tag_change_with_any_file() {
    let baseline = {
        let fixture = Fixture::new();
        fixture
            .language("kotlin", &["jvm"])
            .language("scala", &["jvm"]);
        fixture.generate().unwrap().index
    };
    let kotlin = &baseline.languages["kotlin"];
    // The digest is over the files' lines, in order, and the tag is its prefix.
    let lines: String = (kotlin.files.iter())
        .map(|file| format!("{}\t{}\t{}\n", file.name, file.size, file.sha256))
        .collect();
    assert_eq!(kotlin.digest, sha256(lines.as_bytes()));
    assert_eq!(kotlin.tag, format!("kotlin-{}", &kotlin.digest[..16]));

    for file in names(&kotlin.files) {
        let fixture = Fixture::new();
        fixture
            .language("kotlin", &["jvm"])
            .language("scala", &["jvm"]);
        let path = if file.ends_with(".wasm") {
            fixture.wasm().join(file)
        } else {
            fixture.language_dir("kotlin").join(file)
        };
        let mut bytes = fs::read(&path).unwrap();
        bytes.push(b'\n');
        fs::write(&path, bytes).unwrap();
        let index = fixture.generate().unwrap().index;
        let changed = &index.languages["kotlin"];
        assert_ne!(changed.digest, kotlin.digest, "{file}");
        assert_ne!(changed.tag, kotlin.tag, "{file}");
        assert!(changed.tag.starts_with("kotlin-"), "{}", changed.tag);
        // The other language is the same release.
        assert_eq!(
            index.languages["scala"], baseline.languages["scala"],
            "{file}"
        );
    }

    // A file the artifact doesn't have changes nothing.
    let fixture = Fixture::new();
    fixture
        .language("kotlin", &["jvm"])
        .language("scala", &["jvm"]);
    fs::write(
        fixture.language_dir("kotlin").join("test/sample.x"),
        "other\n",
    )
    .unwrap();
    assert_eq!(fixture.generate().unwrap().index, baseline);
}

#[test]
fn same_inputs_give_identical_bytes() {
    let build = || {
        let fixture = Fixture::new();
        fixture.language("scala", &["jvm", "functional"]);
        fixture.language("kotlin", &["jvm", "mobile"]);
        fixture.language("lua", &[]);
        fs::write(fixture.language_dir("kotlin").join("NOTICE"), "n\n").unwrap();
        fixture.write();
        // Twice into the same directory, as a rebuild does.
        fixture.write();
        let index = fs::read(fixture.out().join(INDEX_FILE)).unwrap();
        let artifacts: BTreeMap<String, BTreeMap<String, Vec<u8>>> =
            (fs::read_dir(fixture.out().join(LANGUAGES_DIR)).unwrap())
                .map(|entry| {
                    let entry = entry.unwrap();
                    let name = entry.file_name().into_string().unwrap();
                    (name, read_dir(&entry.path()))
                })
                .collect();
        (fixture, index, artifacts)
    };
    let (_first, index, artifacts) = build();
    let (_second, again, artifacts_again) = build();
    assert_eq!(
        String::from_utf8(index.clone()).unwrap(),
        String::from_utf8(again).unwrap()
    );
    assert_eq!(artifacts, artifacts_again);
    assert_eq!(
        artifacts.keys().map(String::as_str).collect::<Vec<_>>(),
        ["kotlin", "lua", "scala"]
    );
    assert!(index.ends_with(b"}\n"));
    // No temporary path or time is in it: only the serial.
    let text = String::from_utf8(index).unwrap();
    assert!(!text.contains("tmp"), "{text}");
    assert!(text.contains(&format!("\"serial\": {SERIAL},")), "{text}");
}

#[test]
fn unknown_pack_is_refused() {
    let fixture = Fixture::new();
    fixture.language("kotlin", &["jvm", "android"]);
    match fixture.generate() {
        Err(error @ Error::UnknownPack { .. }) => {
            let Error::UnknownPack { language, pack, .. } = &error else {
                unreachable!()
            };
            assert_eq!((language.as_str(), pack.as_str()), ("kotlin", "android"));
            let message = error.to_string();
            assert!(message.contains("\"android\""), "{message}");
            assert!(message.contains("kotlin"), "{message}");
            assert!(message.contains("registry.toml"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    assert!(!fixture.out().exists());

    // A pack listed twice is refused too.
    let fixture = Fixture::new();
    fixture.language("kotlin", &["jvm", "jvm"]);
    let error = fixture.generate().unwrap_err().to_string();
    assert!(error.contains("jvm twice"), "{error}");
}

#[test]
fn language_without_packs_is_listed_in_no_pack() {
    let fixture = Fixture::new();
    fixture.language("kotlin", &["jvm"]).language("lua", &[]);
    let generated = fixture.generate().unwrap();
    let lua = &generated.index.languages["lua"];
    assert!(lua.packs.is_empty());
    assert_eq!(lua.engine.abi, 14);
    for (name, pack) in &generated.index.packs {
        assert!(!pack.languages.contains(&"lua".to_owned()), "{name}");
    }
    assert_eq!(generated.warnings.len(), 1, "{:?}", generated.warnings);
    assert!(
        generated.warnings[0].contains("lua"),
        "{:?}",
        generated.warnings
    );
    // It's still written, and the index holds together.
    generated.write(&fixture.out()).unwrap();
    assert!(
        fixture
            .out()
            .join(LANGUAGES_DIR)
            .join("lua/LICENSE")
            .is_file()
    );
    generated.index.check().unwrap();
}

#[test]
fn pack_lists_its_languages_sorted() {
    let fixture = Fixture::new();
    fixture.language("scala", &["functional", "jvm"]);
    fixture.language("kotlin", &["jvm", "mobile"]);
    fixture.language("clojure", &["jvm", "functional"]);
    let index = fixture.generate().unwrap().index;
    assert_eq!(index.packs["jvm"].languages, ["clojure", "kotlin", "scala"]);
    assert_eq!(index.packs["functional"].languages, ["clojure", "scala"]);
    assert_eq!(index.packs["mobile"].languages, ["kotlin"]);
    let jvm = &index.packs["jvm"];
    assert_eq!(
        (jvm.name.as_str(), jvm.description.as_str()),
        ("JVM", "Kotlin and friends")
    );
    assert_eq!(jvm.tarball, None);
    // A language's own list keeps its order: the first is its main pack.
    assert_eq!(index.languages["scala"].packs, ["functional", "jvm"]);
    assert_eq!(index.languages["clojure"].packs, ["jvm", "functional"]);
    // A defined pack no language lists isn't in the index.
    assert_eq!(
        index.packs.keys().map(String::as_str).collect::<Vec<_>>(),
        ["functional", "jvm", "mobile"]
    );
}

#[test]
fn missing_or_bad_abi_is_refused() {
    let fixture = Fixture::new();
    fixture.language("kotlin", &["jvm"]);
    fs::remove_file(fixture.abi("kotlin")).unwrap();
    let error = fixture.generate().unwrap_err().to_string();
    assert!(error.contains("tree-sitter-kotlin.wasm.abi"), "{error}");
    assert!(error.contains("missing"), "{error}");

    for bad in [
        "",
        "\n",
        "fourteen\n",
        "14\n15\n",
        "-1\n",
        " 14\n",
        "14 \n",
        "99999999999\n",
    ] {
        fs::write(fixture.abi("kotlin"), bad).unwrap();
        let error = fixture.generate().unwrap_err().to_string();
        assert!(
            error.contains("tree-sitter-kotlin.wasm.abi"),
            "{bad:?}: {error}"
        );
    }
    for good in ["15\n", "15"] {
        fs::write(fixture.abi("kotlin"), good).unwrap();
        let index = fixture.generate().unwrap().index;
        assert_eq!(index.languages["kotlin"].engine.abi, 15, "{good:?}");
    }

    // A missing module is refused naming the script that builds it.
    fs::remove_file(fixture.wasm().join("tree-sitter-kotlin.wasm")).unwrap();
    let error = fixture.generate().unwrap_err().to_string();
    assert!(error.contains("scripts/build-grammar-wasm.sh"), "{error}");
}

#[test]
fn grammar_source_and_test_files_are_not_in_the_artifact() {
    let fixture = Fixture::new();
    fixture.language("kotlin", &["jvm"]);
    let dir = fixture.language_dir("kotlin");
    fs::write(dir.join("notes.txt"), "stray\n").unwrap();
    fs::create_dir(dir.join("LICENSE-dir")).unwrap();
    let generated = fixture.write();
    let files = names(&generated.index.languages["kotlin"].files);
    assert_eq!(
        files,
        [
            "LICENSE",
            "extension.toml",
            "highlights.scm",
            "injections.scm",
            "tree-sitter-kotlin.wasm",
        ]
    );
    let written = read_dir(&fixture.out().join(LANGUAGES_DIR).join("kotlin"));
    assert!(!written.contains_key("grammar-source.toml"));
    assert!(!written.contains_key("test"));
    assert_eq!(written.len(), files.len());

    // A stale artifact file from an earlier run goes.
    let stale = fixture.out().join(LANGUAGES_DIR).join("kotlin/old.wasm");
    fs::write(&stale, "old").unwrap();
    fixture.write();
    assert!(!stale.exists());

    // No LICENSE is refused.
    fs::remove_file(dir.join("LICENSE")).unwrap();
    let error = fixture.generate().unwrap_err().to_string();
    assert!(error.contains("LICENSE"), "{error}");
}

#[test]
fn index_round_trips_through_its_types() {
    let fixture = Fixture::new();
    fixture
        .language("kotlin", &["jvm", "mobile"])
        .language("scala", &["jvm"]);
    fixture.language("lua", &[]);
    let generated = fixture.write();
    let packs = fixture.dir.path().join("packs");
    fs::create_dir(&packs).unwrap();
    for pack in generated.index.packs.keys() {
        fs::write(
            packs.join(grammars::tarball_name(pack)),
            format!("tarball {pack}"),
        )
        .unwrap();
    }

    let path = fixture.out().join(INDEX_FILE);
    let text = fs::read_to_string(&path).unwrap();
    let mut index = Index::parse(&text).unwrap();
    assert_eq!(index, generated.index);
    assert_eq!(index.to_json(), text);

    grammars::finalize(&mut index, &packs).unwrap();
    let jvm = index.packs["jvm"].tarball.clone().unwrap();
    assert_eq!(
        jvm,
        FileEntry::of("Tenmoku-language-pack-jvm.tar.gz", b"tarball jvm")
    );
    grammars::write_index(&path, &index).unwrap();
    let finalized = fs::read_to_string(&path).unwrap();
    assert_eq!(Index::parse(&finalized).unwrap(), index);
    assert!(finalized.contains("\"tarball\": {"), "{finalized}");

    // A key the types don't have, another schema, and a tampered entry are
    // refused.
    let unknown = finalized.replacen("\"serial\"", "\"signed\": true,\n  \"serial\"", 1);
    let error = Index::parse(&unknown).unwrap_err().to_string();
    assert!(error.contains("signed"), "{error}");
    let schema = finalized.replacen("\"schema\": 1", "\"schema\": 2", 1);
    assert!(matches!(Index::parse(&schema), Err(Error::Schema { .. })));
    let file = &index.languages["kotlin"].files[0];
    let tampered = finalized.replacen(&file.sha256, &"0".repeat(64), 1);
    assert!(matches!(Index::parse(&tampered), Err(Error::Index(_))));
    let escaping = finalized.replacen("\"name\": \"LICENSE\"", "\"name\": \"../LICENSE\"", 1);
    assert!(matches!(Index::parse(&escaping), Err(Error::Index(_))));

    // finalize refuses a missing tarball.
    fs::remove_file(packs.join(grammars::tarball_name("mobile"))).unwrap();
    let error = grammars::finalize(&mut index, &packs)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("Tenmoku-language-pack-mobile.tar.gz"),
        "{error}"
    );
}

/// The repository's grammars: every pack an `extension.toml` lists is one
/// `registry.toml` defines, and the first eight have theirs.
#[test]
fn every_first_party_pack_is_defined() {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    let grammars = Path::new(&manifest).join("../../extensions/grammars");
    let registry = Registry::read(&grammars).unwrap();
    let defined: Vec<&str> = registry.packs.keys().map(String::as_str).collect();
    let mut expected = [
        "shells",
        "web",
        "infra",
        "functional",
        "jvm",
        "dotnet",
        "systems",
        "mobile",
        "data",
        "docs",
        "shaders",
        "hardware",
        "config",
        "scripting",
    ];
    expected.sort();
    assert_eq!(defined, expected);
    for (key, pack) in &registry.packs {
        assert!(
            !pack.name.is_empty() && !pack.description.is_empty(),
            "{key}"
        );
    }
    assert!(registry.url_template.contains("{tag}") && registry.url_template.contains("{file}"));
    assert!(registry.index_url.ends_with("/index.json"));

    let packs = grammars::manifest_packs(&grammars).unwrap();
    assert!(packs.len() >= 8, "{packs:?}");
    for (language, listed) in &packs {
        for pack in listed {
            assert!(
                registry.packs.contains_key(pack),
                "{language} lists the pack {pack}, which registry.toml doesn't define"
            );
        }
    }
    for (language, theirs) in [
        ("kotlin", &["jvm", "mobile"][..]),
        ("scala", &["jvm", "functional"]),
        ("csharp", &["dotnet"]),
        ("swift", &["mobile"]),
        ("cpp", &["systems"]),
        ("sql", &["data"]),
        ("php", &["web"]),
        ("ruby", &["web", "scripting"]),
    ] {
        assert_eq!(packs[language], theirs, "{language}");
    }
}
