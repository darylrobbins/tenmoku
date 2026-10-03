use std::{path::Path, sync::OnceLock};

use tree_sitter::Query;

use crate::Highlight;

/// A grammar with its highlight query, compiled on first use.
pub struct Language {
    name: &'static str,
    pub(crate) grammar: tree_sitter::Language,
    pub(crate) query: Query,
    /// The highlight for each of the query's captures, by capture index.
    pub(crate) highlights: Vec<Option<Highlight>>,
    pub(crate) earlier_patterns_win: bool,
}

impl Language {
    pub fn name(&self) -> &'static str {
        self.name
    }

    fn compile(def: &Def) -> Self {
        let grammar = (def.grammar)();
        let source = (def.highlights)().concat();
        let query = Query::new(&grammar, &source)
            .unwrap_or_else(|error| panic!("{} highlights query: {error}", def.name));
        let highlights = query
            .capture_names()
            .iter()
            .map(|name| Highlight::from_capture(name))
            .collect();
        Self {
            name: def.name,
            grammar,
            query,
            highlights,
            earlier_patterns_win: def.earlier_patterns_win,
        }
    }
}

impl std::fmt::Debug for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Language").field(&self.name).finish()
    }
}

/// The name of the language for a file, chosen by its extension or name.
pub fn language_name_for_path(path: &Path) -> Option<&'static str> {
    let file_name = path.file_name()?.to_str()?;
    let extension = path.extension().and_then(|ext| ext.to_str());
    DEFS.iter()
        .find(|def| {
            def.file_names.contains(&file_name)
                || extension.is_some_and(|ext| def.extensions.contains(&ext))
        })
        .map(|def| def.name)
}

/// The language for a file. See [`language_by_name`].
pub fn language_for_path(path: &Path) -> Option<&'static Language> {
    language_by_name(language_name_for_path(path)?)
}

/// The language with this name. Compiles its query on first use, which takes
/// up to about 20 ms; see [`loaded_language`] to avoid waiting.
pub fn language_by_name(name: &str) -> Option<&'static Language> {
    let ix = DEFS.iter().position(|def| def.name == name)?;
    Some(COMPILED[ix].get_or_init(|| Language::compile(&DEFS[ix])))
}

/// The language with this name if its query is already compiled.
pub fn loaded_language(name: &str) -> Option<&'static Language> {
    let ix = DEFS.iter().position(|def| def.name == name)?;
    COMPILED[ix].get()
}

/// Every language compiled into this build.
pub fn language_names() -> impl Iterator<Item = &'static str> {
    DEFS.iter().map(|def| def.name)
}

static COMPILED: [OnceLock<Language>; DEFS.len()] = [const { OnceLock::new() }; DEFS.len()];

struct Def {
    name: &'static str,
    extensions: &'static [&'static str],
    file_names: &'static [&'static str],
    grammar: fn() -> tree_sitter::Language,
    /// The highlight query, in parts concatenated in order. Later patterns
    /// win, so a language extending another lists the base language first,
    /// as the grammar's own `tree-sitter.json` does.
    highlights: fn() -> &'static [&'static str],
    /// When several patterns capture the same node, tree-sitter's highlighter
    /// uses the last, and most queries list general patterns first. A few
    /// older queries are written the other way round.
    earlier_patterns_win: bool,
}

