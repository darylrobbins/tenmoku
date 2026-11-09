//! `scripts/package-language-pack.sh`, run on stub wasm files: no wasi-sdk, no
//! real grammar.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const ARCHIVE: &str = "Tenmoku-language-pack.tar.gz";

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

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success() || !output.stdout.is_empty())
}

fn sha_tool() -> Option<&'static [&'static str]> {
    if have("sha256sum") {
        Some(&["sha256sum"])
    } else if have("shasum") {
        Some(&["shasum", "-a", "256"])
    } else {
        None
    }
}

/// True if the machine can't package the pack and the run opted into skipping
/// that with `TENMOKU_SKIP_LANGUAGE_PACK=1`, with the reason on stderr. Without
/// the opt-in a missing tool fails the test: a plain return would report a pass
/// for a test that checked nothing, and nextest can't mark a skip at run time.
fn skip() -> bool {
    let missing = if !have("tar") {
        "tar"
    } else if sha_tool().is_none() {
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

fn wasm_name(language: &str) -> String {
    let path = root().join(format!("extensions/grammars/{language}/extension.toml"));
    let text = fs::read_to_string(&path).unwrap();
    let manifest: toml::Table = toml::from_str(&text).unwrap();
    manifest["contributes"]["grammars"][0]["wasm"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// The files besides the wasm that the pack has of `language`, as its
/// directory has them: extension.toml, the queries, LICENSE, any LICENSE-*
/// and any NOTICE.
fn text_files(language: &str) -> Vec<String> {
    let dir = root().join(format!("extensions/grammars/{language}"));
    let mut files: Vec<String> = [
        "extension.toml",
        "highlights.scm",
        "injections.scm",
        "LICENSE",
    ]
    .map(str::to_owned)
    .into();
    for entry in fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if name.starts_with("LICENSE-") || name == "NOTICE" {
            files.push(name);
        }
    }
    files
}

/// A fake module, keyed by its file name: languages sharing a grammar (ERB and
/// EJS) share one module, so a stub per language would overwrite the other.
fn stub(wasm: &str) -> Vec<u8> {
    format!("\0asm stub for {wasm}").into_bytes()
}

struct Run {
    _dir: tempfile::TempDir,
    wasm: PathBuf,
    dist: PathBuf,
    output: Output,
}

/// Runs the script on stubs for every language but `without`.
fn package(without: Option<&str>) -> Run {
    let dir = tempfile::tempdir().unwrap();
    let wasm = dir.path().join("wasm");
    let dist = dir.path().join("dist");
    fs::create_dir(&wasm).unwrap();
    for language in languages() {
        if Some(language.as_str()) != without {
            let name = wasm_name(&language);
            fs::write(wasm.join(&name), stub(&name)).unwrap();
        }
    }
    let output = Command::new(root().join("scripts/package-language-pack.sh"))
        .env("GRAMMAR_WASM_DIR", &wasm)
        .env("DIST_DIR", &dist)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    Run {
        _dir: dir,
        wasm,
        dist,
        output,
    }
}

fn succeeded(run: &Run) {
    assert!(
        run.output.status.success(),
        "{}",
        String::from_utf8_lossy(&run.output.stderr)
    );
}

fn unpack(run: &Run) -> tempfile::TempDir {
    let into = tempfile::tempdir().unwrap();
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(run.dist.join(ARCHIVE))
        .arg("-C")
        .arg(into.path())
        .status()
        .unwrap();
    assert!(status.success());
    into
}

#[test]
#[cfg_attr(windows, ignore = "runs a bash release script, which runs on macOS")]
fn pack_has_every_languages_loadable_extension() {
    if skip() {
        return;
    }
    let run = package(None);
    succeeded(&run);
    let listing = Command::new("tar")
        .arg("-tzf")
        .arg(run.dist.join(ARCHIVE))
        .output()
        .unwrap();
    assert!(listing.status.success());
    let listing = String::from_utf8(listing.stdout).unwrap();
    let files: BTreeSet<&str> = listing
        .lines()
        .map(|line| line.trim_start_matches("./"))
        .filter(|line| !line.ends_with('/'))
        .collect();
    let mut expected = BTreeSet::new();
    let languages = languages();
    let wasm_names: Vec<String> = languages.iter().map(|l| wasm_name(l)).collect();
    for (language, wasm) in languages.iter().zip(&wasm_names) {
        for file in text_files(language).iter().chain([wasm]) {
            expected.insert(format!("tenmoku.grammar_{language}/{file}"));
        }
    }
    let expected: BTreeSet<&str> = expected.iter().map(String::as_str).collect();
    assert_eq!(files, expected);
    // Directories, if the listing has them, are only the languages'.
    for line in listing.lines().filter(|line| line.ends_with('/')) {
        let name = line.trim_start_matches("./").trim_end_matches('/');
        assert!(
            languages
                .iter()
                .any(|l| name == format!("tenmoku.grammar_{l}")),
            "{line}"
        );
    }
}

#[test]
#[cfg_attr(windows, ignore = "runs a bash release script, which runs on macOS")]
fn pack_matches_the_installer_layout() {
    if skip() {
        return;
    }
    let run = package(None);
    succeeded(&run);
    let unpacked = unpack(&run);
    // grammar_install writes the wasm under the name the extension's manifest
    // gives, and the other three files as they are in extensions/grammars/;
    // the pack has the licence files too, as the registry's artifact does.
    for language in languages() {
        let dir = unpacked.path().join(format!("tenmoku.grammar_{language}"));
        let source = root().join(format!("extensions/grammars/{language}"));
        let text_files = text_files(&language);
        for file in &text_files {
            assert_eq!(
                fs::read(dir.join(file)).unwrap(),
                fs::read(source.join(file)).unwrap(),
                "{language}/{file}"
            );
        }
        let wasm = wasm_name(&language);
        assert_eq!(fs::read(dir.join(&wasm)).unwrap(), stub(&wasm), "{wasm}");
        let names: BTreeSet<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names.len(), text_files.len() + 1, "{names:?}");
    }
}

#[test]
#[cfg_attr(windows, ignore = "runs a bash release script, which runs on macOS")]
fn checksum_file_verifies() {
    if skip() {
        return;
    }
    let run = package(None);
    succeeded(&run);
    let sums = run.dist.join(format!("{ARCHIVE}.sha256"));
    assert!(sums.is_file());
    let tool = sha_tool().unwrap();
    let output = Command::new(tool[0])
        .args(&tool[1..])
        .arg("-c")
        .arg(format!("{ARCHIVE}.sha256"))
        .current_dir(&run.dist)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // And it fails on a changed archive.
    fs::write(run.dist.join(ARCHIVE), b"changed").unwrap();
    let output = Command::new(tool[0])
        .args(&tool[1..])
        .arg("-c")
        .arg(format!("{ARCHIVE}.sha256"))
        .current_dir(&run.dist)
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
#[cfg_attr(windows, ignore = "runs a bash release script, which runs on macOS")]
fn missing_wasm_fails_naming_the_script() {
    if skip() {
        return;
    }
    let missing = languages().pop().unwrap();
    let run = package(Some(&missing));
    assert!(!run.output.status.success());
    let stderr = String::from_utf8_lossy(&run.output.stderr);
    assert!(stderr.contains("scripts/build-grammar-wasm.sh"), "{stderr}");
    assert!(stderr.contains(&wasm_name(&missing)), "{stderr}");
    assert!(!run.dist.join(ARCHIVE).exists());
    assert!(!run.dist.join(format!("{ARCHIVE}.sha256")).exists());
    assert!(run.wasm.is_dir());
}
