//! The default features are the day-one built-in set (T427). The languages
//! that moved out of it load as first-party wasm extensions, and
//! `register_language` refuses a language built in under the same name or
//! extension, so the two must never overlap.

const MANIFEST: &str = include_str!("../Cargo.toml");

/// Each moved language with its extensions.
const MOVED: &[(&str, &[&str])] = &[
    ("cpp", &["cpp", "cc", "hpp"]),
    ("ruby", &["rb"]),
    ("php", &["php"]),
    ("csharp", &["cs"]),
    ("scala", &["scala"]),
    ("swift", &["swift"]),
    ("kotlin", &["kt", "kts"]),
    ("sql", &["sql"]),
];

/// The entries of the `default` list in the `[features]` table.
fn defaults() -> Vec<&'static str> {
    let features = MANIFEST.split("[features]").nth(1).unwrap();
    let list = features.split("default = [").nth(1).unwrap();
    let list = list.split(']').next().unwrap();
    list.split(',')
        .map(|entry| entry.trim().trim_matches('"'))
        .filter(|entry| !entry.is_empty())
        .collect()
}

#[test]
fn default_features_are_the_day_one_set() {
    let mut found = defaults();
    found.sort_unstable();
    let mut expected = [
        "rust",
        "python",
        "javascript",
        "typescript",
        "json",
        "toml",
        "go",
        "c",
        "bash",
        "html",
        "css",
        "yaml",
        "markdown",
        "java",
        "lua",
        "xml",
        "dockerfile",
        "make",
        "gitignore",
        "gitcommit",
        "gitrebase",
        "gitconfig",
        "gitattributes",
        "ini",
        "just",
        "diff",
        "json5",
        "csv",
    ];
    expected.sort_unstable();
    assert_eq!(found, expected);
    for (name, _) in MOVED {
        assert!(!found.contains(name), "{name}");
        // The feature itself stays defined.
        assert!(MANIFEST.contains(&format!("\n{name} = [\"dep:")), "{name}");
    }
}

#[cfg(not(any(
    feature = "cpp",
    feature = "ruby",
    feature = "php",
    feature = "csharp",
    feature = "scala",
    feature = "swift",
    feature = "kotlin",
    feature = "sql"
)))]
#[test]
fn moved_languages_are_not_built_in() {
    use std::path::Path;

    use fe_syntax::{language_by_name, language_name_for_path};

    for (name, extensions) in MOVED {
        assert!(language_by_name(name).is_none(), "{name}");
        for extension in *extensions {
            assert!(language_by_name(extension).is_none(), "{extension}");
            let file = format!("x.{extension}");
            assert!(language_name_for_path(Path::new(&file)).is_none(), "{file}");
        }
    }
}

#[cfg(all(
    feature = "cpp",
    feature = "ruby",
    feature = "php",
    feature = "csharp",
    feature = "scala",
    feature = "swift",
    feature = "kotlin",
    feature = "sql"
))]
#[test]
fn moved_languages_return_with_their_feature() {
    use std::path::Path;

    use fe_syntax::{language_by_name, language_name_for_path};

    for (name, _) in MOVED {
        assert!(language_by_name(name).is_some(), "{name}");
    }
    for (name, extensions) in MOVED {
        for extension in *extensions {
            let file = format!("x.{extension}");
            assert_eq!(
                language_name_for_path(Path::new(&file)),
                Some(*name),
                "{file}"
            );
        }
    }
}
