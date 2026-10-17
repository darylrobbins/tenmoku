use std::{path::Path, sync::OnceLock};

use tree_sitter::Query;

use crate::{Highlight, injection::InjectionQuery};

/// A grammar with its highlight query, compiled on first use.
pub struct Language {
    name: &'static str,
    pub(crate) grammar: tree_sitter::Language,
    pub(crate) query: Query,
    /// What each of the query's captures paints, by capture index: a
    /// highlight, or `Some(None)` for `@none`, which clears the highlights
    /// around it. Captures outside our vocabulary (`spell`, …) are `None`
    /// and ignored.
    pub(crate) highlights: Vec<Option<Option<Highlight>>>,
    pub(crate) earlier_patterns_win: bool,
    /// Whether no pattern spans several siblings, so a node's children can
    /// be queried one at a time. True of every query so far.
    pub(crate) local_patterns: bool,
    pub(crate) injections: Option<InjectionQuery>,
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
            .map(|name| match *name {
                "none" => Some(None),
                name => Highlight::from_capture(name).map(Some),
            })
            .collect();
        let local_patterns = (0..query.pattern_count()).all(|ix| !query.is_pattern_non_local(ix));
        let injections = def.injections.map(|source| {
            InjectionQuery::new(&grammar, source)
                .unwrap_or_else(|error| panic!("{} injections query: {error}", def.name))
        });
        Self {
            name: def.name,
            grammar,
            query,
            highlights,
            earlier_patterns_win: def.earlier_patterns_win,
            local_patterns,
            injections,
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
    listed()
        .find(|def| {
            def.file_names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(file_name))
                || extension.is_some_and(|ext| {
                    def.extensions
                        .iter()
                        .any(|known| known.eq_ignore_ascii_case(ext))
                })
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
    listed().map(|def| def.name)
}

fn listed() -> impl Iterator<Item = &'static Def> {
    DEFS.iter().filter(|def| !def.hidden)
}

/// Other names injections use for our languages, besides file extensions: the
/// fence names GitHub (linguist) and VS Code accept.
const ALIASES: &[(&str, &str)] = &[
    ("shell", "bash"),
    ("shell-script", "bash"),
    ("golang", "go"),
    ("c#", "csharp"),
    ("python3", "python"),
    ("docker", "dockerfile"),
    ("containerfile", "dockerfile"),
    ("makefile", "make"),
    ("node", "javascript"),
    ("javascriptreact", "javascript"),
    ("typescriptreact", "tsx"),
    ("jsonc", "json"),
];

/// What an injection's language name refers to.
pub(crate) enum Injected {
    Ready(&'static Language),
    /// A language of this build whose query isn't compiled yet. Compiling it
    /// here could take a few frames; see [`crate::Syntax::take_missing_languages`].
    Missing(&'static str),
    /// Not a language of this build.
    Unknown,
}

/// The language an injection names, ignoring case: by its name, a file
/// extension (`js`, `py`, `c++`) or an alias such as `shell`. Anything after
/// a comma is ignored, as in a ```` ```rust,ignore ```` fence.
pub(crate) fn injected_language(name: &str) -> Injected {
    let name = name.split(',').next().unwrap_or_default().trim();
    let name = name.to_ascii_lowercase();
    let name = ALIASES
        .iter()
        .find(|(alias, _)| *alias == name)
        .map_or(name.as_str(), |(_, canonical)| canonical);
    let Some(ix) = DEFS
        .iter()
        .position(|def| def.name == name)
        .or_else(|| DEFS.iter().position(|def| def.extensions.contains(&name)))
    else {
        return Injected::Unknown;
    };
    match COMPILED[ix].get() {
        Some(language) => Injected::Ready(language),
        None => Injected::Missing(DEFS[ix].name),
    }
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
    ///
    /// Where the crate's query is weak, our own `queries/<lang>.scm` goes
    /// after it as an override, so its patterns win on the same node (with
    /// `earlier_patterns_win` false). When the crate's query has a pattern
    /// that can't be cancelled that way, ours stands in for it instead, as
    /// TOML's does; the file's header says which.
    highlights: fn() -> &'static [&'static str],
    /// When several patterns capture the same node, tree-sitter's highlighter
    /// uses the last, and most queries list general patterns first. A few
    /// older queries are written the other way round.
    earlier_patterns_win: bool,
    /// The query marking code in other languages; see [`crate::injection`].
    injections: Option<&'static str>,
    /// Only reached through injections: not listed, nor chosen by file name.
    hidden: bool,
}

/// One entry per Cargo feature. Every query is the one the grammar crate ships,
/// plus an override from `queries/` where noted.
const DEFS: &[Def] = &[
    #[cfg(feature = "rust")]
    Def {
        name: "rust",
        extensions: &["rs"],
        file_names: &[],
        grammar: || tree_sitter_rust::LANGUAGE.into(),
        highlights: || &[tree_sitter_rust::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "python")]
    Def {
        name: "python",
        extensions: &["py", "pyi", "pyw"],
        file_names: &[],
        grammar: || tree_sitter_python::LANGUAGE.into(),
        highlights: || &[tree_sitter_python::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
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
        injections: Some(tree_sitter_javascript::INJECTIONS_QUERY),
        hidden: false,
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
        // TypeScript ships no injections query; JavaScript's fits its trees.
        injections: Some(tree_sitter_javascript::INJECTIONS_QUERY),
        hidden: false,
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
        injections: Some(tree_sitter_javascript::INJECTIONS_QUERY),
        hidden: false,
    },
    #[cfg(feature = "json")]
    Def {
        name: "json",
        extensions: &["json"],
        file_names: &[],
        grammar: || tree_sitter_json::LANGUAGE.into(),
        highlights: || &[tree_sitter_json::HIGHLIGHTS_QUERY],
        earlier_patterns_win: true,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "toml")]
    Def {
        name: "toml",
        extensions: &["toml"],
        file_names: &["Cargo.lock"],
        grammar: || tree_sitter_toml_ng::LANGUAGE.into(),
        highlights: || &[include_str!("../queries/toml.scm")],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "go")]
    Def {
        name: "go",
        extensions: &["go"],
        file_names: &[],
        grammar: || tree_sitter_go::LANGUAGE.into(),
        highlights: || &[tree_sitter_go::HIGHLIGHTS_QUERY],
        earlier_patterns_win: true,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "c")]
    Def {
        name: "c",
        extensions: &["c", "h"],
        file_names: &[],
        grammar: || tree_sitter_c::LANGUAGE.into(),
        highlights: || &[tree_sitter_c::HIGHLIGHT_QUERY],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
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
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "bash")]
    Def {
        name: "bash",
        extensions: &["sh", "bash", "zsh", "ksh"],
        file_names: &[
            ".bashrc",
            ".bash_profile",
            ".zshrc",
            ".zshenv",
            ".profile",
            ".zprofile",
            ".zlogin",
            ".zlogout",
            ".bash_login",
            ".bash_logout",
            ".bash_aliases",
            ".envrc",
            "PKGBUILD",
            "APKBUILD",
        ],
        grammar: || tree_sitter_bash::LANGUAGE.into(),
        highlights: || &[tree_sitter_bash::HIGHLIGHT_QUERY],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "html")]
    Def {
        name: "html",
        extensions: &["html", "htm"],
        file_names: &[],
        grammar: || tree_sitter_html::LANGUAGE.into(),
        highlights: || &[tree_sitter_html::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
        injections: Some(tree_sitter_html::INJECTIONS_QUERY),
        hidden: false,
    },
    #[cfg(feature = "css")]
    Def {
        name: "css",
        extensions: &["css"],
        file_names: &[],
        grammar: || tree_sitter_css::LANGUAGE.into(),
        highlights: || &[tree_sitter_css::HIGHLIGHTS_QUERY],
        earlier_patterns_win: true,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "yaml")]
    Def {
        name: "yaml",
        extensions: &["yaml", "yml"],
        file_names: &[],
        grammar: || tree_sitter_yaml::LANGUAGE.into(),
        highlights: || &[tree_sitter_yaml::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    // Block structure only: inline markup is `markdown_inline`, injected.
    #[cfg(feature = "markdown")]
    Def {
        name: "markdown",
        extensions: &["md", "markdown"],
        file_names: &[],
        grammar: || tree_sitter_md::LANGUAGE.into(),
        // The crate's query clears a fence's code with `@none`; we keep it
        // `markup.raw`, so a fence in a language we lack still stands out.
        // Later patterns win, so this overrides it.
        highlights: || {
            &[
                tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
                "\n(code_fence_content) @text.literal\n",
            ]
        },
        earlier_patterns_win: false,
        injections: Some(tree_sitter_md::INJECTION_QUERY_BLOCK),
        hidden: false,
    },
    #[cfg(feature = "markdown")]
    Def {
        name: "markdown_inline",
        extensions: &[],
        file_names: &[],
        grammar: || tree_sitter_md::INLINE_LANGUAGE.into(),
        highlights: || &[tree_sitter_md::HIGHLIGHT_QUERY_INLINE],
        earlier_patterns_win: false,
        injections: Some(tree_sitter_md::INJECTION_QUERY_INLINE),
        hidden: true,
    },
    #[cfg(feature = "java")]
    Def {
        name: "java",
        extensions: &["java"],
        file_names: &[],
        grammar: || tree_sitter_java::LANGUAGE.into(),
        highlights: || &[tree_sitter_java::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "ruby")]
    Def {
        name: "ruby",
        extensions: &["rb", "gemspec", "rake", "ru", "podspec"],
        file_names: &[
            "Gemfile",
            "Rakefile",
            "Podfile",
            "Vagrantfile",
            "Brewfile",
            "Guardfile",
            "Capfile",
            "Fastfile",
        ],
        grammar: || tree_sitter_ruby::LANGUAGE.into(),
        highlights: || {
            &[
                tree_sitter_ruby::HIGHLIGHTS_QUERY,
                include_str!("../queries/ruby.scm"),
            ]
        },
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "lua")]
    Def {
        name: "lua",
        extensions: &["lua"],
        file_names: &[],
        grammar: || tree_sitter_lua::LANGUAGE.into(),
        highlights: || &[tree_sitter_lua::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
        injections: Some(tree_sitter_lua::INJECTIONS_QUERY),
        hidden: false,
    },
    // PHP in an HTML page; the HTML around the `<?php` tags stays plain.
    #[cfg(feature = "php")]
    Def {
        name: "php",
        extensions: &["php"],
        file_names: &[],
        grammar: || tree_sitter_php::LANGUAGE_PHP.into(),
        highlights: || &[tree_sitter_php::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
        injections: Some(include_str!("../queries/php-injections.scm")),
        hidden: false,
    },
    #[cfg(feature = "csharp")]
    Def {
        name: "csharp",
        extensions: &["cs"],
        file_names: &[],
        grammar: || tree_sitter_c_sharp::LANGUAGE.into(),
        highlights: || &[tree_sitter_c_sharp::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "scala")]
    Def {
        name: "scala",
        extensions: &["scala", "sc"],
        file_names: &[],
        grammar: || tree_sitter_scala::LANGUAGE.into(),
        highlights: || &[tree_sitter_scala::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "xml")]
    Def {
        name: "xml",
        extensions: &["xml", "svg", "plist"],
        file_names: &[],
        grammar: || tree_sitter_xml::LANGUAGE_XML.into(),
        highlights: || &[tree_sitter_xml::XML_HIGHLIGHT_QUERY],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "swift")]
    Def {
        name: "swift",
        extensions: &["swift"],
        file_names: &[],
        grammar: || tree_sitter_swift::LANGUAGE.into(),
        highlights: || &[tree_sitter_swift::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "kotlin")]
    Def {
        name: "kotlin",
        extensions: &["kt", "kts"],
        file_names: &[],
        grammar: || tree_sitter_kotlin_sg::LANGUAGE.into(),
        highlights: || {
            &[
                tree_sitter_kotlin_sg::HIGHLIGHTS_QUERY,
                include_str!("../queries/kotlin.scm"),
            ]
        },
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "sql")]
    Def {
        name: "sql",
        extensions: &["sql"],
        file_names: &[],
        grammar: || tree_sitter_sequel::LANGUAGE.into(),
        highlights: || {
            &[
                tree_sitter_sequel::HIGHLIGHTS_QUERY,
                include_str!("../queries/sql.scm"),
            ]
        },
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "dockerfile")]
    Def {
        name: "dockerfile",
        extensions: &["dockerfile"],
        file_names: &["Dockerfile", "Containerfile"],
        grammar: || tree_sitter_containerfile::LANGUAGE.into(),
        highlights: || &[tree_sitter_containerfile::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
        injections: Some(tree_sitter_containerfile::INJECTIONS_QUERY),
        hidden: false,
    },
    #[cfg(feature = "make")]
    Def {
        name: "make",
        extensions: &["mk", "mak"],
        file_names: &["Makefile", "GNUmakefile"],
        grammar: || tree_sitter_make::LANGUAGE.into(),
        highlights: || &[tree_sitter_make::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
];
