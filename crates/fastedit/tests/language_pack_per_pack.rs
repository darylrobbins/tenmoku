//! `scripts/package-language-pack.sh --pack`, run on a fixture grammars
//! directory with stub wasm files and an index crates/fe-registry's
//! `grammar-registry` generates from it: no wasi-sdk, no real grammar.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::OnceLock,
};

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

[packs.web]
name = "Web"
description = "Servers"
"#;

/// The fixture's languages, their packs and their licence files besides
/// LICENSE.
const LANGUAGES: [(&str, &[&str], &[&str]); 4] = [
    ("kotlin", &["jvm", "mobile"], &["LICENSE-APACHE"]),
    ("lua", &[], &[]),
    ("ruby", &["web"], &["NOTICE"]),
    ("scala", &["jvm", "functional"], &[]),
];

fn root() -> PathBuf {
    // Read at run time: a path compiled in can name a deleted snapshot.
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    Path::new(&manifest).join("../..")
}

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success() || !output.stdout.is_empty())
}

/// True if the machine can't package the pack and the run opted into skipping
/// that with `TENMOKU_SKIP_LANGUAGE_PACK=1`, as `language_pack.rs`'s does.
fn skip() -> bool {
    let missing = if !have("tar") {
        "tar"
    } else if !have("sha256sum") && !have("shasum") {
        "sha256sum or shasum"
    } else {
        return false;
    };
    let reason = format!("{missing} isn't installed, so the language pack can't be packaged");
    assert!(
        std::env::var_os("TENMOKU_SKIP_LANGUAGE_PACK").is_some_and(|value| value == "1"),
        "{reason}; set TENMOKU_SKIP_LANGUAGE_PACK=1 to skip these tests"
    );
    eprintln!("SKIPPED: {reason}");
    true
}

/// crates/fe-registry's `grammar-registry`, built once.
/// A copy of `grammar-registry` this test process alone runs. Even a build
/// with nothing to do rewrites cargo's copy of the binary, and nextest runs
/// each test in its own process: a test starting the shared copy while
/// another test's build rewrites it gets "Malformed Mach-o file" on macOS. So
/// each process builds and copies under one lock in the target directory.
fn grammar_registry() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
        let lock = fs::File::create(tmp.join("grammar-registry.lock")).unwrap();
        lock.lock().unwrap();
        let built = build_grammar_registry();
        let own = tmp.join(format!("grammar-registry-{}", std::process::id()));
        fs::copy(&built, &own).unwrap();
        own
    })
}

fn build_grammar_registry() -> PathBuf {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["build", "--quiet", "--locked", "-p", "fe-registry"])
        .args(["--bin", "grammar-registry", "--message-format=json"])
        .current_dir(root())
        .stderr(Stdio::inherit())
        .output()
        .unwrap();
    assert!(output.status.success(), "building grammar-registry failed");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let executable = (stdout.lines())
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|message| {
            let name = &message["target"]["name"];
            let path = message["executable"].as_str()?;
            (name == "grammar-registry").then(|| PathBuf::from(path))
        });
    executable.expect("cargo names grammar-registry's executable")
}

fn wasm_name(language: &str) -> String {
    format!("tree-sitter-{language}.wasm")
}

fn stub(language: &str) -> Vec<u8> {
    format!("\0asm stub for {language}").into_bytes()
}

