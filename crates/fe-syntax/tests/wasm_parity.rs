//! The first-party WebAssembly grammars (`extensions/grammars/<language>/`):
//! each directory is well formed, and its module highlights its samples as
//! their golden files say.
//!
//! `every_extension_highlights_its_sample`, ignored, loads each module
//! `scripts/build-grammar-wasm.sh` builds, through the loader user grammars
//! take, and compares its highlights of each sample with the sample's
//! `.golden` file, in the format `src/tests/golden.rs` writes. A language
//! fe-syntax also has built in (behind a feature, as the first eight are) is
//! held to the built-in's own sample, `tests/golden/<language>.*`, so its wasm
//! highlights as the native grammar does. Any other holds its samples itself,
//! `test/sample.<extension>` and `test/sample.<extension>.golden`. The script
//! runs it, for the languages `GRAMMAR_LANGUAGES` names; the gate has no
//! wasi-sdk and the modules aren't committed.
//!
//! A language whose sample injects another wasm-only language (php's heredoc
//! tag `<<<SQL` asks for "sql") names it in its `grammar-source.toml`'s
//! `parity_needs`, and that one is registered too, under its real name: else
//! the injection stays unresolved and the golden file differs. A built-in
//! language is found by name without that.
//!
//! The others need no module. `every_extension_is_well_formed` checks what a
//! language card may get wrong ([`problems`]); the `*_extension_is_as_built_in`
//! ones keep what the extensions hold from drifting from what the built-in
//! languages use, so a grammar bump in `Cargo.lock` or a query change fails
//! here too, on every run.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use fe_buffer::Buffer;
use fe_syntax::{
    Clash, GrammarError, GrammarSpec, Highlight, Language, Syntax, register_language,
    unregister_language,
};

const SCRIPT: &str = "scripts/build-grammar-wasm.sh";

fn repository() -> PathBuf {
    // Read at run time: a path compiled in can name a deleted snapshot.
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("run by cargo");
    Path::new(&manifest).join("../..")
}

fn grammars() -> PathBuf {
    repository().join("extensions/grammars")
}

fn extension(language: &str) -> PathBuf {
    grammars().join(language)
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

/// The languages under `root`, one per directory, sorted.
fn languages_in(root: &Path) -> Vec<String> {
    let mut languages: Vec<String> = fs::read_dir(root)
        .unwrap_or_else(|error| panic!("{}: {error}", root.display()))
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    languages.sort();
    languages
}

/// Every first-party language.
fn languages() -> Vec<String> {
    languages_in(&grammars())
}

/// The sample fe-syntax holds its built-in `language` to, when it has one
/// (perhaps behind a feature): `tests/golden/<language>.<extension>`.
fn native_sample(language: &str) -> Option<PathBuf> {
    let dir = repository().join("crates/fe-syntax/tests/golden");
    let mut samples = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| !path.to_string_lossy().ends_with(".golden"));
    samples.find(|path| path.file_stem().is_some_and(|stem| stem == language))
}

/// A sample and the golden file of its highlights.
struct Sample {
    text: PathBuf,
    golden: PathBuf,
    /// The extension's own, under `test/`, which `UPDATE_GOLDEN=1` rewrites;
    /// a built-in's belong to `src/tests/golden.rs`.
    own: bool,
}

/// The samples under `dir/test/`, by name.
fn own_samples(dir: &Path) -> Vec<Sample> {
    let test = dir.join("test");
    let Ok(entries) = fs::read_dir(&test) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("sample.") && !name.ends_with(".golden"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| Sample {
            golden: test.join(format!("{name}.golden")),
            text: test.join(name),
            own: true,
        })
        .collect()
}

/// `language`'s samples: the built-in's, then its own.
fn samples(language: &str) -> Vec<Sample> {
    let native = native_sample(language).map(|text| Sample {
        golden: PathBuf::from(format!("{}.golden", text.display())),
        text,
        own: false,
    });
    native
        .into_iter()
        .chain(own_samples(&extension(language)))
        .collect()
}

/// A manifest of ours as `(table, key, value)`, one per line: one `key =
/// value` a line, each value on its line, under the `[table]` or `[[table]]`
/// line before. Comments and blank lines are skipped.
fn keys(text: &str) -> Result<Vec<(String, String, String)>, String> {
    let mut table = String::new();
    let mut keys = Vec::new();
    for (ix, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let header = (line
            .strip_prefix("[[")
            .and_then(|rest| rest.strip_suffix("]]")))
        .or_else(|| {
            line.strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
        });
        if let Some(name) = header {
            table = name.trim().to_owned();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("line {}, {line:?}, isn't key = value", ix + 1));
        };
        keys.push((
            table.clone(),
            key.trim().to_owned(),
            value.trim().to_owned(),
        ));
    }
    Ok(keys)
}

/// The string a value `"..."` holds, if it's one, with no escapes.
fn string(value: &str) -> Option<&str> {
    (value.strip_prefix('"')?.strip_suffix('"')).filter(|text| !text.contains(['"', '\\']))
}

/// The strings of a one-line list `["a", "b"]`, if it's one.
fn strings(value: &str) -> Option<Vec<&str>> {
    let items = value.strip_prefix('[')?.strip_suffix(']')?.trim();
    if items.is_empty() {
        return Some(Vec::new());
    }
    items.split(',').map(|item| string(item.trim())).collect()
}

/// The value of `key` in `table` of the manifest at `file`, if it has one.
fn value_in(file: &Path, table: &str, key: &str) -> Option<String> {
    let text = fs::read_to_string(file).ok()?;
    keys(&text)
        .ok()?
        .into_iter()
        .find(|(t, k, _)| t == table && k == key)
        .map(|(_, _, value)| value)
}

