//! Patches, JSON5 and the JSON-with-comments files VS Code calls JSONC.

#![cfg(all(
    feature = "diff",
    feature = "json",
    feature = "json5",
    feature = "markdown"
))]

use std::{fs, path::Path};

use fe_buffer::Buffer;
use fe_syntax::{Language, Syntax, language_by_name, language_name_for_path};

fn parsed(language: &'static Language, text: &str) -> Syntax {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language, &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());
    syntax
}

/// `text`'s highlighted runs in `language`, as `(text, highlight)`.
fn highlighted<'a>(language: &str, text: &'a str) -> Vec<(&'a str, &'static str)> {
    parsed(language_by_name(language).unwrap(), text)
        .highlights(0..text.len())
        .into_iter()
        .map(|span| (&text[span.range], span.highlight.name()))
        .collect()
}

#[test]
fn jsonc_comments_and_trailing_commas_keep_their_highlights() {
    let text = r#"{
  // Line comment
  "compilerOptions": {
    /* Block comment */
    "target": "es2022",
    "strict": true,
  },
  "include": ["src"],
  "exclude": ["dist",],
  "last": 1,
}
"#;
    assert_eq!(
        language_name_for_path(Path::new("proj/tsconfig.json")),
        Some("json")
    );
    let found = highlighted("json", text);
    for comment in ["// Line comment", "/* Block comment */"] {
        assert!(
            found.contains(&(comment, "comment")),
            "{comment}: {found:?}"
        );
    }
    for key in [
        "\"compilerOptions\"",
        "\"target\"",
        "\"strict\"",
        "\"include\"",
        "\"exclude\"",
        "\"last\"",
    ] {
        assert!(
            found.contains(&(key, "string.special.key")),
            "{key}: {found:?}"
        );
    }
}

#[test]
fn vs_code_jsonc_files_are_json() {
    for name in ["x.code-workspace", "x.code-snippets", "X.CODE-WORKSPACE"] {
        assert_eq!(
            language_name_for_path(Path::new(name)),
            Some("json"),
            "{name}"
        );
    }
}

#[test]
fn patches_are_diff() {
    for name in ["x.diff", "x.patch", "X.PATCH", "a/b/0001-fix.patch"] {
        assert_eq!(
            language_name_for_path(Path::new(name)),
            Some("diff"),
            "{name}"
        );
    }
}

#[test]
fn json5_files_are_json5() {
    assert_eq!(language_name_for_path(Path::new("x.json5")), Some("json5"));
}

/// The highlight at `needle` in `source`, in a Markdown fence called `fence`.
fn fence_highlight(fence: &str, source: &str, needle: &str) -> Option<&'static str> {
    let text = format!("```{fence}\n{source}\n```\n");
    let start = text.rfind(needle).unwrap();
    parsed(language_by_name("markdown").unwrap(), &text)
        .highlights(0..text.len())
        .into_iter()
        .find(|span| span.range.start == start)
        .map(|span| span.highlight.name())
}

#[test]
fn fences_highlight_with_their_own_grammars() {
    language_by_name("json5").unwrap();
    language_by_name("diff").unwrap();
    // JSON's grammar has no unquoted keys or hex numbers.
    assert_eq!(
        fence_highlight("json5", "{a: 0xFF}", "a:"),
        Some("string.special.key")
    );
    assert_eq!(
        fence_highlight("JSON5", "{a: 0xFF}", "0xFF"),
        Some("number")
    );
    assert_eq!(
        fence_highlight("diff", "@@ -1 +1 @@\n-a\n+b", "-a"),
        Some("markup.deleted")
    );
    assert_eq!(
        fence_highlight("diff", "@@ -1 +1 @@\n-a\n+b", "+b"),
        Some("markup.inserted")
    );
}

#[test]
fn every_theme_colours_additions_and_deletions() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let themes = Path::new(&manifest).join("../fe-config/themes");
    let mut seen = 0;
    for entry in fs::read_dir(themes).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "toml") {
            continue;
        }
        let theme = fs::read_to_string(&path).unwrap();
        let (_, syntax) = theme.split_once("[syntax]").unwrap();
        let colour = |name: &str| {
            syntax
                .lines()
                .find_map(|line| line.strip_prefix(&format!("\"{name}\" = ")))
                .unwrap_or_else(|| panic!("{} leaves {name} unstyled", path.display()))
        };
        assert_ne!(
            colour("markup.inserted"),
            colour("markup.deleted"),
            "{}",
            path.display()
        );
        seen += 1;
    }
    assert!(seen >= 10, "found {seen} themes");
}
