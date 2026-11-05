//! The first-party WebAssembly grammars (`extensions/grammars/<language>/`)
//! highlight as the built-in languages do.
//!
//! The ignored tests, one per language and named after it, load the module
//! `scripts/build-grammar-wasm.sh` builds, through the loader user grammars
//! take, and compare its highlights of the golden sample with the sample's
//! `.golden` file, which `src/tests/golden.rs` holds the built-in language to.
//! The script runs them; the gate has no wasi-sdk and the modules aren't
//! committed.
//!
//! A language whose injections name another wasm-only language (php's heredoc
//! tag `<<<SQL` asks for "sql") must register that one too, under its real
//! name, or the injection stays unresolved and the golden file differs; see
//! `load_injected`. A built-in language is found by name without that.
//!
//! The others need no module: they keep what the extensions hold from drifting
//! from what the built-in languages use, so a grammar bump in `Cargo.lock` or a
//! query change fails here too, on every run.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_buffer::Buffer;
use fe_syntax::{GrammarSpec, Language, Syntax, register_language};

const SCRIPT: &str = "scripts/build-grammar-wasm.sh";

fn repository() -> PathBuf {
    // Read at run time: a path compiled in can name a deleted snapshot.
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    Path::new(&manifest).join("../..")
}

fn extension(language: &str) -> PathBuf {
    repository().join("extensions/grammars").join(language)
}

fn read(file: &Path) -> String {
    fs::read_to_string(file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// The string `key = "..."` in a manifest of ours, which has one per line.
fn field(file: &Path, key: &str) -> String {
    read(file)
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == key).then(|| value.trim().trim_matches('"').to_owned())
        })
        .unwrap_or_else(|| panic!("{} has no {key}", file.display()))
}

/// `language`'s module in `modules`, with its extension's queries and
/// precedence, under a name of its own: a built-in language refuses its name
/// and file extensions to a registered one.
fn load(modules: &Path, language: &str) -> &'static Language {
    load_as(modules, language, &format!("{language}_wasm"))
}

/// `language`'s module registered as `name`.
fn load_as(modules: &Path, language: &str, name: &str) -> &'static Language {
    let dir = extension(language);
    let manifest = dir.join("extension.toml");
    let wasm = modules.join(field(&manifest, "wasm"));
    assert!(
        wasm.is_file(),
        "{} doesn't exist: build it with {SCRIPT} {language}",
        wasm.display()
    );
    register_language(&GrammarSpec {
        language: name.to_owned(),
        wasm,
        highlights: Some(dir.join(field(&manifest, "highlights"))),
        injections: Some(dir.join(field(&manifest, "injections"))),
        earlier_patterns_win: field(&manifest, "earlier_patterns_win") == "true",
        ..GrammarSpec::default()
    })
    .unwrap_or_else(|error| panic!("{language}: {error}"))
}

/// `language`'s module under its real name, for a language injected by name
/// into another (the injection asks for "sql", not "sql_wasm"). Nothing is
/// registered if a built-in language has the name.
fn load_injected(modules: &Path, language: &str) {
    if fe_syntax::language_by_name(language).is_none() {
        load_as(modules, language, language);
    }
}

/// One line per highlight span, as `src/tests/golden.rs` writes them.
fn render(language: &'static Language, text: &str) -> String {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());
    let tree = syntax.tree().expect("the sample parses");
    assert!(
        !tree.root_node().has_error(),
        "the sample parses without errors"
    );
    let spans = loop {
        let spans = syntax.highlights(0..text.len());
        let missing = syntax.take_missing_languages();
        if missing.is_empty() {
            break spans;
        }
        for name in missing {
            fe_syntax::language_by_name(name);
        }
    };
    let point = |offset| {
        let point = buffer.text().offset_to_point(offset);
        (point.row + 1, point.column + 1)
    };
    let mut out = String::new();
    for span in spans {
        let (row, column) = point(span.range.start);
        let (end_row, end_column) = point(span.range.end);
        let end = if end_row == row {
            end_column.to_string()
        } else {
            format!("{end_row}:{end_column}")
        };
        let name = span.highlight.name();
        let text = &text[span.range];
        out.push_str(&format!("{row}:{column}-{end} {name} {text:?}\n"));
    }
    out
}

/// `language`'s module highlights `sample` as its `.golden` file says.
fn assert_parity(language: &str, sample: &str) {
    let modules = repository().join("target/grammar-wasm");
    if language == "php" {
        // Its heredoc `<<<SQL` injects sql.
        load_injected(&modules, "sql");
    }
    let loaded = load(&modules, language);
    let golden_dir = repository().join("crates/fe-syntax/tests/golden");
    let actual = render(loaded, &read(&golden_dir.join(sample)));
    let expected = read(&golden_dir.join(format!("{sample}.golden")));
    if actual != expected {
        let line = expected
            .lines()
            .zip(actual.lines())
            .position(|(expected, actual)| expected != actual)
            .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()));
        panic!(
            "{language}'s wasm differs from {sample}.golden at line {} ({} lines, wasm {}):\n\
             - {}\n+ {}",
            line + 1,
            expected.lines().count(),
            actual.lines().count(),
            expected.lines().nth(line).unwrap_or("(end)"),
            actual.lines().nth(line).unwrap_or("(end)"),
        );
    }
}