/// The string `key` of the extension's grammar.
fn grammar_string(dir: &Path, key: &str) -> Result<String, String> {
    let manifest = dir.join("extension.toml");
    value_in(&manifest, "contributes.grammars", key)
        .and_then(|value| string(&value).map(str::to_owned))
        .ok_or_else(|| format!("{}'s grammar has no {key}", manifest.display()))
}

/// The first-party languages `language`'s samples inject, transitively: its
/// `parity_needs`, theirs, and so on.
fn parity_needs(language: &str) -> Vec<String> {
    let mut needs = Vec::new();
    let mut pending = vec![language.to_owned()];
    while let Some(next) = pending.pop() {
        let source = extension(&next).join("grammar-source.toml");
        let listed = value_in(&source, "", "parity_needs").unwrap_or_default();
        for need in strings(&listed).unwrap_or_default() {
            if need != language && !needs.iter().any(|known| known == need) {
                needs.push(need.to_owned());
                pending.push(need.to_owned());
            }
        }
    }
    needs
}

/// `language`'s module in `modules`, with its extension's queries and
/// precedence, under a name of its own: a built-in language refuses its name
/// and file extensions to a registered one.
fn load(modules: &Path, language: &str) -> &'static Language {
    load_as(modules, language, &format!("{language}_wasm"))
        .unwrap_or_else(|error| panic!("{error}"))
}

/// `language`'s module registered as `name`.
fn load_as(modules: &Path, language: &str, name: &str) -> Result<&'static Language, String> {
    let dir = extension(language);
    let wasm = modules.join(grammar_string(&dir, "wasm")?);
    if !wasm.is_file() {
        return Err(format!(
            "{} doesn't exist: build it with {SCRIPT} {language}",
            wasm.display()
        ));
    }
    let earlier_patterns_win = value_in(
        &dir.join("extension.toml"),
        "contributes.grammars",
        "earlier_patterns_win",
    );
    register_language(&GrammarSpec {
        language: name.to_owned(),
        wasm,
        highlights: Some(dir.join(grammar_string(&dir, "highlights")?)),
        injections: Some(dir.join(grammar_string(&dir, "injections")?)),
        earlier_patterns_win: earlier_patterns_win.as_deref() == Some("true"),
        ..GrammarSpec::default()
    })
    .map_err(|error| format!("{language}: {error}"))
}

/// One line per highlight span, as `src/tests/golden.rs` writes them.
fn render(language: &'static Language, text: &str) -> Result<String, String> {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language, buffer.current());
    syntax.finish(syntax.parse_job().run());
    let tree = syntax.tree().ok_or("the sample doesn't parse")?;
    // Either kind of node: ERROR, or a MISSING one the parser made up.
    if tree.root_node().has_error() {
        return Err("the sample doesn't parse without errors".to_owned());
    }
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
    Ok(out)
}

/// Where `actual` first differs from `expected`, golden file `golden`'s.
fn first_difference(golden: &Path, expected: &str, actual: &str) -> String {
    let line = expected
        .lines()
        .zip(actual.lines())
        .position(|(expected, actual)| expected != actual)
        .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()));
    format!(
        "differs from {} at line {} ({} lines, wasm {}):\n  - {}\n  + {}",
        golden.display(),
        line + 1,
        expected.lines().count(),
        actual.lines().count(),
        expected.lines().nth(line).unwrap_or("(end)"),
        actual.lines().nth(line).unwrap_or("(end)"),
    )
}

/// `language`'s module highlights each of its samples as their golden files
/// say; with `update`, its own golden files are rewritten instead. What it
/// registers is withdrawn after, so that it can't resolve another language's
/// injections.
fn check_samples(modules: &Path, language: &str, update: bool) -> Result<(), String> {
    let mut registered = Vec::new();
    let result = (|| {
        for need in parity_needs(language) {
            if fe_syntax::language_by_name(&need).is_none() {
                load_as(modules, &need, &need)?;
                registered.push(need);
            }
        }
        // As the app registers it, unless a built-in may have the name.
        let name = match native_sample(language) {
            Some(_) => format!("{language}_wasm"),
            None => language.to_owned(),
        };
        let loaded = load_as(modules, language, &name)?;
        registered.push(name);

        let samples = samples(language);
        if samples.is_empty() {
            return Err("no sample".to_owned());
        }
        let mut differences = Vec::new();
        for sample in samples {
            let actual = render(loaded, &read(&sample.text))
                .map_err(|error| format!("{}: {error}", sample.text.display()))?;
            if update && sample.own {
                fs::write(&sample.golden, &actual)
                    .map_err(|error| format!("{}: {error}", sample.golden.display()))?;
                continue;
            }
            let Ok(expected) = fs::read_to_string(&sample.golden) else {
                differences.push(format!(
                    "{} is missing: write it with UPDATE_GOLDEN=1 {SCRIPT} {language}",
                    sample.golden.display()
                ));
                continue;
            };
            if expected != actual {
                differences.push(first_difference(&sample.golden, &expected, &actual));
            }
        }
        if differences.is_empty() {
            Ok(())
        } else {
            Err(differences.join("\n"))
        }
    })();
    for name in registered {
        unregister_language(&name);
    }
    result
}

/// The languages `GRAMMAR_LANGUAGES` names, separated by commas; every one if
/// it names none.
fn selected(all: &[String]) -> Vec<String> {
    let names = std::env::var("GRAMMAR_LANGUAGES").unwrap_or_default();
    let names: Vec<String> = (names.split(',').map(str::trim))
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect();
    if names.is_empty() {
        return all.to_vec();
    }
    for name in &names {
        assert!(
            all.contains(name),
            "GRAMMAR_LANGUAGES names {name}, which has no extensions/grammars/{name}/"
        );
    }
    names
}

