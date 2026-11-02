//! Comment delimiters of each language in the table. They follow VS Code's
//! language configurations, except Ruby's `=begin`/`=end` block, which must
//! start a row and so is left out.

/// A language's comment delimiters.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CommentTokens {
    /// The token that starts a line comment, if the language has one.
    pub line: Option<&'static str>,
    /// The open and close tokens of a block comment, if the language has one.
    pub block: Option<(&'static str, &'static str)>,
}

const fn tokens(
    line: Option<&'static str>,
    block: Option<(&'static str, &'static str)>,
) -> CommentTokens {
    CommentTokens { line, block }
}

/// The comment delimiters of the language called `language`, by the names
/// [`crate::language_names`] gives (and `markdown_inline`), whether or not its
/// Cargo feature is on. `None` for an unknown name.
pub fn comment_tokens(language: &str) -> Option<CommentTokens> {
    let c_like = tokens(Some("//"), Some(("/*", "*/")));
    let hash = tokens(Some("#"), None);
    let html = tokens(None, Some(("<!--", "-->")));
    Some(match language {
        "rust" | "json" | "javascript" | "typescript" | "tsx" | "go" | "c" | "cpp" | "java"
        | "php" | "csharp" | "scala" | "swift" | "kotlin" => c_like,
        "toml" | "bash" | "yaml" | "ruby" | "dockerfile" | "make" | "gitignore" | "gitcommit"
        | "gitrebase" | "gitconfig" | "gitattributes" | "dotenv" | "just" => hash,
        "python" => tokens(Some("#"), Some(("\"\"\"", "\"\"\""))),
        "ini" => tokens(Some(";"), None),
        "lua" => tokens(Some("--"), Some(("--[[", "]]"))),
        "sql" => tokens(Some("--"), Some(("/*", "*/"))),
        "css" => tokens(None, Some(("/*", "*/"))),
        "html" | "xml" | "markdown" => html,
        "markdown_inline" | "rustdoc_inline" => tokens(None, None),
        _ => return None,
    })
}

/// The pairs typing an opener closes in the language called `language`, by the
/// names [`crate::language_names`] gives (and `markdown_inline`), whether or not
/// its Cargo feature is on. Brackets come first, then quotes. An unknown name
/// (plain text) gets `()`, `[]`, `{}`, `""`.
pub fn auto_pairs(language: &str) -> &'static [(char, char)] {
    match language {
        "markdown_inline" | "rustdoc_inline" => &[],
        "rust" => &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"')],
        "javascript" | "typescript" | "tsx" | "go" | "bash" | "python" | "ruby" | "swift"
        | "json" | "just" => &[
            ('(', ')'),
            ('[', ']'),
            ('{', '}'),
            ('"', '"'),
            ('\'', '\''),
            ('`', '`'),
        ],
        "markdown" | "gitcommit" => &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('`', '`')],
        "php" | "lua" | "sql" | "toml" | "yaml" | "css" | "html" | "xml" | "dockerfile"
        | "make" | "scala" | "kotlin" | "csharp" | "java" | "c" | "cpp" | "gitrebase"
        | "gitconfig" | "gitattributes" | "ini" | "dotenv" => {
            &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')]
        }
        _ => &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"')],
    }
}
