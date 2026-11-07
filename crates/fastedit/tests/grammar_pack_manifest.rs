//! `extensions/grammars/pack.toml`, read as the file it is, against each
//! language's `extension.toml`; and `scripts/record-grammar-hashes.sh`, which
//! rewrites it, with a fake `gh`.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const LANGUAGES: [&str; 8] = [
    "kotlin", "csharp", "scala", "swift", "cpp", "sql", "php", "ruby",
];

fn root() -> PathBuf {
    // Read at run time: a path compiled in can name a deleted snapshot.
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    Path::new(&manifest).join("../..")
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

#[test]
fn pack_lists_all_eight() {
    let pack = table(&root().join("extensions/grammars/pack.toml"));

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

    let languages = pack["languages"].as_table().unwrap();
    assert_eq!(languages.len(), LANGUAGES.len(), "{languages:?}");
    for language in LANGUAGES {
        let entry = languages[language].as_table().unwrap();
        assert_eq!(
            entry["file"].as_str(),
            Some(extension_wasm(language).as_str()),
            "{language}'s file"
        );
        let size = entry["size"].as_integer().unwrap();
        let sha256 = entry["sha256"].as_str().unwrap();
        assert!(
            sha256.is_empty() || is_sha256(sha256),
            "{language}'s sha256 {sha256:?}"
        );
        // Until it's released neither is known; after, both are, and the
        // loader must be able to take it.
        if sha256.is_empty() {
            assert_eq!(size, 0, "{language} has a size but no hash");
        } else {
            assert!(
                size > 0 && size as u64 <= fe_syntax::MAX_GRAMMAR_BYTES,
                "{language}'s size {size}"
            );
        }
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
        for language in LANGUAGES {
            let file = extension_wasm(language);
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
    let languages = pack["languages"].as_table().unwrap();
    assert_eq!(languages.len(), LANGUAGES.len());
    for language in LANGUAGES {
        let file = extension_wasm(language);
        let entry = languages[language].as_table().unwrap();
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

    // The comments stay, and only the sizes and hashes moved.
    let after = fs::read_to_string(fixture.pack()).unwrap();
    assert_eq!(before.lines().count(), after.lines().count());
    for (before, after) in before.lines().zip(after.lines()) {
        if !before.starts_with("size = ") && !before.starts_with("sha256 = ") {
            assert_eq!(before, after);
        }
    }

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
    let missing = extension_wasm("ruby");
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
