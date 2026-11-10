//! `fe_registry::signing` and the `grammar-registry` binary's `sign` and
//! `verify`, with throwaway keys and a temporary `registry.toml`.

use std::{
    collections::BTreeMap,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use fe_registry::{
    grammars::{self, Engine, FileEntry, Index, Language, Pack},
    signing::{self, Error},
};

const BIN: &str = env!("CARGO_BIN_EXE_grammar-registry");

/// A directory with a `registry.toml` holding `public_key` and an index.
struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    /// With a new key pair's public half in `registry.toml`; returns the
    /// private half too.
    fn new() -> (Fixture, String) {
        let (public, private) = signing::generate_key();
        (Fixture::with_public_key(Some(&public)), private)
    }

    fn with_public_key(public: Option<&str>) -> Fixture {
        let fixture = Fixture {
            dir: tempfile::tempdir().unwrap(),
        };
        let key = public.map_or(String::new(), |key| format!("public_key = \"{key}\"\n"));
        fs::write(
            fixture.registry_toml(),
            format!(
                "schema = 1\nurl_template = \"https://example.com/{{tag}}/{{file}}\"\n\
                 index_url = \"https://example.com/index/index.json\"\n{key}\n\
                 [packs.jvm]\nname = \"JVM\"\ndescription = \"Kotlin\"\n"
            ),
        )
        .unwrap();
        fs::write(fixture.index(), index().to_json()).unwrap();
        fixture
    }

    fn registry_toml(&self) -> PathBuf {
        self.dir.path().join("registry.toml")
    }

    fn index(&self) -> PathBuf {
        self.dir.path().join("index.json")
    }

    fn sig(&self) -> PathBuf {
        self.dir.path().join("index.json.sig")
    }

    /// The files in the directory, sorted.
    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    /// Runs the binary with `args`, `stdin` on its stdin, from the
    /// fixture's directory, with no inherited private key variable.
    fn run(&self, args: &[&str], stdin: &str, env: &[(&str, &str)]) -> Output {
        let mut child = Command::new(BIN)
            .args(args)
            .current_dir(self.dir.path())
            .env_remove("TENMOKU_REGISTRY_PRIVATE_KEY")
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn sign(&self, stdin: &str) -> Output {
        self.run(
            &[
                "sign",
                "--index",
                "index.json",
                "--registry-toml",
                "registry.toml",
            ],
            stdin,
            &[],
        )
    }

    fn verify(&self) -> Output {
        self.run(
            &[
                "verify",
                "--index",
                "index.json",
                "--registry-toml",
                "registry.toml",
            ],
            "",
            &[],
        )
    }
}

/// An index with one language, which parses.
fn index() -> Index {
    let files = vec![
        FileEntry::of("LICENSE", b"MIT\n"),
        FileEntry::of("tree-sitter-kotlin.wasm", b"\0asm"),
    ];
    let digest = grammars::digest(&files);
    let language = Language {
        name: "Kotlin".into(),
        version: "0.4.1".into(),
        tag: grammars::tag("kotlin", &digest),
        digest,
        extensions: vec!["kt".into()],
        file_names: vec![],
        shebangs: vec![],
        packs: vec!["jvm".into()],
        license: "MIT".into(),
        engine: Engine {
            tenmoku: "1.0".into(),
            abi: 14,
            captures: grammars::CAPTURES,
        },
        files,
    };
    let pack = Pack {
        name: "JVM".into(),
        description: "Kotlin".into(),
        languages: vec!["kotlin".into()],
        tarball: None,
    };
    Index {
        schema: grammars::SCHEMA,
        serial: 1_762_430_400,
        url_template: "https://example.com/{tag}/{file}".into(),
        packs: BTreeMap::from([("jvm".into(), pack)]),
        languages: BTreeMap::from([("kotlin".into(), language)]),
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Failed with an error message, exit 1, not a panic (101).
fn assert_refused(output: &Output) {
    let stderr = stderr(output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.starts_with("grammar-registry: "), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn sign_then_verify_round_trips() {
    let (fixture, private) = Fixture::new();
    let sig = signing::sign_index(&fixture.index(), &fixture.registry_toml(), &private).unwrap();
    assert_eq!(sig, fixture.sig());
    // The base64 of 64 bytes and a newline, and no temporary file left.
    let text = fs::read_to_string(&sig).unwrap();
    assert!(
        text.ends_with('\n') && text.trim_end().len() == 88,
        "{text:?}"
    );
    assert_eq!(
        fixture.files(),
        ["index.json", "index.json.sig", "registry.toml"]
    );
    signing::verify_index(&fixture.index(), None, &fixture.registry_toml()).unwrap();
    signing::verify_index(&fixture.index(), Some(&sig), &fixture.registry_toml()).unwrap();

    // Ed25519 is deterministic: the binary, given the key on stdin, writes
    // the same signature, and its verify agrees.
    fs::remove_file(&sig).unwrap();
    let signed = fixture.sign(&format!("{private}\n"));
    assert!(signed.status.success(), "{}", stderr(&signed));
    assert_eq!(fs::read_to_string(&sig).unwrap(), text);
    let verified = fixture.verify();
    assert!(verified.status.success(), "{}", stderr(&verified));

    // A sig elsewhere, by --sig.
    let moved = fixture.dir.path().join("moved.sig");
    fs::rename(&sig, &moved).unwrap();
    let output = fixture.run(
        &[
            "verify",
            "--index",
            "index.json",
            "--sig",
            "moved.sig",
            "--registry-toml",
            "registry.toml",
        ],
        "",
        &[],
    );
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn a_key_not_matching_registry_toml_is_refused_before_writing() {
    let (fixture, _) = Fixture::new();
    let (other_public, other_private) = signing::generate_key();
    let error = signing::sign_index(&fixture.index(), &fixture.registry_toml(), &other_private)
        .unwrap_err();
    assert!(matches!(error, Error::KeyMismatch { .. }), "{error:?}");
    // Names the key's public half, which isn't secret, and not the private one.
    let message = error.to_string();
    assert!(message.contains(&other_public), "{message}");
    assert!(!message.contains(&other_private), "{message}");
    assert_eq!(fixture.files(), ["index.json", "registry.toml"]);

    let output = fixture.sign(&other_private);
    assert_refused(&output);
    assert!(
        stderr(&output).contains("nothing was written"),
        "{}",
        stderr(&output)
    );
    assert_eq!(fixture.files(), ["index.json", "registry.toml"]);

    // A registry.toml without a key, or with a malformed one, signs nothing.
    for public in [None, Some("not base64!"), Some("AAAA")] {
        let fixture = Fixture::with_public_key(public);
        let error = signing::sign_index(&fixture.index(), &fixture.registry_toml(), &other_private)
            .unwrap_err();
        assert!(
            matches!(error, Error::NoPublicKey { .. } | Error::PublicKey { .. }),
            "{public:?}: {error:?}"
        );
        assert_eq!(fixture.files(), ["index.json", "registry.toml"]);
    }

    // Nor does a matching key over a file that isn't an index.
    let (fixture, private) = Fixture::new();
    fs::write(fixture.index(), "{\"schema\": 2}\n").unwrap();
    let error =
        signing::sign_index(&fixture.index(), &fixture.registry_toml(), &private).unwrap_err();
    assert!(matches!(error, Error::Index { .. }), "{error:?}");
    assert_eq!(fixture.files(), ["index.json", "registry.toml"]);
}

/// The committed key is a 32-byte key: a fresh key is refused as not it,
/// rather than the registry's key being malformed or missing.
#[test]
fn the_committed_registry_toml_has_a_public_key() {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    let registry_toml = Path::new(&manifest).join("../../extensions/grammars/registry.toml");
    let (fixture, private) = Fixture::new();
    let error = signing::sign_index(&fixture.index(), &registry_toml, &private).unwrap_err();
    assert!(matches!(error, Error::KeyMismatch { .. }), "{error:?}");
}

#[test]
fn tampered_index_fails_verify() {
    let (fixture, private) = Fixture::new();
    signing::sign_index(&fixture.index(), &fixture.registry_toml(), &private).unwrap();
    let signed = fs::read_to_string(fixture.index()).unwrap();

    // A changed serial, a changed field, and one more byte: the signature is
    // over the exact bytes.
    let serial = signed.replace("1762430400", "1762430401");
    let version = signed.replace("\"0.4.1\"", "\"0.4.2\"");
    for tampered in [serial, version, format!("{signed}\n")] {
        assert_ne!(tampered, signed);
        fs::write(fixture.index(), &tampered).unwrap();
        let error =
            signing::verify_index(&fixture.index(), None, &fixture.registry_toml()).unwrap_err();
        assert!(matches!(error, Error::Signature { .. }), "{error:?}");
        assert_refused(&fixture.verify());
    }

    // Signed by a key that isn't registry.toml's.
    fs::write(fixture.index(), &signed).unwrap();
    let (other_public, other_private) = signing::generate_key();
    let other = Fixture::with_public_key(Some(&other_public));
    fs::write(other.index(), &signed).unwrap();
    signing::sign_index(&other.index(), &other.registry_toml(), &other_private).unwrap();
    fs::copy(other.sig(), fixture.sig()).unwrap();
    let error =
        signing::verify_index(&fixture.index(), None, &fixture.registry_toml()).unwrap_err();
    assert!(matches!(error, Error::Signature { .. }), "{error:?}");
    assert_refused(&fixture.verify());
}

#[test]
fn missing_or_malformed_sig_is_an_error_not_a_panic() {
    let (fixture, private) = Fixture::new();
    let error =
        signing::verify_index(&fixture.index(), None, &fixture.registry_toml()).unwrap_err();
    assert!(matches!(error, Error::Io { .. }), "{error:?}");
    assert!(error.to_string().contains("index.json.sig"), "{error}");
    assert_refused(&fixture.verify());

    signing::sign_index(&fixture.index(), &fixture.registry_toml(), &private).unwrap();
    let good = fs::read(fixture.sig()).unwrap();
    let short = {
        // The base64 of 63 bytes: valid base64, the wrong length.
        let text = String::from_utf8(good.clone()).unwrap();
        let mut decoded_len_63 = text.trim_end().to_owned();
        decoded_len_63.replace_range(84.., "");
        decoded_len_63
    };
    for bad in [
        Vec::new(),
        b"\n".to_vec(),
        b"not base64!\n".to_vec(),
        short.into_bytes(),
        [good.as_slice(), b"AAAA"].concat(),
        vec![0xff, 0xfe, 0x00, 0x80],
    ] {
        fs::write(fixture.sig(), &bad).unwrap();
        let error =
            signing::verify_index(&fixture.index(), None, &fixture.registry_toml()).unwrap_err();
        assert!(
            matches!(error, Error::Signature { .. }),
            "{bad:?}: {error:?}"
        );
        assert_refused(&fixture.verify());
    }

    // A missing index or registry.toml is an error too.
    fs::write(fixture.sig(), &good).unwrap();
    fs::remove_file(fixture.registry_toml()).unwrap();
    let error =
        signing::verify_index(&fixture.index(), None, &fixture.registry_toml()).unwrap_err();
    assert!(matches!(error, Error::Registry(_)), "{error:?}");
    assert_refused(&fixture.verify());
    fs::remove_file(fixture.index()).unwrap();
    assert_refused(&fixture.verify());
}

#[test]
fn private_key_is_read_from_stdin_only() {
    let (fixture, private) = Fixture::new();
    let base = [
        "sign",
        "--index",
        "index.json",
        "--registry-toml",
        "registry.toml",
    ];
    // A key on the command line, however it's given, is refused without
    // being echoed, and nothing is written, even with the key on stdin too.
    let given: [&[&str]; 4] = [
        &["--key", &private],
        &["--private-key", &private],
        &[&private],
        &[&format!("--key={private}")],
    ];
    for extra in given {
        let args: Vec<&str> = base.iter().copied().chain(extra.iter().copied()).collect();
        let output = fixture.run(&args, &private, &[]);
        assert_refused(&output);
        let stderr = stderr(&output);
        assert!(stderr.contains("stdin only"), "{stderr}");
        assert!(!stderr.contains(&private), "{stderr}");
        assert!(output.stdout.is_empty());
        assert_eq!(
            fixture.files(),
            ["index.json", "registry.toml"],
            "{extra:?}"
        );
    }

    // Nor is it taken from the environment: with nothing on stdin, there's
    // no key.
    let output = fixture.run(
        &base,
        "",
        &[("TENMOKU_REGISTRY_PRIVATE_KEY", private.as_str())],
    );
    assert_refused(&output);
    assert!(
        stderr(&output).contains("no private key on stdin"),
        "{}",
        stderr(&output)
    );
    assert_eq!(fixture.files(), ["index.json", "registry.toml"]);

    // On stdin, it signs.
    let output = fixture.run(&base, &private, &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        fixture.files(),
        ["index.json", "index.json.sig", "registry.toml"]
    );
}