/// One entry per Cargo feature. Every query is the one the grammar crate ships.
const DEFS: &[Def] = &[
    #[cfg(feature = "rust")]
    Def {
        name: "rust",
        extensions: &["rs"],
        file_names: &[],
        grammar: || tree_sitter_rust::LANGUAGE.into(),
        highlights: || &[tree_sitter_rust::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
    },
    #[cfg(feature = "python")]
    Def {
        name: "python",
        extensions: &["py", "pyi", "pyw"],
        file_names: &[],
        grammar: || tree_sitter_python::LANGUAGE.into(),
        highlights: || &[tree_sitter_python::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
    },
    #[cfg(feature = "javascript")]
    Def {
        name: "javascript",
        extensions: &["js", "mjs", "cjs", "jsx"],
        file_names: &[],
        grammar: || tree_sitter_javascript::LANGUAGE.into(),
        highlights: || {
            &[
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
            ]
        },
        earlier_patterns_win: false,
    },
    #[cfg(feature = "typescript")]
    Def {
        name: "typescript",
        extensions: &["ts", "mts", "cts"],
        file_names: &[],
        grammar: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        highlights: || {
            &[
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
            ]
        },
        earlier_patterns_win: false,
    },
    #[cfg(feature = "typescript")]
    Def {
        name: "tsx",
        extensions: &["tsx"],
        file_names: &[],
        grammar: || tree_sitter_typescript::LANGUAGE_TSX.into(),
        highlights: || {
            &[
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
            ]
        },
        earlier_patterns_win: false,
    },
    #[cfg(feature = "json")]
    Def {
        name: "json",
        extensions: &["json"],
        file_names: &[],
        grammar: || tree_sitter_json::LANGUAGE.into(),
        highlights: || &[tree_sitter_json::HIGHLIGHTS_QUERY],
        earlier_patterns_win: true,
    },
    #[cfg(feature = "toml")]
    Def {
        name: "toml",
        extensions: &["toml"],
        file_names: &["Cargo.lock"],
        grammar: || tree_sitter_toml_ng::LANGUAGE.into(),
        highlights: || &[tree_sitter_toml_ng::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
    },
    #[cfg(feature = "go")]
    Def {
        name: "go",
        extensions: &["go"],
        file_names: &[],
        grammar: || tree_sitter_go::LANGUAGE.into(),
        highlights: || &[tree_sitter_go::HIGHLIGHTS_QUERY],
        earlier_patterns_win: true,
    },
    #[cfg(feature = "c")]
    Def {
        name: "c",
        extensions: &["c", "h"],
        file_names: &[],
        grammar: || tree_sitter_c::LANGUAGE.into(),
        highlights: || &[tree_sitter_c::HIGHLIGHT_QUERY],
        earlier_patterns_win: false,
    },
    #[cfg(feature = "cpp")]
    Def {
        name: "cpp",
        extensions: &["cc", "cpp", "cxx", "c++", "hh", "hpp", "hxx", "h++"],
        file_names: &[],
        grammar: || tree_sitter_cpp::LANGUAGE.into(),
        highlights: || {
            &[
                tree_sitter_c::HIGHLIGHT_QUERY,
                tree_sitter_cpp::HIGHLIGHT_QUERY,
            ]
        },
        earlier_patterns_win: false,
    },
    #[cfg(feature = "bash")]
    Def {
        name: "bash",
        extensions: &["sh", "bash", "zsh"],
        file_names: &[".bashrc", ".bash_profile", ".zshrc", ".zshenv", ".profile"],
        grammar: || tree_sitter_bash::LANGUAGE.into(),
        highlights: || &[tree_sitter_bash::HIGHLIGHT_QUERY],
        earlier_patterns_win: false,
    },
    #[cfg(feature = "html")]
    Def {
        name: "html",
        extensions: &["html", "htm"],
        file_names: &[],
        grammar: || tree_sitter_html::LANGUAGE.into(),
        highlights: || &[tree_sitter_html::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
    },
    #[cfg(feature = "css")]
    Def {
        name: "css",
        extensions: &["css"],
        file_names: &[],
        grammar: || tree_sitter_css::LANGUAGE.into(),
        highlights: || &[tree_sitter_css::HIGHLIGHTS_QUERY],
        earlier_patterns_win: true,
    },
    #[cfg(feature = "yaml")]
    Def {
        name: "yaml",
        extensions: &["yaml", "yml"],
        file_names: &[],
        grammar: || tree_sitter_yaml::LANGUAGE.into(),
        highlights: || &[tree_sitter_yaml::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
    },
    // Block structure only: inline markup needs the inline grammar injected.
    #[cfg(feature = "markdown")]
    Def {
        name: "markdown",
        extensions: &["md", "markdown"],
        file_names: &[],
        grammar: || tree_sitter_md::LANGUAGE.into(),
        highlights: || &[tree_sitter_md::HIGHLIGHT_QUERY_BLOCK],
        earlier_patterns_win: false,
    },
    #[cfg(feature = "java")]
    Def {
        name: "java",
        extensions: &["java"],
        file_names: &[],
        grammar: || tree_sitter_java::LANGUAGE.into(),
        highlights: || &[tree_sitter_java::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
    },
];
