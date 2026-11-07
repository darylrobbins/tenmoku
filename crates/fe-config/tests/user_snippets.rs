//! The user's snippet files, in VS Code's format: parsed with comments and
//! trailing commas, scoped by the file's name or each snippet's `scope`,
//! bad entries dropped with a diagnostic naming the file and the snippet,
//! and the directory's stamp following its files.

use std::{fs, path::Path};

use fe_config::{
    Severity,
    user_snippets::{
        DirStamp, GLOBAL_SNIPPETS_FILE, UserSnippet, example_file, load_snippets,
        parse_snippet_file,
    },
};

/// Every body parses.
fn any(_: &str) -> Option<String> {
    None
}

/// A body parses unless it has a `${` never closed: enough of the engine's
/// check for these tests.
fn unclosed(body: &str) -> Option<String> {
    let open = body.matches("${").count();
    (open > body.matches('}').count()).then(|| "a `${` is never closed".to_owned())
}

fn snippet(
    file: &Path,
    name: &str,
    prefixes: &[&str],
    body: &str,
    description: Option<&str>,
    languages: &[&str],
) -> UserSnippet {
    UserSnippet {
        name: name.to_owned(),
        prefixes: prefixes.iter().map(|&p| p.to_owned()).collect(),
        body: body.to_owned(),
        description: description.map(str::to_owned),
        languages: languages.iter().map(|&l| l.to_owned()).collect(),
        file: file.to_owned(),
    }
}

#[test]
fn parses_the_vscode_format() {
    let dir = tempfile::tempdir().unwrap();
    let rust = dir.path().join("rust.json");
    fs::write(
        &rust,
        r##"{
    // A comment, and a block one.
    /* "Not": { "prefix": "no", "body": "no" }, */
    "Main function": {
        "prefix": "main",
        "body": "fn main() {\n\t$0\n}",
        "description": "A main function",
        // Ignored in a language's file.
        "scope": "python",
    },
    "Test": {
        "prefix": ["test", "tst"],
        "body": [
            "#[test]",
            "fn ${1:name}() {",
            "\t$0",
            "}",
        ],
    },
}
"##,
    )
    .unwrap();
    let parsed = parse_snippet_file(&rust, &fs::read_to_string(&rust).unwrap(), &any);
    assert_eq!(parsed.diagnostics, []);
    assert_eq!(
        parsed.snippets,
        [
            snippet(
                &rust,
                "Main function",
                &["main"],
                "fn main() {\n\t$0\n}",
                Some("A main function"),
                &["rust"],
            ),
            snippet(
                &rust,
                "Test",
                &["test", "tst"],
                "#[test]\nfn ${1:name}() {\n\t$0\n}",
                None,
                &["rust"],
            ),
        ]
    );
    assert!(parsed.snippets[0].is_for(Some("rust")) && parsed.snippets[0].is_for(Some("Rust")));
    assert!(!parsed.snippets[0].is_for(Some("python")) && !parsed.snippets[0].is_for(None));

    // A `.code-snippets` file: each snippet's `scope` lists its languages,
    // and one with none is for every language.
    let global = dir.path().join(GLOBAL_SNIPPETS_FILE);
    fs::write(
        &global,
        r##"// Leading comment
{
    "Todo": { "prefix": "todo", "body": "TODO($1): $0", "description": "A note" },
    "Header": { "prefix": "hdr", "body": "# $1", "scope": "python, shellscript ,," },
}"##,
    )
    .unwrap();
    let parsed = parse_snippet_file(&global, &fs::read_to_string(&global).unwrap(), &any);
    assert_eq!(parsed.diagnostics, []);
    assert_eq!(
        parsed.snippets,
        [
            snippet(
                &global,
                "Todo",
                &["todo"],
                "TODO($1): $0",
                Some("A note"),
                &[]
            ),
            snippet(
                &global,
                "Header",
                &["hdr"],
                "# $1",
                None,
                &["python", "shellscript"]
            ),
        ]
    );
    assert!(parsed.snippets[0].is_for(Some("go")) && parsed.snippets[0].is_for(None));
    assert!(parsed.snippets[1].is_for(Some("python")) && !parsed.snippets[1].is_for(Some("rust")));

    // The directory: every snippet file, by name, others skipped; none for
    // a directory that doesn't exist.
    fs::write(dir.path().join("notes.txt"), "{}").unwrap();
    fs::write(dir.path().join("empty.json"), "").unwrap();
    let loaded = load_snippets(dir.path(), &any);
    assert_eq!(loaded.diagnostics, []);
    let names: Vec<_> = (loaded.snippets.iter())
        .map(|snippet| snippet.name.as_str())
        .collect();
    assert_eq!(names, ["Todo", "Header", "Main function", "Test"]);
    assert_eq!(
        load_snippets(&dir.path().join("none"), &any),
        Default::default()
    );

    // The commented examples Configure User Snippets writes load, as
    // nothing.
    for language in [Some("rust"), None] {
        let example = example_file(language);
        assert!(example.contains("\"prefix\": \"log\""), "{example}");
        assert_eq!(
            language.is_none(),
            example.contains("\"scope\""),
            "{example}"
        );
        let parsed = parse_snippet_file(&rust, &example, &any);
        assert_eq!(parsed, Default::default(), "{example}");
    }
}

