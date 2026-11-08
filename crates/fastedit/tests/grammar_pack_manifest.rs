//! `extensions/grammars/pack.toml`, read as the file it is, against each
//! language's `extension.toml`; and `scripts/record-grammar-hashes.sh`, which
//! rewrites it, with a fake `gh`.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn root() -> PathBuf {
    // Read at run time: a path compiled in can name a deleted snapshot.
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    Path::new(&manifest).join("../..")
}

/// The first-party languages: the directories under `extensions/grammars/`.
fn languages() -> Vec<String> {
    let mut languages: Vec<String> = fs::read_dir(root().join("extensions/grammars"))
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_dir())
        .map(|entry| entry.file_name().into_string().unwrap())
        .collect();
    languages.sort();
    assert!(!languages.is_empty());
    languages
}

fn table(path: &Path) -> toml::Table {
    let text = fs::read_to_string(path).unwrap_or_else(|error| panic!("{path:?}: {error}"));
    toml::from_str(&text).unwrap_or_else(|error| panic!("{path:?}: {error}"))
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn extension_wasm(language: &str) -> String {
    let path = root().join(format!("extensions/grammars/{language}/extension.toml"));
    let manifest = table(&path);
    let grammars = manifest["contributes"]["grammars"].as_array().unwrap();
    assert_eq!(grammars.len(), 1, "{language}");
    grammars[0]["wasm"].as_str().unwrap().to_owned()
}

/// pack.toml is a release's URL, and the entries recorded from it: each for a
/// directory, naming its extension's wasm, with a size the loader takes and
/// its hash.
#[test]
fn pack_holds_its_release_and_recorded_entries() {
    let pack = table(&root().join("extensions/grammars/pack.toml"));
    let keys: Vec<&str> = pack.keys().map(String::as_str).collect();
    assert!(
        keys == ["base_url"] || keys == ["base_url", "languages"],
        "{keys:?}"
    );

    let base_url = pack["base_url"].as_str().unwrap();
    let release = base_url
        .strip_prefix("https://github.com/")
        .unwrap_or_else(|| panic!("{base_url} isn't an https github.com URL"));
    let parts: Vec<&str> = release.split('/').collect();
    assert!(
        matches!(parts[..], [owner, repo, "releases", "download", tag]
            if !owner.is_empty() && !repo.is_empty() && !tag.is_empty()),
        "{base_url} isn't a release's URL"
    );

    let languages = languages();
    let empty = toml::Table::new();
    let entries = pack
        .get("languages")
        .map_or(&empty, |entries| entries.as_table().unwrap());
    for (language, entry) in entries {
        assert!(
            languages.contains(language),
            "pack.toml records {language}, which has no directory"
        );
        let entry = entry.as_table().unwrap();
        assert_eq!(
            entry["file"].as_str(),
            Some(extension_wasm(language).as_str()),
            "{language}'s file"
        );
        let size = entry["size"].as_integer().unwrap();
        let sha256 = entry["sha256"].as_str().unwrap();
        // What's recorded is released; what isn't has no entry.
        assert!(is_sha256(sha256), "{language}'s sha256 {sha256:?}");
        assert!(
            size > 0 && size as u64 <= fe_syntax::MAX_GRAMMAR_BYTES,
            "{language}'s size {size}"
        );
    }
}

/// A copy of the real manifest, and a `gh` that "downloads" the named release
/// by copying the files in `release/`, logging its arguments.
struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        fs::copy(
            root().join("extensions/grammars/pack.toml"),
            dir.path().join("pack.toml"),
        )
        .unwrap();
        fs::create_dir(dir.path().join("release")).unwrap();
        fs::create_dir(dir.path().join("bin")).unwrap();
        let gh = dir.path().join("bin/gh");
        fs::write(
            &gh,
            format!(
                "#!/bin/sh\necho \"$@\" >>'{0}/gh.log'\n\
                 while [ $# -gt 0 ]; do\n  \
                   [ \"$1\" = --dir ] && cp '{0}'/release/* \"$2\"\n  shift\n\
                 done\n",
                dir.path().display()
            ),
        )
        .unwrap();
        make_executable(&gh);
        Fixture { dir }
    }

    fn release(&self, file: &str, contents: &[u8]) {
        fs::write(self.dir.path().join("release").join(file), contents).unwrap();
    }

    /// Every language's file, `<file>` as its contents.
    fn release_all(&self) {
        for language in languages() {
            let file = extension_wasm(&language);
            self.release(&file, file.as_bytes());
        }
    }

    fn pack(&self) -> PathBuf {
        self.dir.path().join("pack.toml")
    }

    fn run(&self, args: &[&str]) -> Output {
        let path = std::env::var_os("PATH").unwrap();
        let mut paths = vec![self.dir.path().join("bin")];
        paths.extend(std::env::split_paths(&path));
        Command::new("bash")
            .arg(root().join("scripts/record-grammar-hashes.sh"))
            .args(args)
            .env("PACK_TOML", self.pack())
            .env("PATH", std::env::join_paths(paths).unwrap())
            .output()
            .unwrap()
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(not(unix))]
fn make_executable(_: &Path) {}

