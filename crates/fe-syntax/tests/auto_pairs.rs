//! `auto_pairs`: the pairs typing an opener closes, per language.

use fe_syntax::{auto_pairs, language_names};

const BRACKETS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];

fn with_quotes(quotes: &[(char, char)]) -> Vec<(char, char)> {
    BRACKETS.iter().chain(quotes).copied().collect()
}

fn expected(name: &str) -> Vec<(char, char)> {
    let dq = ('"', '"');
    let sq = ('\'', '\'');
    let bt = ('`', '`');
    match name {
        "markdown_inline" => vec![],
        "csv" | "tsv" => vec![dq],
        "rust" => with_quotes(&[dq]),
        "markdown" | "gitcommit" => with_quotes(&[dq, bt]),
        "javascript" | "typescript" | "tsx" | "go" | "bash" | "python" | "ruby" | "swift"
        | "json" | "json5" | "just" => with_quotes(&[dq, sq, bt]),
        "gitignore" | "diff" => with_quotes(&[dq]),
        "php" | "lua" | "sql" | "toml" | "yaml" | "css" | "html" | "xml" | "dockerfile"
        | "make" | "scala" | "kotlin" | "csharp" | "java" | "c" | "cpp" | "gitrebase"
        | "gitconfig" | "gitattributes" | "ini" | "dotenv" => with_quotes(&[dq, sq]),
        other => panic!("no table entry for language {other}"),
    }
}

#[test]
fn every_language_has_an_entry() {
    for name in language_names() {
        assert_eq!(auto_pairs(name), expected(name), "{name}");
    }
    assert!(auto_pairs("markdown_inline").is_empty());
}

#[test]
fn values() {
    assert_eq!(
        auto_pairs("rust"),
        [('(', ')'), ('[', ']'), ('{', '}'), ('"', '"')]
    );
    assert!(!auto_pairs("rust").contains(&('\'', '\'')));
    assert!(auto_pairs("python").contains(&('\'', '\'')));
    assert_eq!(
        auto_pairs("javascript"),
        [
            ('(', ')'),
            ('[', ']'),
            ('{', '}'),
            ('"', '"'),
            ('\'', '\''),
            ('`', '`')
        ]
    );
    assert_eq!(
        auto_pairs("markdown"),
        [('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('`', '`')]
    );
    assert_eq!(
        auto_pairs("json"),
        [
            ('(', ')'),
            ('[', ']'),
            ('{', '}'),
            ('"', '"'),
            ('\'', '\''),
            ('`', '`')
        ]
    );
    assert_eq!(
        auto_pairs("no such language"),
        BRACKETS
            .iter()
            .copied()
            .chain([('"', '"')])
            .collect::<Vec<_>>()
    );
}