#[test]
fn bad_entries_are_dropped_with_a_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let go = dir.path().join("go.json");
    let text = r##"{
    "Good": { "prefix": "fn", "body": "func ${1:name}() {}" },
    "No prefix": { "prefix": "", "body": "x" },
    "Blank prefixes": { "prefix": ["", "  "], "body": "x" },
    "Unclosed": { "prefix": "un", "body": "func ${1:name() {" },
    "Numeric body": { "prefix": "num", "body": 42 },
    "Mixed lines": { "prefix": "mix", "body": ["a", 1] },
    "No body": { "prefix": "nob" },
    "Not an object": "fn main() {}",
    "Also good": { "prefix": ["ok"], "body": ["ok"] }
}"##;
    fs::write(&go, text).unwrap();
    let loaded = load_snippets(dir.path(), &unclosed);
    let names: Vec<_> = (loaded.snippets.iter())
        .map(|snippet| snippet.name.as_str())
        .collect();
    assert_eq!(names, ["Good", "Also good"]);
    let dropped: Vec<_> = (loaded.diagnostics.iter())
        .map(|d| (d.key.as_deref().unwrap(), d.line, d.severity))
        .collect();
    assert_eq!(
        dropped,
        [
            ("No prefix", 3, Severity::Warning),
            ("Blank prefixes", 4, Severity::Warning),
            ("Unclosed", 5, Severity::Warning),
            ("Numeric body", 6, Severity::Warning),
            ("Mixed lines", 7, Severity::Warning),
            ("No body", 8, Severity::Warning),
            ("Not an object", 9, Severity::Warning),
        ]
    );
    // Each message names the file and the snippet, and why.
    for (diagnostic, why) in loaded.diagnostics.iter().zip([
        "no prefix",
        "no prefix",
        "never closed",
        "isn't a string or a list of strings",
        "isn't a string or a list of strings",
        "no body",
        "isn't an object",
    ]) {
        let message = diagnostic.to_string();
        let name = diagnostic.key.as_deref().unwrap();
        assert!(message.starts_with(&go.display().to_string()), "{message}");
        assert!(message.contains(&format!("snippet {name:?}")), "{message}");
        assert!(message.contains(why), "{message}");
    }

    // A file that isn't an object of snippets loads nothing, with an error
    // naming it; the others still load.
    let broken = dir.path().join("broken.code-snippets");
    fs::write(&broken, "{ \"A\": { \"prefix\": \"a\", ").unwrap();
    let list = dir.path().join("list.json");
    fs::write(&list, "[1, 2]").unwrap();
    let loaded = load_snippets(dir.path(), &unclosed);
    let names: Vec<_> = (loaded.snippets.iter())
        .map(|snippet| snippet.name.as_str())
        .collect();
    assert_eq!(names, ["Good", "Also good"]);
    let errors: Vec<_> = (loaded.diagnostics.iter())
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.file.clone())
        .collect();
    assert_eq!(errors, [broken, list]);
}

#[test]
fn the_directory_stamp_follows_its_files() {
    let dir = tempfile::tempdir().unwrap();
    let snippets = dir.path().join("snippets");
    let none = DirStamp::of(&snippets);
    assert_eq!(none, DirStamp::default());
    fs::create_dir(&snippets).unwrap();
    assert_eq!(DirStamp::of(&snippets), none);

    let rust = snippets.join("rust.json");
    fs::write(&rust, "{}").unwrap();
    let written = DirStamp::of(&snippets);
    assert_ne!(written, none);
    // Other files don't count.
    fs::write(snippets.join("notes.txt"), "x").unwrap();
    assert_eq!(DirStamp::of(&snippets), written);

    fs::write(&rust, "{ }").unwrap();
    let rewritten = DirStamp::of(&snippets);
    assert_ne!(rewritten, written);
    fs::remove_file(&rust).unwrap();
    assert_eq!(DirStamp::of(&snippets), none);
}