#[test]
#[cfg_attr(windows, ignore = "runs a bash release script, which runs on macOS")]
fn the_script_records_each_files_size_and_hash() {
    let fixture = Fixture::new();
    fixture.release_all();
    let before = fs::read_to_string(fixture.pack()).unwrap();

    let output = fixture.run(&[]);
    assert!(output.status.success(), "{output:?}");

    let pack = table(&fixture.pack());
    let entries = pack["languages"].as_table().unwrap();
    let languages = languages();
    assert_eq!(
        entries.keys().collect::<Vec<_>>(),
        languages.iter().collect::<Vec<_>>(),
        "an entry for every directory"
    );
    for language in &languages {
        let file = extension_wasm(language);
        let entry = entries[language].as_table().unwrap();
        assert_eq!(entry["file"].as_str(), Some(file.as_str()));
        assert_eq!(entry["size"].as_integer(), Some(file.len() as i64));
        assert_eq!(
            entry["sha256"].as_str(),
            Some(fe_net::sha256_hex(file.as_bytes()).as_str()),
            "{language}: what sha256sum writes is what fe-net compares"
        );
    }
    assert_eq!(
        pack["base_url"].as_str(),
        table(&root().join("extensions/grammars/pack.toml"))["base_url"].as_str()
    );

    // What's before the entries stays, and the entries are written anew: so
    // recording again, as after another release, gives the same file.
    let after = fs::read_to_string(fixture.pack()).unwrap();
    let head = |text: &str| -> String {
        let end = text.find("\n[languages.").unwrap_or(text.len());
        text[..end].trim_end().to_owned()
    };
    assert_eq!(head(&after), head(&before));
    let output = fixture.run(&[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(fixture.pack()).unwrap(), after);

    let log = fs::read_to_string(fixture.dir.path().join("gh.log")).unwrap();
    let base_url = table(&fixture.pack())["base_url"]
        .as_str()
        .unwrap()
        .to_owned();
    let tag = base_url.rsplit('/').next().unwrap();
    assert!(log.contains(&format!("release download {tag} ")), "{log}");
    let repo = base_url
        .strip_prefix("https://github.com/")
        .and_then(|rest| rest.split("/releases/").next())
        .unwrap();
    assert!(log.contains(&format!("--repo {repo}")), "{log}");
}

#[test]
#[cfg_attr(windows, ignore = "runs a bash release script, which runs on macOS")]
fn the_script_can_move_to_another_release() {
    let fixture = Fixture::new();
    fixture.release_all();
    let output = fixture.run(&["grammars-99"]);
    assert!(output.status.success(), "{output:?}");
    let base_url = table(&fixture.pack())["base_url"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        base_url.ends_with("/releases/download/grammars-99"),
        "{base_url}"
    );
}

#[test]
#[cfg_attr(windows, ignore = "runs a bash release script, which runs on macOS")]
fn the_script_leaves_the_manifest_alone_when_a_file_is_missing() {
    let fixture = Fixture::new();
    fixture.release_all();
    let missing = extension_wasm(languages().last().unwrap());
    fs::remove_file(fixture.dir.path().join("release").join(&missing)).unwrap();
    let before = fs::read_to_string(fixture.pack()).unwrap();

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&missing), "{stderr}");
    assert_eq!(fs::read_to_string(fixture.pack()).unwrap(), before);
}

#[test]
#[cfg_attr(windows, ignore = "runs a bash release script, which runs on macOS")]
fn the_script_says_what_it_needs() {
    let fixture = Fixture::new();
    let empty = tempfile::tempdir().unwrap();
    let bash = which_bash();
    let output = Command::new(bash)
        .arg(root().join("scripts/record-grammar-hashes.sh"))
        .env("PACK_TOML", fixture.pack())
        .env("PATH", empty.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("needs gh"), "{stderr}");
}

fn which_bash() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap();
    std::env::split_paths(&path)
        .map(|dir| dir.join("bash"))
        .find(|bash| bash.is_file())
        .expect("bash on PATH")
}