#[test]
#[ignore = "needs scripts/build-grammar-wasm.sh"]
fn every_extension_highlights_its_sample() {
    // The checks are of what the modules highlight, not how fast they load:
    // a big one compiling while others build can take longer than the app's
    // deadline allows.
    fe_syntax::set_grammar_deadlines(fe_syntax::GrammarDeadlines {
        load: std::time::Duration::from_secs(120),
        ..fe_syntax::GrammarDeadlines::DEFAULT
    });
    let all = languages();
    let modules = repository().join("target/grammar-wasm");
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let selected = selected(&all);
    let failures: Vec<String> = (selected.iter())
        .filter_map(|language| {
            let result = check_samples(&modules, language, update);
            result.err().map(|error| format!("{language}: {error}"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} languages fail. If a change to an extension's own golden file is \
         intended, rerun with UPDATE_GOLDEN=1 and review the diff.\n\n{}",
        failures.len(),
        selected.len(),
        failures.join("\n\n")
    );
}

#[test]
#[should_panic(expected = "scripts/build-grammar-wasm.sh kotlin")]
fn a_missing_module_fails_naming_the_script() {
    load(
        &repository().join("target/grammar-wasm/no-such-dir"),
        "kotlin",
    );
}

/// What every extension directory holds, besides `LICENSE-*`, a `NOTICE` and
/// `test/`.
const REQUIRED: [&str; 5] = [
    "LICENSE",
    "extension.toml",
    "grammar-source.toml",
    "highlights.scm",
    "injections.scm",
];

/// The keys of each form of `grammar-source.toml`, besides `parity_needs`.
const CARGO_LOCK_KEYS: [&str; 4] = ["crate", "version", "src", "grammar"];
const GIT_KEYS: [&str; 4] = ["git", "rev", "src", "grammar"];
const CRATES_IO_KEYS: [&str; 6] = ["registry", "crate", "version", "sha256", "src", "grammar"];

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A name of letters, digits and `extra`.
fn is_name(text: &str, extra: &str) -> bool {
    !text.is_empty() && (text.chars()).all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

/// `https://github.com/<owner>/<repository>`.
fn is_github_repository(url: &str) -> bool {
    let Some(path) = url.strip_prefix("https://github.com/") else {
        return false;
    };
    let parts: Vec<&str> = path.split('/').collect();
    parts.len() == 2
        && (parts.iter()).all(|part| is_name(part, "_.-") && *part != "." && *part != "..")
}

/// A relative path that stays inside the directory it's relative to.
fn is_inside(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

/// The capture names in a query, without their `@`, outside its comments and
/// strings.
fn captures(query: &str) -> BTreeSet<&str> {
    let mut captures = BTreeSet::new();
    let bytes = query.as_bytes();
    let mut ix = 0;
    while ix < bytes.len() {
        match bytes[ix] {
            b';' => {
                while ix < bytes.len() && bytes[ix] != b'\n' {
                    ix += 1;
                }
            }
            b'"' => {
                ix += 1;
                while ix < bytes.len() && bytes[ix] != b'"' {
                    ix += if bytes[ix] == b'\\' { 2 } else { 1 };
                }
            }
            b'@' => {
                let start = ix + 1;
                ix = start;
                while ix < bytes.len()
                    && (bytes[ix].is_ascii_alphanumeric() || b"_.-".contains(&bytes[ix]))
                {
                    ix += 1;
                }
                captures.insert(&query[start..ix]);
                continue;
            }
            _ => {}
        }
        ix += 1;
    }
    captures
}

/// The files a directory has: what [`REQUIRED`] names, a licence's other
/// texts, and its samples with their golden files; never a module.
fn check_files(dir: &Path, native: bool, say: &mut dyn FnMut(String)) {
    let Ok(entries) = fs::read_dir(dir) else {
        say("can't be read".to_owned());
        return;
    };
    let mut names = BTreeSet::new();
    for entry in entries {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.file_type().unwrap().is_dir();
        if name == "test" && is_dir {
            check_test_dir(&dir.join("test"), say);
        } else if name.ends_with(".wasm") {
            say(format!(
                "{name}: the module isn't committed; {SCRIPT} builds it"
            ));
        } else if is_dir
            || !(REQUIRED.contains(&name.as_str())
                || name.starts_with("LICENSE-")
                || name == "NOTICE")
        {
            say(format!(
                "{name} isn't one of an extension's files (extensions/grammars/README.md)"
            ));
        }
        names.insert(name);
    }
    for file in REQUIRED {
        if !names.contains(file) {
            say(format!("has no {file}"));
        }
    }
    if !native && own_samples(dir).is_empty() {
        say(format!(
            "has no test/sample.<extension>: a language with no built-in holds a sample and \
             its golden file, written by UPDATE_GOLDEN=1 {SCRIPT} <language>"
        ));
    }
}

/// `test/` holds samples, each with its golden file, and nothing else.
fn check_test_dir(test: &Path, say: &mut dyn FnMut(String)) {
    for entry in fs::read_dir(test).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_file = entry.file_type().unwrap().is_file();
        if !is_file || !name.starts_with("sample.") {
            say(format!("test/{name} isn't a sample or a golden file"));
        } else if let Some(sample) = name.strip_suffix(".golden") {
            if !sample.starts_with("sample.") || !test.join(sample).is_file() {
                say(format!("test/{name} is no sample's golden file"));
            }
        } else if !test.join(format!("{name}.golden")).is_file() {
            say(format!(
                "test/{name} has no golden file test/{name}.golden: write it with \
                 UPDATE_GOLDEN=1 {SCRIPT} <language>"
            ));
        }
    }
}

/// `grammar-source.toml` is exactly one of its forms, its values valid; its
/// `grammar`, if it has one, and its source: the form's keys but `grammar`,
/// as `key=value` lines.
fn check_source(
    dir: &Path,
    language: &str,
    languages: &[String],
    say: &mut dyn FnMut(String),
) -> Option<(String, String)> {
    let text = fs::read_to_string(dir.join("grammar-source.toml")).ok()?;
    let mut values = BTreeMap::new();
    match keys(&text) {
        Ok(keys) => {
            for (table, key, value) in keys {
                if !table.is_empty() {
                    say(format!(
                        "grammar-source.toml: {key} is in a table, [{table}]"
                    ));
                } else if values.insert(key.clone(), value).is_some() {
                    say(format!("grammar-source.toml: {key} is there twice"));
                }
            }
        }
        Err(error) => {
            say(format!("grammar-source.toml: {error}"));
            return None;
        }
    }
    let (form, form_keys): (&str, &[&str]) = if values.contains_key("git") {
        ("git", &GIT_KEYS)
    } else if values.contains_key("registry") {
        ("crates.io", &CRATES_IO_KEYS)
    } else {
        ("Cargo.lock", &CARGO_LOCK_KEYS)
    };
    for key in values.keys() {
        if key != "parity_needs" && !form_keys.contains(&key.as_str()) {
            say(format!(
                "grammar-source.toml: {key} isn't a key of its {form} form (keys {})",
                form_keys.join(", ")
            ));
        }
    }
    let mut strings_of = BTreeMap::new();
    for key in form_keys {
        match values.get(*key).map(|value| string(value)) {
            None => say(format!("grammar-source.toml: its {form} form needs {key}")),
            Some(None) => say(format!("grammar-source.toml: {key} isn't a plain string")),
            Some(Some(value)) => {
                strings_of.insert(*key, value);
            }
        }
    }
    let mut check = |key: &str, valid: fn(&str) -> bool, what: &str| {
        if let Some(value) = strings_of.get(key)
            && !valid(value)
        {
            say(format!("grammar-source.toml: {key} {value:?} isn't {what}"));
        }
    };
    check(
        "git",
        is_github_repository,
        "https://github.com/<owner>/<repository>",
    );
    check(
        "rev",
        |rev| is_hex(rev, 40),
        "a commit's 40 lowercase hex digits",
    );
    check(
        "sha256",
        |hash| is_hex(hash, 64),
        "a SHA-256's 64 lowercase hex digits",
    );
    check(
        "registry",
        |registry| registry == "crates.io",
        "\"crates.io\"",
    );
    check("crate", |name| is_name(name, "_-"), "a crate's name");
    check("version", |version| is_name(version, ".+-"), "a version");
    check("src", is_inside, "a relative path inside the source");
    check(
        "grammar",
        |name| is_name(name, "_"),
        "the name in its export tree_sitter_<grammar>",
    );
    if let Some(needs) = values.get("parity_needs") {
        match strings(needs) {
            None => {
                say("grammar-source.toml: parity_needs isn't a one-line list of strings".into())
            }
            Some(needs) => {
                for need in needs {
                    if need == language || !languages.iter().any(|known| known == need) {
                        say(format!(
                            "grammar-source.toml: parity_needs names {need:?}, which isn't \
                             another directory under extensions/grammars"
                        ));
                    }
                }
            }
        }
    }
    let source = (strings_of.iter())
        .filter(|(key, _)| **key != "grammar")
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(" ");
    strings_of
        .get("grammar")
        .map(|grammar| ((*grammar).to_owned(), source))
}

/// The file extensions and names a directory's grammar is chosen for.
#[derive(Default)]
struct Claims {
    extensions: Vec<String>,
    file_names: Vec<String>,
}

/// `extension.toml` is this directory's, with one grammar: its language, its
/// module named after the export, and the query files the installer writes.
fn check_manifest(
    dir: &Path,
    language: &str,
    grammar: Option<&str>,
    say: &mut dyn FnMut(String),
) -> Claims {
    let mut claims = Claims::default();
    let Ok(text) = fs::read_to_string(dir.join("extension.toml")) else {
        return claims;
    };
    let keys = match keys(&text) {
        Ok(keys) => keys,
        Err(error) => {
            say(format!("extension.toml: {error}"));
            return claims;
        }
    };
    let tables = (text.lines())
        .filter(|line| line.trim() == "[[contributes.grammars]]")
        .count();
    if tables != 1 {
        say(format!(
            "extension.toml has {tables} [[contributes.grammars]], not 1"
        ));
    }
    let get = |table: &str, key: &str| {
        (keys.iter())
            .find(|(t, k, _)| t == table && k == key)
            .map(|(_, _, value)| value.as_str())
    };
    let mut expect = |table: &str, key: &str, expected: &str| {
        let actual = get(table, key).and_then(string);
        if actual != Some(expected) {
            say(format!(
                "extension.toml: {key} is {actual:?}, not {expected:?}"
            ));
        }
    };
    expect("", "id", &format!("tenmoku.grammar_{language}"));
    let grammars = "contributes.grammars";
    expect(grammars, "language", language);
    if let Some(grammar) = grammar {
        // The loader finds tree_sitter_<grammar> by the file's name.
        expect(
            grammars,
            "wasm",
            &format!("tree-sitter-{}.wasm", grammar.replace('_', "-")),
        );
    }
    expect(grammars, "highlights", "highlights.scm");
    expect(grammars, "injections", "injections.scm");
    for (key, into) in [
        ("extensions", &mut claims.extensions),
        ("file_names", &mut claims.file_names),
    ] {
        match get(grammars, key).map(strings) {
            None => {}
            Some(None) => say(format!(
                "extension.toml: {key} isn't a one-line list of strings"
            )),
            Some(Some(list)) => into.extend(list.into_iter().map(str::to_owned)),
        }
    }
    if claims.extensions.is_empty() {
        say("extension.toml: its grammar has no extensions, which the install offer is for".into());
    }
    claims
}

/// What's wrong with the extension directories under `root`, each a line
/// naming its directory: nothing, for what a language card may commit.
///
/// Each holds [`REQUIRED`], perhaps `LICENSE-*`, a `NOTICE` and `test/`'s
/// samples, and no module; a language with no built-in holds a sample and
/// its golden file. Its `grammar-source.toml` is one form, Cargo.lock's
/// crate, a GitHub commit or a crates.io archive, with valid values, and
/// its `parity_needs` are other directories. Its `extension.toml` is its
/// directory's language, with the module named after its export. No two
/// directories, nor a directory and a built-in language other than its own,
/// are for the same file: by extension, by name, or a name with the other's
/// extension, ignoring case. A language with no built-in has no capture in
/// its `highlights.scm` that no highlight takes, but those starting with
/// `_`; a built-in's are its queries verbatim, which the
/// `*_extension_is_as_built_in` tests check.
fn problems(root: &Path) -> Vec<String> {
    let languages = languages_in(root);
    let mut problems = Vec::new();
    // Each directory's language, with what it's for.
    let mut all_claims: Vec<(&String, Claims)> = Vec::new();
    // Each grammar export to the directories for it, with their sources.
    let mut exports: BTreeMap<String, Vec<(&String, String)>> = BTreeMap::new();
    for language in &languages {
        let dir = root.join(language);
        let native = native_sample(language).is_some();
        let mut say = |problem: String| {
            problems.push(format!("extensions/grammars/{language}: {problem}"));
        };
        check_files(&dir, native, &mut say);
        let found = check_source(&dir, language, &languages, &mut say);
        let grammar = found.as_ref().map(|(grammar, _)| grammar.as_str());
        let claims = check_manifest(&dir, language, grammar, &mut say);
        if let Some((grammar, source)) = found {
            exports.entry(grammar).or_default().push((language, source));
        }
        if !native && (fe_syntax::language_names()).any(|name| name.eq_ignore_ascii_case(language))
        {
            say(format!("{language} is a built-in language's name"));
        }
        // A built-in language may be for its files only if it's this one, built
        // in by its feature.
        let another = |built_in: &str| !(native && built_in == language.as_str());
        for extension in &claims.extensions {
            let path = format!("sample.{extension}");
            let built_in = if extension.contains('.') {
                built_in_claiming(extension)
            } else {
                fe_syntax::language_name_for_path(Path::new(&path))
            };
            if let Some(built_in) = built_in
                && another(built_in)
            {
                say(format!(
                    "the extension {extension:?} is the built-in {built_in}'s"
                ));
            }
        }
        for name in &claims.file_names {
            if let Some(built_in) = fe_syntax::language_name_for_path(Path::new(name))
                && another(built_in)
            {
                say(format!(
                    "the file name {name:?} is the built-in {built_in}'s"
                ));
            }
        }
        if !native {
            let query = fs::read_to_string(dir.join("highlights.scm")).unwrap_or_default();
            for capture in captures(&query) {
                if !capture.starts_with('_') && Highlight::from_capture(capture).is_none() {
                    say(format!(
                        "highlights.scm captures @{capture}, which no highlight takes: \
                         use one that does, or start it with _ if a predicate needs it"
                    ));
                }
            }
        }
        all_claims.push((language, claims));
    }

    // Directories sharing an export share its module, so one source.
    for (grammar, sharers) in &exports {
        for pair in sharers.windows(2) {
            let ((first, first_source), (second, second_source)) = (&pair[0], &pair[1]);
            if first_source != second_source {
                problems.push(format!(
                    "extensions/grammars: {first} and {second} both export \
                     tree_sitter_{grammar} but pin different sources \
                     ({first_source:?} and {second_source:?}): one grammar is built \
                     from one source"
                ));
            }
        }
    }

    // Each extension and file name, lowercased, to the directories for it.
    let mut extensions: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut file_names: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for (language, claims) in &all_claims {
        for (claimed, map) in [
            (&claims.extensions, &mut extensions),
            (&claims.file_names, &mut file_names),
        ] {
            for claim in claimed {
                let claimants = map.entry(claim.to_lowercase()).or_default();
                if !claimants.contains(&language.as_str()) {
                    claimants.push(language.as_str());
                }
            }
        }
    }
    for (prefix, map) in [("*.", &extensions), ("", &file_names)] {
        for (claim, claimants) in map {
            if claimants.len() > 1 {
                problems.push(format!(
                    "extensions/grammars: {} are each for {prefix}{claim}",
                    claimants.join(" and ")
                ));
            }
        }
    }
    // A file name with another directory's extension is a file both are for:
    // the loader gives it to whichever registered first.
    for (language, claims) in &all_claims {
        for name in &claims.file_names {
            let Some(extension) = Path::new(name).extension() else {
                continue;
            };
            let extension = extension.to_string_lossy().to_lowercase();
            for other in extensions.get(&extension).into_iter().flatten() {
                if other != language {
                    problems.push(format!(
                        "extensions/grammars/{language}: the file name {name:?} has \
                         {other}'s extension {extension:?}"
                    ));
                }
            }
        }
    }
    problems
}

/// The built-in language claiming `extension` as a whole, as
/// `register_language`'s clash check compares claims: `php`'s is no claim to
/// `blade.php`, which a file's path would give it with no Blade registered.
fn built_in_claiming(extension: &str) -> Option<&'static str> {
    // The check comes before the module is read, so nothing registers.
    let probe = GrammarSpec {
        language: "wasm-parity extension probe".into(),
        wasm: PathBuf::from("/nonexistent/tree-sitter-probe.wasm"),
        extensions: vec![extension.to_owned()],
        ..GrammarSpec::default()
    };
    match register_language(&probe) {
        Err(GrammarError::Clash {
            built_in,
            clash: Clash::Extension(_),
            ..
        }) => Some(built_in),
        Err(GrammarError::Read { .. }) => None,
        other => panic!("probing {extension:?}: {other:?}"),
    }
}

#[test]
fn every_extension_is_well_formed() {
    assert!(
        !languages().is_empty(),
        "extensions/grammars has no language"
    );
    let problems = problems(&grammars());
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// A directory holding extension directories, for [`problems`], removed on
/// drop.
struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("wasm-parity-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// A well-formed directory for `language`, a language with no built-in,
    /// whose grammar comes from `source`; its path.
    fn add(&self, language: &str, source: &str) -> PathBuf {
        let dir = self.0.join(language);
        fs::create_dir_all(dir.join("test")).unwrap();
        let manifest = format!(
            "id = \"tenmoku.grammar_{language}\"\nname = \"{language} syntax\"\n\
             version = \"1.0.0\"\nlicense = \"MIT\"\n\n[engine]\ntenmoku = \"1.0\"\n\n\
             [[contributes.grammars]]\nlanguage = \"{language}\"\n\
             wasm = \"tree-sitter-{language}.wasm\"\nextensions = [\"{language}\"]\n\
             highlights = \"highlights.scm\"\ninjections = \"injections.scm\"\n"
        );
        let files = [
            ("LICENSE", "MIT\n".to_owned()),
            ("extension.toml", manifest),
            ("grammar-source.toml", source.replace("<grammar>", language)),
            (
                "highlights.scm",
                "; A comment with @spell in it.\n(identifier) @variable\n\
                 ((identifier) @_name (#eq? @_name \"x@y\"))\n(string) @string.special\n"
                    .to_owned(),
            ),
            ("injections.scm", "; none\n".to_owned()),
            (&format!("test/sample.{language}"), "x\n".to_owned()),
            (&format!("test/sample.{language}.golden"), String::new()),
        ];
        for (file, text) in files {
            fs::write(dir.join(file), text).unwrap();
        }
        dir
    }

    fn problems(&self) -> Vec<String> {
        problems(&self.0)
    }

    /// The one problem there is, which says `what`.
    fn assert_problem(&self, what: &str) {
        let problems = self.problems();
        assert!(
            problems.len() == 1 && problems[0].contains(what),
            "expected one problem saying {what:?}: {problems:#?}"
        );
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const GIT_SOURCE: &str = "\
# A comment.
git = \"https://github.com/acme/tree-sitter-<grammar>\"
rev = \"0123456789abcdef0123456789abcdef01234567\"
src = \"grammars/<grammar>/src\"
grammar = \"<grammar>\"
";

const CRATES_IO_SOURCE: &str = "\
registry = \"crates.io\"
crate = \"tree-sitter-<grammar>\"
version = \"0.23.0\"
sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"
src = \"src\"
grammar = \"<grammar>\"
";

fn edit(file: &Path, from: &str, to: &str) {
    let text = read(file);
    assert!(text.contains(from), "{} has no {from:?}", file.display());
    fs::write(file, text.replace(from, to)).unwrap();
}

#[test]
fn a_new_languages_directory_can_be_well_formed() {
    let root = Root::new("well-formed");
    root.add("zzlang", GIT_SOURCE);
    let dir = root.add("yylang", CRATES_IO_SOURCE);
    edit(
        &dir.join("grammar-source.toml"),
        "grammar = ",
        "parity_needs = [\"zzlang\"]\ngrammar = ",
    );
    fs::write(dir.join("LICENSE-APACHE"), "").unwrap();
    fs::write(dir.join("NOTICE"), "").unwrap();
    assert_eq!(root.problems(), Vec::<String>::new());
}

#[test]
fn a_missing_sample_or_golden_file_is_a_problem() {
    let root = Root::new("sample");
    let dir = root.add("zzlang", GIT_SOURCE);
    fs::remove_file(dir.join("test/sample.zzlang.golden")).unwrap();
    root.assert_problem("test/sample.zzlang has no golden file");
    fs::remove_file(dir.join("test/sample.zzlang")).unwrap();
    root.assert_problem("has no test/sample.<extension>");
    fs::write(dir.join("test/sample.zzlang.golden"), "").unwrap();
    let problems = root.problems();
    assert_eq!(problems.len(), 2, "{problems:#?}");
    assert!(
        problems[0].contains("is no sample's golden file"),
        "{problems:#?}"
    );
}

#[test]
fn a_rev_that_isnt_a_full_commit_is_a_problem() {
    let root = Root::new("rev");
    let dir = root.add("zzlang", GIT_SOURCE);
    let source = dir.join("grammar-source.toml");
    edit(&source, "01234567\"", "\"");
    root.assert_problem("rev \"0123456789abcdef0123456789abcdef\" isn't a commit's 40");
    edit(
        &source,
        "0123456789abcdef0123456789abcdef\"",
        "0123456789ABCDEF0123456789ABCDEF01234567\"",
    );
    root.assert_problem("isn't a commit's 40 lowercase hex digits");
}

#[test]
fn a_capture_no_highlight_takes_is_a_problem() {
    let root = Root::new("capture");
    let dir = root.add("zzlang", GIT_SOURCE);
    fs::write(
        dir.join("highlights.scm"),
        "(identifier) @spell\n(string) @string\n",
    )
    .unwrap();
    root.assert_problem("highlights.scm captures @spell, which no highlight takes");
}

#[test]
fn a_built_in_languages_extension_or_file_name_is_a_problem() {
    let root = Root::new("built-in");
    let dir = root.add("zzlang", GIT_SOURCE);
    let manifest = dir.join("extension.toml");
    edit(
        &manifest,
        "extensions = [\"zzlang\"]",
        "extensions = [\"zzlang\", \"RS\"]",
    );
    root.assert_problem("the extension \"RS\" is the built-in rust's");
    edit(&manifest, ", \"RS\"]", "]\nfile_names = [\"Makefile\"]");
    root.assert_problem("the file name \"Makefile\" is the built-in make's");
}

/// A language fe-syntax also builds in may be for its own built-in's files,
/// with its feature on, but no other's.
#[test]
fn a_native_directory_claiming_another_built_ins_files_is_a_problem() {
    let root = Root::new("native");
    let dir = root.add("kotlin", GIT_SOURCE);
    let manifest = dir.join("extension.toml");
    edit(
        &manifest,
        "extensions = [\"kotlin\"]",
        "extensions = [\"kt\", \"RS\"]",
    );
    root.assert_problem("the extension \"RS\" is the built-in rust's");
    edit(&manifest, ", \"RS\"]", "]\nfile_names = [\"Makefile\"]");
    root.assert_problem("the file name \"Makefile\" is the built-in make's");
}

#[test]
fn a_file_name_with_another_directorys_extension_is_a_problem() {
    let root = Root::new("name-extension");
    root.add("zzlang", GIT_SOURCE);
    let dir = root.add("yylang", CRATES_IO_SOURCE);
    edit(
        &dir.join("extension.toml"),
        "extensions = [\"yylang\"]",
        "extensions = [\"yylang\"]\nfile_names = [\"Example.ZZlang\"]",
    );
    root.assert_problem("the file name \"Example.ZZlang\" has zzlang's extension \"zzlang\"");
}

#[test]
fn another_directorys_file_name_is_a_problem() {
    let root = Root::new("file-name");
    for language in ["yylang", "zzlang"] {
        let dir = root.add(language, GIT_SOURCE);
        edit(
            &dir.join("extension.toml"),
            &format!("extensions = [\"{language}\"]"),
            &format!(
                "extensions = [\"{language}\"]\nfile_names = [\"{language}file\", \"Buildfile\"]"
            ),
        );
    }
    fs::write(
        root.0.join("zzlang/extension.toml"),
        read(&root.0.join("zzlang/extension.toml")).replace("\"Buildfile\"", "\"BUILDFILE\""),
    )
    .unwrap();
    root.assert_problem("yylang and zzlang are each for buildfile");
}

/// A compound extension is a claim of its own: `blade.rs` isn't the built-in
/// rust's `rs`, nor `yy.zzlang` another directory's `zzlang`, though two
/// directories claiming the same compound one clash.
#[test]
fn a_compound_extension_is_a_claim_of_its_own() {
    let root = Root::new("compound");
    let zz = root.add("zzlang", GIT_SOURCE);
    let dir = root.add("yylang", CRATES_IO_SOURCE);
    edit(
        &dir.join("extension.toml"),
        "[\"yylang\"]",
        "[\"yylang\", \"blade.RS\", \"yy.zzlang\"]",
    );
    assert_eq!(root.problems(), Vec::<String>::new());
    edit(
        &zz.join("extension.toml"),
        "[\"zzlang\"]",
        "[\"zzlang\", \"Blade.rs\"]",
    );
    root.assert_problem("yylang and zzlang are each for *.blade.rs");
}

#[test]
fn another_directorys_extension_is_a_problem() {
    let root = Root::new("claimed");
    root.add("zzlang", GIT_SOURCE);
    let dir = root.add("yylang", CRATES_IO_SOURCE);
    edit(
        &dir.join("extension.toml"),
        "[\"yylang\"]",
        "[\"yylang\", \"ZZlang\"]",
    );
    root.assert_problem("yylang and zzlang are each for *.zzlang");
}

#[test]
fn a_source_of_no_one_form_is_a_problem() {
    let root = Root::new("source");
    let dir = root.add("zzlang", GIT_SOURCE);
    let source = dir.join("grammar-source.toml");
    edit(&source, "src = ", "version = \"1.0.0\"\nsrc = ");
    root.assert_problem("version isn't a key of its git form");
    fs::write(
        &source,
        CRATES_IO_SOURCE
            .replace("<grammar>", "zzlang")
            .replace("crates.io\"", "crates\""),
    )
    .unwrap();
    root.assert_problem("registry \"crates\" isn't \"crates.io\"");
    fs::write(
        &source,
        CRATES_IO_SOURCE
            .replace("<grammar>", "zzlang")
            .replace("abcdef\"", "\""),
    )
    .unwrap();
    root.assert_problem("isn't a SHA-256's 64 lowercase hex digits");
    fs::write(
        &source,
        GIT_SOURCE
            .replace("<grammar>", "zzlang")
            .replace("grammars/zzlang/src", "../src"),
    )
    .unwrap();
    root.assert_problem("src \"../src\" isn't a relative path inside the source");
    fs::write(
        &source,
        GIT_SOURCE
            .replace("<grammar>", "zzlang")
            .replace("https://github.com", "http://github.com"),
    )
    .unwrap();
    root.assert_problem("isn't https://github.com/<owner>/<repository>");
    fs::write(
        &source,
        format!(
            "{}parity_needs = [\"sql\"]\n",
            GIT_SOURCE.replace("<grammar>", "zzlang")
        ),
    )
    .unwrap();
    root.assert_problem("parity_needs names \"sql\", which isn't another directory");
}

#[test]
fn a_manifest_not_of_its_directory_is_a_problem() {
    let root = Root::new("manifest");
    let dir = root.add("zzlang", GIT_SOURCE);
    let manifest = dir.join("extension.toml");
    edit(&manifest, "language = \"zzlang\"", "language = \"yylang\"");
    root.assert_problem("language is Some(\"yylang\"), not \"zzlang\"");
    edit(
        &manifest,
        "language = \"yylang\"\nwasm = \"tree-sitter-zzlang.wasm\"",
        "language = \"zzlang\"\nwasm = \"zzlang.wasm\"",
    );
    root.assert_problem("wasm is Some(\"zzlang.wasm\"), not \"tree-sitter-zzlang.wasm\"");
}

#[test]
fn a_committed_module_or_another_file_is_a_problem() {
    let root = Root::new("files");
    let dir = root.add("zzlang", GIT_SOURCE);
    fs::write(dir.join("tree-sitter-zzlang.wasm"), "").unwrap();
    root.assert_problem("the module isn't committed");
    fs::remove_file(dir.join("tree-sitter-zzlang.wasm")).unwrap();
    fs::write(dir.join("test/notes.txt"), "").unwrap();
    root.assert_problem("test/notes.txt isn't a sample or a golden file");
    fs::remove_file(dir.join("test/notes.txt")).unwrap();
    fs::remove_file(dir.join("injections.scm")).unwrap();
    root.assert_problem("has no injections.scm");
}

/// The README's templates, copied as they are, make well-formed directories:
/// its two source forms and its manifest.
#[test]
fn the_readmes_templates_are_well_formed() {
    let readme = read(&grammars().join("README.md"));
    let blocks: Vec<&str> = (readme.split("```toml\n").skip(1))
        .map(|block| block.split("```").next().unwrap())
        .collect();
    let [git, crates_io, manifest] = blocks[..] else {
        panic!("README.md has {} TOML blocks, not 3", blocks.len());
    };
    assert!(
        git.lines().any(|line| line.starts_with("git = ")) && crates_io.contains("registry = ")
    );
    let root = Root::new("readme");
    let ocaml = root.add("ocaml", git);
    fs::write(ocaml.join("extension.toml"), manifest).unwrap();
    let zig = root.add("zig", crates_io);
    let zig_manifest =
        (manifest.replace("ocaml", "zig").replace("OCaml", "Zig")).replace("[\"ml\"]", "[\"zig\"]");
    fs::write(zig.join("extension.toml"), zig_manifest).unwrap();
    assert_eq!(root.problems(), Vec::<String>::new());
}

#[test]
fn each_pin_is_cargo_locks() {
    let lock = read(&repository().join("Cargo.lock"));
    for language in languages() {
        let source = extension(&language).join("grammar-source.toml");
        // The other forms aren't the crate fe-syntax links.
        if value_in(&source, "", "git").is_some() || value_in(&source, "", "registry").is_some() {
            continue;
        }
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

/// The script downloads the wasi-sdk the workflow installs, so a module
/// built here is the one the release has.
#[test]
fn the_script_downloads_the_workflows_wasi_sdk() {
    let script = read(&repository().join(SCRIPT));
    let workflow = read(&repository().join(".github/workflows/grammars.yml"));
    let pinned = |key: &str| {
        (workflow.lines())
            .find_map(|line| line.trim().strip_prefix(key))
            .map(|value| value.trim().trim_matches('\'').to_owned())
            .unwrap_or_else(|| panic!("grammars.yml has no {key}"))
    };
    let version = (script.lines())
        .find_map(|line| line.strip_prefix("wasi_sdk_version="))
        .expect("the script pins wasi_sdk_version");
    assert_eq!(version, pinned("WASI_SDK_VERSION:"));
    // The workflow's archive is the macOS arm64 one.
    let hash = pinned("WASI_SDK_SHA256:");
    assert!(
        script.contains(&format!("arm64-macos) echo {hash} ;;")),
        "the script's macOS arm64 hash isn't grammars.yml's {hash}"
    );
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
        Some(include_str!("../queries/cpp-injections.scm")),
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

/// Adds `language` as a directory for the export `grammar`, which `source`
/// (with no `<grammar>` in it) names.
fn add_sharing(root: &Root, language: &str, grammar: &str, source: &str) -> PathBuf {
    let dir = root.add(language, source);
    edit(
        &dir.join("extension.toml"),
        &format!("tree-sitter-{language}.wasm"),
        &format!("tree-sitter-{grammar}.wasm"),
    );
    dir
}

#[test]
fn directories_sharing_an_export_must_share_a_source() {
    let root = Root::new("shared-export");
    let source = GIT_SOURCE.replace("<grammar>", "zzshared");
    add_sharing(&root, "zzone", "zzshared", &source);
    let two = add_sharing(&root, "zztwo", "zzshared", &source);
    assert_eq!(root.problems(), Vec::<String>::new());
    edit(
        &two.join("grammar-source.toml"),
        "0123456789abcdef0123456789abcdef01234567",
        "1123456789abcdef0123456789abcdef01234567",
    );
    root.assert_problem("zzone and zztwo both export tree_sitter_zzshared");
}