#[test]
#[ignore = "needs scripts/build-grammar-wasm.sh"]
fn kotlin() {
    assert_parity("kotlin", "kotlin.kt");
}

#[test]
#[ignore = "needs scripts/build-grammar-wasm.sh"]
fn csharp() {
    assert_parity("csharp", "csharp.cs");
}

#[test]
#[ignore = "needs scripts/build-grammar-wasm.sh"]
fn scala() {
    assert_parity("scala", "scala.scala");
}

#[test]
#[ignore = "needs scripts/build-grammar-wasm.sh"]
fn swift() {
    assert_parity("swift", "swift.swift");
}

#[test]
#[ignore = "needs scripts/build-grammar-wasm.sh"]
fn cpp() {
    assert_parity("cpp", "cpp.cpp");
}

#[test]
#[ignore = "needs scripts/build-grammar-wasm.sh"]
fn sql() {
    assert_parity("sql", "sql.sql");
}

#[test]
#[ignore = "needs scripts/build-grammar-wasm.sh"]
fn php() {
    assert_parity("php", "php.php");
}

#[test]
#[ignore = "needs scripts/build-grammar-wasm.sh"]
fn ruby() {
    assert_parity("ruby", "ruby.rb");
}

#[test]
#[should_panic(expected = "scripts/build-grammar-wasm.sh kotlin")]
fn a_missing_module_fails_naming_the_script() {
    load(
        &repository().join("target/grammar-wasm/no-such-dir"),
        "kotlin",
    );
}

/// What every extension holds, and for each language anything more. No
/// module: those are built, and published as release assets.
const LANGUAGES: &[(&str, &[&str])] = &[
    // The Apache-2.0 text for highlights.scm's nvim-treesitter patterns.
    ("kotlin", &["LICENSE-APACHE"]),
    ("csharp", &[]),
    ("scala", &[]),
    ("swift", &[]),
    ("cpp", &[]),
    ("sql", &[]),
    ("php", &[]),
    ("ruby", &[]),
];

#[test]
fn every_extension_is_listed() {
    let mut found: Vec<String> = fs::read_dir(repository().join("extensions/grammars"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        // The release manifest beside the languages: grammar_pack_manifest's.
        .filter(|name| name != "pack.toml")
        .collect();
    found.sort();
    let mut listed: Vec<&str> = LANGUAGES.iter().map(|(language, _)| *language).collect();
    listed.sort();
    assert_eq!(found, listed);
}

#[test]
fn each_extension_holds_its_files_and_no_module() {
    for (language, more) in LANGUAGES {
        let mut files: Vec<String> = fs::read_dir(extension(language))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        let mut expected: Vec<String> = [
            "LICENSE",
            "extension.toml",
            "grammar-source.toml",
            "highlights.scm",
            "injections.scm",
        ]
        .iter()
        .chain(more.iter())
        .map(|&file| file.to_owned())
        .collect();
        expected.sort();
        assert_eq!(files, expected, "extensions/grammars/{language}");
    }
}

#[test]
fn each_pin_is_cargo_locks() {
    let lock = read(&repository().join("Cargo.lock"));
    for (language, _) in LANGUAGES {
        let source = extension(language).join("grammar-source.toml");
        let (krate, version) = (field(&source, "crate"), field(&source, "version"));
        let entry = format!("name = \"{krate}\"\nversion = \"{version}\"\n");
        assert!(
            lock.contains(&entry),
            "{language}: Cargo.lock doesn't pin {krate} {version}; update {} and rebuild \
             with {SCRIPT}",
            source.display()
        );
    }
}

#[test]
fn each_wasm_file_is_named_after_its_export() {
    for (language, _) in LANGUAGES {
        let dir = extension(language);
        let grammar = field(&dir.join("grammar-source.toml"), "grammar");
        assert_eq!(
            field(&dir.join("extension.toml"), "wasm"),
            format!("tree-sitter-{}.wasm", grammar.replace('_', "-")),
            "{language}: the loader finds tree_sitter_{grammar} by the file's name"
        );
    }
}

/// A query file with nothing but comments and blank lines.
fn has_no_patterns(query: &str) -> bool {
    query.lines().all(|line| {
        let line = line.trim();
        line.is_empty() || line.starts_with(';')
    })
}

/// The strings of the one-line list `key = [...]` in a manifest, if it has it.
#[allow(dead_code)] // With every grammar's feature off.
fn list<'a>(manifest: &'a str, key: &str) -> Vec<&'a str> {
    let prefix = format!("{key} = ");
    manifest
        .lines()
        .find_map(|line| line.strip_prefix(prefix.as_str()))
        .map_or(Vec::new(), |list| {
            list.trim_matches(['[', ']'])
                .split(", ")
                .map(|item| item.trim_matches('"'))
                .collect()
        })
}

