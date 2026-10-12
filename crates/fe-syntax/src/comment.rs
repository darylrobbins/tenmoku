//! Comment delimiters of each language in the table.

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
        "rust" | "javascript" | "typescript" | "tsx" | "go" | "c" | "cpp" | "java" | "php"
        | "csharp" | "scala" | "swift" | "kotlin" => c_like,
        "python" | "toml" | "bash" | "yaml" | "ruby" | "dockerfile" | "make" => hash,
        "lua" => tokens(Some("--"), Some(("--[[", "]]"))),
        "sql" => tokens(Some("--"), Some(("/*", "*/"))),
        "css" => tokens(None, Some(("/*", "*/"))),
        "html" | "xml" | "markdown" => html,
        "json" | "markdown_inline" => tokens(None, None),
        _ => return None,
    })
}