/// The fixture's grammars, wasm and generated index in a temporary
/// directory.
struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Fixture {
        let fixture = Fixture {
            dir: tempfile::tempdir().unwrap(),
        };
        let grammars = fixture.grammars();
        fs::create_dir_all(&grammars).unwrap();
        fs::create_dir_all(fixture.wasm()).unwrap();
        fs::write(grammars.join("registry.toml"), REGISTRY).unwrap();
        for (language, packs, licences) in LANGUAGES {
            let dir = grammars.join(language);
            fs::create_dir_all(dir.join("test")).unwrap();
            let packs: Vec<String> = packs.iter().map(|pack| format!("{pack:?}")).collect();
            let manifest = format!(
                "id = \"tenmoku.grammar_{language}\"\n\
                 name = \"{language} syntax\"\n\
                 version = \"1.0.0\"\n\
                 license = \"MIT\"\n\
                 \n\
                 [engine]\n\
                 tenmoku = \"1.0\"\n\
                 \n\
                 [[contributes.grammars]]\n\
                 language = \"{language}\"\n\
                 wasm = \"{}\"\n\
                 extensions = [\"{language}\"]\n\
                 highlights = \"highlights.scm\"\n\
                 injections = \"injections.scm\"\n\
                 packs = [{}]\n",
                wasm_name(language),
                packs.join(", ")
            );
            fs::write(dir.join("extension.toml"), manifest).unwrap();
            fs::write(dir.join("highlights.scm"), format!("; {language}\n")).unwrap();
            fs::write(dir.join("injections.scm"), "; none\n").unwrap();
            fs::write(dir.join("grammar-source.toml"), "crate = \"x\"\n").unwrap();
            fs::write(dir.join("test/sample.x"), "sample\n").unwrap();
            for licence in ["LICENSE"].iter().chain(licences) {
                fs::write(dir.join(licence), format!("{language}'s {licence}\n")).unwrap();
            }
            let wasm = fixture.wasm().join(wasm_name(language));
            fs::write(&wasm, stub(language)).unwrap();
            fs::write(
                fixture.wasm().join(format!("{}.abi", wasm_name(language))),
                "14\n",
            )
            .unwrap();
        }
        let generated = Command::new(grammar_registry())
            .arg("generate")
            .arg("--grammars")
            .arg(&grammars)
            .arg("--wasm")
            .arg(fixture.wasm())
            .arg("--out")
            .arg(fixture.registry())
            .args(["--serial", "1"])
            .output()
            .unwrap();
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        fixture
    }

    fn grammars(&self) -> PathBuf {
        self.dir.path().join("grammars")
    }

    fn wasm(&self) -> PathBuf {
        self.dir.path().join("wasm")
    }

    fn registry(&self) -> PathBuf {
        self.dir.path().join("registry")
    }

    fn dist(&self) -> PathBuf {
        self.dir.path().join("dist")
    }

    /// Runs the script with `args` on the fixture.
    fn package(&self, args: &[&str]) -> Output {
        self.package_into(&self.dist(), "022", "UTC", args)
    }

    /// Runs the script with `args` on the fixture, writing into `dist`,
    /// under that umask and time zone.
    fn package_into(&self, dist: &Path, umask: &str, tz: &str, args: &[&str]) -> Output {
        Command::new("sh")
            .args(["-c", "umask \"$0\" && exec \"$@\"", umask])
            .arg(root().join("scripts/package-language-pack.sh"))
            .args(args)
            .env("GRAMMARS_DIR", self.grammars())
            .env("GRAMMAR_WASM_DIR", self.wasm())
            .env("GRAMMAR_INDEX", self.registry().join("index.json"))
            .env("GRAMMAR_REGISTRY", grammar_registry())
            .env("DIST_DIR", dist)
            .env("TZ", tz)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// The files in `archive`, under dist/, without the directories.
    fn listing(&self, archive: &str) -> BTreeSet<String> {
        let listing = Command::new("tar")
            .arg("-tzf")
            .arg(self.dist().join(archive))
            .output()
            .unwrap();
        assert!(listing.status.success());
        (String::from_utf8(listing.stdout).unwrap().lines())
            .map(|line| line.trim_start_matches("./").to_owned())
            .filter(|line| !line.ends_with('/'))
            .collect()
    }

    /// What the tarball has of each of `languages`: its artifact.
    fn expected(&self, languages: &[&str]) -> BTreeSet<String> {
        let mut expected = BTreeSet::new();
        for (language, _, licences) in LANGUAGES {
            if !languages.contains(&language) {
                continue;
            }
            let files = [
                "extension.toml",
                "highlights.scm",
                "injections.scm",
                "LICENSE",
            ];
            let wasm = wasm_name(language);
            for file in files.iter().chain(licences).chain([&wasm.as_str()]) {
                expected.insert(format!("tenmoku.grammar_{language}/{file}"));
            }
        }
        expected
    }
}

fn succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn pack_packs_only_its_languages_with_their_licenses() {
    if skip() {
        return;
    }
    let fixture = Fixture::new();
    let output = fixture.package(&["--pack", "jvm"]);
    succeeded(&output);
    let archive = "Tenmoku-language-pack-jvm.tar.gz";
    let files = fixture.listing(archive);
    assert_eq!(files, fixture.expected(&["kotlin", "scala"]));
    assert!(
        files.contains("tenmoku.grammar_kotlin/LICENSE-APACHE"),
        "{files:?}"
    );
    assert!(files.contains("tenmoku.grammar_scala/LICENSE"), "{files:?}");

    // Its checksum file is beside it, and nothing else is written.
    let written: BTreeSet<String> = (fs::read_dir(fixture.dist()).unwrap())
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(
        written,
        BTreeSet::from([archive.to_owned(), format!("{archive}.sha256")])
    );
    let sums = fs::read_to_string(fixture.dist().join(format!("{archive}.sha256"))).unwrap();
    assert!(sums.trim_end().ends_with(archive), "{sums}");

    // Another pack's own languages.
    succeeded(&fixture.package(&["--pack", "web"]));
    assert_eq!(
        fixture.listing("Tenmoku-language-pack-web.tar.gz"),
        fixture.expected(&["ruby"])
    );
}