/// The extension is for the files the built-in language is, and its queries
/// are the built-in's, the parts concatenated as `language.rs` does.
/// `injections` is `None` where the built-in has no such query.
#[allow(dead_code)] // With every grammar's feature off.
fn assert_as_built_in(language: &str, highlights: &[&str], injections: Option<&str>) {
    let dir = extension(language);
    let manifest = read(&dir.join("extension.toml"));
    let extensions = list(&manifest, "extensions");
    assert!(
        !extensions.is_empty(),
        "extension.toml lists its extensions"
    );
    let paths = extensions
        .iter()
        .map(|extension| format!("file.{extension}"))
        .chain(list(&manifest, "file_names").into_iter().map(str::to_owned));
    for path in paths {
        assert_eq!(
            fe_syntax::language_name_for_path(Path::new(&path)),
            Some(language),
            "{path}"
        );
    }
    assert!(
        read(&dir.join("highlights.scm")) == highlights.concat(),
        "extensions/grammars/{language}/highlights.scm isn't the built-in's query"
    );
    let file = read(&dir.join("injections.scm"));
    match injections {
        Some(query) => assert!(
            file == query,
            "extensions/grammars/{language}/injections.scm isn't the built-in's query"
        ),
        None => assert!(
            has_no_patterns(&file),
            "extensions/grammars/{language}/injections.scm has patterns; the built-in has none"
        ),
    }
}

#[cfg(feature = "kotlin")]
#[test]
fn kotlin_extension_is_as_built_in() {
    assert_as_built_in(
        "kotlin",
        &[
            tree_sitter_kotlin_sg::HIGHLIGHTS_QUERY,
            include_str!("../queries/kotlin.scm"),
        ],
        None,
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_extension_is_as_built_in() {
    assert_as_built_in("csharp", &[tree_sitter_c_sharp::HIGHLIGHTS_QUERY], None);
}

#[cfg(feature = "scala")]
#[test]
fn scala_extension_is_as_built_in() {
    assert_as_built_in("scala", &[tree_sitter_scala::HIGHLIGHTS_QUERY], None);
}

#[cfg(feature = "swift")]
#[test]
fn swift_extension_is_as_built_in() {
    assert_as_built_in("swift", &[tree_sitter_swift::HIGHLIGHTS_QUERY], None);
}

#[cfg(feature = "cpp")]
#[test]
fn cpp_extension_is_as_built_in() {
    assert_as_built_in(
        "cpp",
        &[
            tree_sitter_c::HIGHLIGHT_QUERY,
            tree_sitter_cpp::HIGHLIGHT_QUERY,
        ],
        None,
    );
}

#[cfg(feature = "sql")]
#[test]
fn sql_extension_is_as_built_in() {
    assert_as_built_in(
        "sql",
        &[
            tree_sitter_sequel::HIGHLIGHTS_QUERY,
            include_str!("../queries/sql.scm"),
        ],
        None,
    );
}

#[cfg(feature = "php")]
#[test]
fn php_extension_is_as_built_in() {
    assert_as_built_in(
        "php",
        &[tree_sitter_php::HIGHLIGHTS_QUERY],
        Some(include_str!("../queries/php-injections.scm")),
    );
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_extension_is_as_built_in() {
    assert_as_built_in(
        "ruby",
        &[
            tree_sitter_ruby::HIGHLIGHTS_QUERY,
            include_str!("../queries/ruby.scm"),
        ],
        None,
    );
}

/// `release.yml` and `grammars.yml` publish to one release from the same tag.
/// These run both workflows' real publishing scripts against a fake `gh`, whose
/// `release create` fails if the release exists, as GitHub's does.
#[cfg(unix)]
mod publishing {
    use std::{os::unix::fs::PermissionsExt, process::Command};

    use super::*;

    const APP: &str = "Publish the GitHub Release";
    const GRAMMARS: &str = "Attach them to the GitHub Release";

    const FAKE_GH: &str = r#"#!/bin/sh
echo "$PUBLISHER $2" >>log
case $2 in
view)
  [ -e release ] && exit 0
  # WINDOW: the grammars publisher runs after the app's view, before its create.
  if [ "$PUBLISHER" = app ] && [ -n "$WINDOW" ]; then
    PUBLISHER=grammars WINDOW= AT_SLEEP=stop bash -e grammars.sh
  fi
  exit 1
  ;;
create) [ ! -e release ] && touch release ;;
upload) [ -e release ] ;;
esac
"#;

    /// What the grammars publisher's wait lets happen: the app publisher runs,
    /// or the wait is cut short.
    const FAKE_SLEEP: &str = r#"#!/bin/sh
echo "$PUBLISHER sleep" >>log
case $AT_SLEEP in
app) PUBLISHER=app bash -e app.sh ;;
stop) exit 1 ;;
esac
"#;

    /// The `run: |` script of the step `name` in a workflow.
    fn step_script(workflow: &str, name: &str) -> String {
        let text = read(&repository().join(".github/workflows").join(workflow));
        let indent = |line: &str| line.len() - line.trim_start().len();
        let mut lines = text
            .lines()
            .skip_while(|line| line.trim() != format!("- name: {name}"))
            .skip_while(|line| line.trim() != "run: |");
        let run = lines
            .next()
            .unwrap_or_else(|| panic!("{workflow} has no step {name} with a run: | script"));
        lines
            .take_while(|line| line.trim().is_empty() || indent(line) > indent(run))
            .map(|line| line.get(indent(run) + 2..).unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A directory holding both publishers' scripts and the fake commands.
    struct Stage(PathBuf);

    impl Stage {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("publishing-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("bin")).unwrap();
            for (command, script) in [("gh", FAKE_GH), ("sleep", FAKE_SLEEP)] {
                let file = dir.join("bin").join(command);
                fs::write(&file, script).unwrap();
                fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
            }
            fs::write(dir.join("app.sh"), step_script("release.yml", APP)).unwrap();
            fs::write(
                dir.join("grammars.sh"),
                step_script("grammars.yml", GRAMMARS),
            )
            .unwrap();
            fs::write(dir.join("log"), "").unwrap();
            Self(dir)
        }

        /// Runs a publisher as Actions does, with `env` added; true if it succeeds.
        fn publish(&self, who: &str, env: &[(&str, &str)]) -> bool {
            let path = format!(
                "{}:{}",
                self.0.join("bin").display(),
                std::env::var("PATH").unwrap()
            );
            Command::new("bash")
                .args(["-e", &format!("{who}.sh")])
                .current_dir(&self.0)
                .env("PATH", path)
                .env("PUBLISHER", who)
                .env("GITHUB_REF_NAME", "v1.0.0")
                .env("GITHUB_EVENT_NAME", "push")
                .envs(env.iter().copied())
                .status()
                .unwrap()
                .success()
        }

        fn log(&self) -> String {
            read(&self.0.join("log"))
        }

        fn count(&self, entry: &str) -> usize {
            self.log().lines().filter(|line| *line == entry).count()
        }

        /// The app created the release, the only creator, and the grammars
        /// were uploaded to it.
        fn assert_both_published(&self) {
            let log = self.log();
            assert_eq!(self.count("app create"), 1, "{log}");
            assert_eq!(self.count("grammars create"), 0, "{log}");
            assert_eq!(self.count("grammars upload"), 1, "{log}");
        }
    }

    impl Drop for Stage {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_app_publishes_first() {
        let stage = Stage::new("app-first");
        assert!(stage.publish("app", &[]));
        assert!(stage.publish("grammars", &[]));
        stage.assert_both_published();
    }

    #[test]
    fn the_grammars_wait_for_the_app_to_publish() {
        let stage = Stage::new("grammars-first");
        assert!(stage.publish("grammars", &[("AT_SLEEP", "app")]));
        stage.assert_both_published();
    }

    #[test]
    fn the_grammars_run_between_the_apps_view_and_create() {
        let stage = Stage::new("window");
        assert!(stage.publish("app", &[("WINDOW", "1")]));
        assert_eq!(stage.count("grammars view"), 1, "{}", stage.log());
        assert!(stage.publish("grammars", &[]));
        stage.assert_both_published();
    }

    #[test]
    fn a_release_that_never_comes_fails_the_grammars_without_creating_it() {
        let stage = Stage::new("never");
        assert!(!stage.publish("grammars", &[]));
        let log = stage.log();
        assert!(
            stage.count("grammars sleep") > 100,
            "waits on a tag push: {log}"
        );
        assert_eq!(stage.count("grammars create"), 0, "{log}");
        assert_eq!(stage.count("grammars upload"), 0, "{log}");
    }

    #[test]
    fn a_manual_run_does_not_wait_for_a_release() {
        // release.yml's manual run publishes nothing, so nothing is coming.
        let stage = Stage::new("manual");
        assert!(!stage.publish("grammars", &[("GITHUB_EVENT_NAME", "workflow_dispatch")]));
        assert_eq!(stage.log(), "grammars view\n");
    }
}