#[test]
fn unknown_pack_fails_naming_it() {
    if skip() {
        return;
    }
    let fixture = Fixture::new();
    let output = fixture.package(&["--pack", "android"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("android"), "{stderr}");
    assert!(
        !fixture
            .dist()
            .join("Tenmoku-language-pack-android.tar.gz")
            .exists()
    );

    // An empty pack name isn't taken as no --pack.
    let output = fixture.package(&["--pack", ""]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--pack needs a pack's name"), "{stderr}");
    assert!(!fixture.dist().exists(), "it wrote an archive");
}

/// The same inputs give byte-identical tarballs, whatever the clock, umask
/// or time zone: with GNU tar here, with BSD tar on macOS.
#[test]
fn same_inputs_give_identical_tarballs() {
    if skip() {
        return;
    }
    let fixture = Fixture::new();
    let first = fixture.dir.path().join("first");
    let second = fixture.dir.path().join("second");
    for args in [&["--pack", "jvm"][..], &[]] {
        succeeded(&fixture.package_into(&first, "022", "UTC", args));
    }
    // The stage's files are made a second or more later.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    for args in [&["--pack", "jvm"][..], &[]] {
        succeeded(&fixture.package_into(&second, "077", "Pacific/Auckland", args));
    }
    for archive in [
        "Tenmoku-language-pack-jvm.tar.gz",
        "Tenmoku-language-pack.tar.gz",
    ] {
        let bytes = fs::read(first.join(archive)).unwrap();
        assert!(
            bytes == fs::read(second.join(archive)).unwrap(),
            "{archive} differs between runs"
        );
        let sums = format!("{archive}.sha256");
        assert_eq!(
            fs::read(first.join(&sums)).unwrap(),
            fs::read(second.join(&sums)).unwrap()
        );
    }
    // Fixed owner, modes and times, in GNU tar's verbose listing (BSD tar's
    // has another format).
    let version = Command::new("tar").arg("--version").output().unwrap();
    if !String::from_utf8_lossy(&version.stdout).contains("GNU") {
        return;
    }
    let listing = Command::new("tar")
        .arg("-tvzf")
        .arg(first.join("Tenmoku-language-pack-jvm.tar.gz"))
        .env("TZ", "UTC")
        .output()
        .unwrap();
    assert!(listing.status.success());
    let listing = String::from_utf8(listing.stdout).unwrap();
    for line in listing.lines() {
        let expected = if line.ends_with('/') {
            "drwxr-xr-x 0/0"
        } else {
            "-rw-r--r-- 0/0"
        };
        assert!(line.starts_with(expected), "{line}");
        assert!(line.contains("2000-01-01 00:00"), "{line}");
    }
}

#[test]
fn no_pack_packs_every_language() {
    if skip() {
        return;
    }
    let fixture = Fixture::new();
    // The index isn't needed for it.
    fs::remove_dir_all(fixture.registry()).unwrap();
    let output = fixture.package(&[]);
    succeeded(&output);
    let languages: Vec<&str> = LANGUAGES.iter().map(|(language, ..)| *language).collect();
    assert_eq!(
        fixture.listing("Tenmoku-language-pack.tar.gz"),
        fixture.expected(&languages)
    );
    // Licence files are packed as they are.
    let unpacked = tempfile::tempdir().unwrap();
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(fixture.dist().join("Tenmoku-language-pack.tar.gz"))
        .arg("-C")
        .arg(unpacked.path())
        .status()
        .unwrap();
    assert!(status.success());
    for (language, _, licences) in LANGUAGES {
        for licence in ["LICENSE"].iter().chain(licences) {
            let path = format!("{language}/{licence}");
            assert_eq!(
                fs::read(unpacked.path().join(format!("tenmoku.grammar_{path}"))).unwrap(),
                fs::read(fixture.grammars().join(&path)).unwrap(),
                "{path}"
            );
        }
    }
}
