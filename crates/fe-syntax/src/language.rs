use std::{
    path::{Component, Path, PathBuf},
    sync::OnceLock,
};

use tree_sitter::{Query, QueryError};

use crate::{
    Highlight,
    injection::InjectionQuery,
    runtime::{self, Clash, GrammarSpec},
};

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
    /// The module a [registered](crate::register_language) language was
    /// loaded from.
    pub(crate) wasm: Option<PathBuf>,
}

impl Language {
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Its usual file extension, without the dot: the first a language compiled
    /// into this build lists. `None` for a [registered](crate::register_language)
    /// one, and for one known only by its file names.
    pub fn first_extension(&self) -> Option<&'static str> {
        let def = DEFS.iter().find(|def| def.name == self.name)?;
        def.extensions.first().copied()
    }

    fn compile(def: &Def) -> Self {
        let source = (def.highlights)().concat();
        Self::new(
            def.name,
            (def.grammar)(),
            &source,
            def.earlier_patterns_win,
            def.injections,
        )
        .unwrap_or_else(|(kind, error)| panic!("{} {kind} query: {error}", def.name))
    }

    /// A language of `grammar` with these queries; an error says which query
    /// failed.
    pub(crate) fn new(
        name: &'static str,
        grammar: tree_sitter::Language,
        highlights: &str,
        earlier_patterns_win: bool,
        injections: Option<&str>,
    ) -> Result<Self, (QueryKind, QueryError)> {
        let query =
            Query::new(&grammar, highlights).map_err(|error| (QueryKind::Highlights, error))?;
        let highlights = query
            .capture_names()
            .iter()
            .map(|name| match *name {
                "none" => Some(None),
                name => Highlight::from_capture(name).map(Some),
            })
            .collect();
        let local_patterns = (0..query.pattern_count()).all(|ix| !query.is_pattern_non_local(ix));
        let injections = injections
            .map(|source| InjectionQuery::new(&grammar, source))
            .transpose()
            .map_err(|error| (QueryKind::Injections, error))?;
        Ok(Self {
            name,
            grammar,
            query,
            highlights,
            earlier_patterns_win,
            local_patterns,
            injections,
            wasm: None,
        })
    }
}

/// Which of a language's queries [`Language::new`] couldn't compile.
#[derive(Clone, Copy, Debug)]
pub(crate) enum QueryKind {
    Highlights,
    Injections,
}

impl std::fmt::Display for QueryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Highlights => "highlights",
            Self::Injections => "injections",
        })
    }
}

impl std::fmt::Debug for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Language").field(&self.name).finish()
    }
}

/// Files whose name alone doesn't say what they are, by the end of their path:
/// `(tail, language)`. Checked before file names and extensions.
const PATH_SUFFIXES: &[(&str, &str)] = &[
    (".git/config", "gitconfig"),
    (".config/git/config", "gitconfig"),
    (".git/info/attributes", "gitattributes"),
    (".config/git/attributes", "gitattributes"),
    (".git/info/exclude", "gitignore"),
    (".config/git/ignore", "gitignore"),
];

/// Files named by how their name starts, whatever follows: `(prefix, language)`,
/// ignoring ASCII case. Checked last, so `.env.sh` stays a shell script.
const FILE_NAME_PREFIXES: &[(&str, &str)] = &[(".env.", "dotenv")];

/// Whether `path` ends with the components of `tail`, ignoring ASCII case as
/// file names do.
fn path_ends_with(path: &Path, tail: &str) -> bool {
    let mut components = path.components().rev();
    tail.rsplit('/').all(|part| {
        matches!(
            components.next(),
            Some(Component::Normal(name)) if name.eq_ignore_ascii_case(part)
        )
    })
}

/// The name of the language for a file, chosen by the end of its path, its
/// extension or its name: a built-in language's, else a
/// [registered](crate::register_language) one's, else by how its name starts.
pub fn language_name_for_path(path: &Path) -> Option<&'static str> {
    let by_tail = PATH_SUFFIXES
        .iter()
        .filter(|(tail, _)| path_ends_with(path, tail))
        .find_map(|(_, language)| listed().find(|def| def.name == *language));
    if let Some(def) = by_tail {
        return Some(def.name);
    }
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
        .or_else(|| runtime::name_for_file(file_name, extension))
        .or_else(|| language_by_file_name_prefix(file_name))
}

/// The built-in language [`FILE_NAME_PREFIXES`] gives a file name.
fn language_by_file_name_prefix(file_name: &str) -> Option<&'static str> {
    FILE_NAME_PREFIXES
        .iter()
        .filter(|(prefix, _)| {
            file_name
                .as_bytes()
                .get(..prefix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(prefix.as_bytes()))
        })
        .find_map(|(_, language)| listed().find(|def| def.name == *language))
        .map(|def| def.name)
}

/// The language for a file. See [`language_by_name`].
pub fn language_for_path(path: &Path) -> Option<&'static Language> {
    language_by_name(language_name_for_path(path)?)
}

/// The language with this name. Compiles its query on first use, which takes
/// up to about 20 ms; see [`loaded_language`] to avoid waiting. A
/// [registered](crate::register_language) language is compiled already.
pub fn language_by_name(name: &str) -> Option<&'static Language> {
    let Some(ix) = DEFS.iter().position(|def| def.name == name) else {
        return runtime::by_name(name);
    };
    Some(COMPILED[ix].get_or_init(|| Language::compile(&DEFS[ix])))
}

/// The language with this name if its query is already compiled.
pub fn loaded_language(name: &str) -> Option<&'static Language> {
    let Some(ix) = DEFS.iter().position(|def| def.name == name) else {
        return runtime::by_name(name);
    };
    COMPILED[ix].get()
}

/// Every language compiled into this build, then those
/// [registered](crate::register_language) so far.
pub fn language_names() -> impl Iterator<Item = &'static str> {
    listed().map(|def| def.name).chain(runtime::names())
}

/// The built-in language `spec` would take a name, extension or file name
/// from, and which.
pub(crate) fn built_in_clash(spec: &GrammarSpec) -> Option<(&'static str, Clash)> {
    DEFS.iter().find_map(|def| {
        if def.name.eq_ignore_ascii_case(&spec.language) {
            return Some((def.name, Clash::Name));
        }
        let extension = spec.extensions.iter().find(|extension| {
            def.extensions
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        });
        if let Some(extension) = extension {
            return Some((def.name, Clash::Extension(extension.clone())));
        }
        let file_name = spec.file_names.iter().find(|file_name| {
            def.file_names
                .iter()
                .any(|known| known.eq_ignore_ascii_case(file_name))
        });
        file_name.map(|file_name| (def.name, Clash::FileName(file_name.clone())))
    })
}

fn listed() -> impl Iterator<Item = &'static Def> {
    DEFS.iter().filter(|def| !def.hidden)
}

/// Other names injections use for our languages, besides file extensions: the
/// fence names GitHub (linguist) accepts and those VS Code's Markdown
/// grammar does too.
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
    ("xhtml", "html"),
    ("shtml", "html"),
    ("php3", "php"),
    ("php4", "php"),
    ("php5", "php"),
    ("phpt", "php"),
    ("phtml", "php"),
    ("ctp", "php"),
    ("ddl", "sql"),
    ("dml", "sql"),
    ("xsd", "xml"),
    ("tld", "xml"),
    ("rss", "xml"),
    ("opml", "xml"),
    ("xsl", "xml"),
    ("xslt", "xml"),
    ("es6", "javascript"),
    ("py3", "python"),
    ("rpy", "python"),
    ("cpy", "python"),
    ("gyp", "python"),
    ("gypi", "python"),
    ("sconstruct", "python"),
    ("sconscript", "python"),
    ("sbt", "scala"),
    ("bashrc", "bash"),
    ("bash_profile", "bash"),
    ("bash_login", "bash"),
    ("bash_logout", "bash"),
    ("profile", "bash"),
    ("rbx", "ruby"),
    ("rjs", "ruby"),
    ("gemfile", "ruby"),
    ("rakefile", "ruby"),
    ("irbrc", "ruby"),
    ("capfile", "ruby"),
    ("vagrantfile", "ruby"),
    ("bsh", "java"),
    ("gnumakefile", "make"),
];

/// What an injection's language name refers to.
pub(crate) enum Injected {
    Ready(&'static Language),
    /// A language of this build whose query isn't compiled yet. Compiling it
    /// here could take a few frames; see [`crate::Syntax::take_missing_languages`].
    Missing(&'static str),
    /// Neither a language of this build nor a registered one.
    Unknown,
}

/// The language an injection names, ignoring case: by its name, a file
/// extension (`js`, `py`, `c++`) or an alias such as `shell`. Anything after
/// a comma or colon is ignored, as in ```` ```rust,ignore ```` or
/// ```` ```js:app.js ```` fences.
pub(crate) fn injected_language(name: &str) -> Injected {
    let name = name.split([',', ':']).next().unwrap_or_default().trim();
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
        return runtime::injected(name).map_or(Injected::Unknown, Injected::Ready);
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
        injections: Some(include_str!("../queries/rust-injections.scm")),
        hidden: false,
    },
    #[cfg(feature = "python")]
    Def {
        name: "python",
        extensions: &["py", "pyi", "pyw"],
        file_names: &[],
        grammar: || tree_sitter_python::LANGUAGE.into(),
        highlights: || {
            &[
                tree_sitter_python::HIGHLIGHTS_QUERY,
                include_str!("../queries/python.scm"),
            ]
        },
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
        extensions: &[
            "json",
            "jsonc",
            "webmanifest",
            "har",
            "jsonld",
            "geojson",
            "ipynb",
            "bowerrc",
            "jscsrc",
            "jslintrc",
            "vuerc",
            "eslintrc",
            "jsfmtrc",
            "jshintrc",
            "swcrc",
            "hintrc",
            "babelrc",
            "code-workspace",
            "code-snippets",
        ],
        file_names: &[
            ".bowerrc",
            ".jscsrc",
            ".jslintrc",
            ".vuerc",
            ".eslintrc",
            ".jsfmtrc",
            ".jshintrc",
            ".swcrc",
            ".hintrc",
            ".babelrc",
            "composer.lock",
            "bun.lock",
            ".watchmanconfig",
            ".ember-cli",
        ],
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
        injections: Some(include_str!("../queries/go-injections.scm")),
        hidden: false,
    },
    // Go format strings, injected for their verbs. The grammar only parses the
    // layer: src/verbs.rs finds the verbs, as the C grammar misreads Go's.
    #[cfg(feature = "printf")]
    Def {
        name: "printf",
        extensions: &[],
        file_names: &[],
        grammar: || tree_sitter_printf::LANGUAGE.into(),
        highlights: || &[],
        earlier_patterns_win: false,
        injections: None,
        hidden: true,
    },
    // C format strings: the same grammar, scanned with the C dialect.
    #[cfg(feature = "printf")]
    Def {
        name: "printf-c",
        extensions: &[],
        file_names: &[],
        grammar: || tree_sitter_printf::LANGUAGE.into(),
        highlights: || &[],
        earlier_patterns_win: false,
        injections: None,
        hidden: true,
    },
    #[cfg(feature = "c")]
    Def {
        name: "c",
        extensions: &["c", "h"],
        file_names: &[],
        grammar: || tree_sitter_c::LANGUAGE.into(),
        highlights: || &[tree_sitter_c::HIGHLIGHT_QUERY],
        earlier_patterns_win: false,
        injections: Some(include_str!("../queries/c-injections.scm")),
        hidden: false,
    },
    // An extension by default (T427): built in only with the `cpp` feature.
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
        injections: Some(include_str!("../queries/cpp-injections.scm")),
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
        // The upstream query leaves `||` and a trailing `&` plain. A run of
        // words (bash's `concatenation`) that is exactly `{NAME}` or `{NAME:-x}` is what a Makefile recipe's
        // double-dollar `{NAME:-x}` leaves for bash once make takes the
        // double dollar out.
        highlights: || {
            &[
                tree_sitter_bash::HIGHLIGHT_QUERY,
                "[\"||\" \"&\"] @operator\n\
                 ((concatenation) @variable\n\
                 (#match? @variable \"^[{][A-Za-z_][A-Za-z0-9_]*([:]?[-=+?][^{}]*)?[}]$\"))",
            ]
        },
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
        // The crate's query colours a URL autolink but not an email one.
        highlights: || {
            &[
                tree_sitter_md::HIGHLIGHT_QUERY_INLINE,
                "\n(email_autolink) @text.uri\n",
            ]
        },
        earlier_patterns_win: false,
        injections: Some(tree_sitter_md::INJECTION_QUERY_INLINE),
        hidden: true,
    },
    // Inline Markdown in Rust doc comments: only code spans and link text are highlighted.
    #[cfg(all(feature = "markdown", feature = "rust"))]
    Def {
        name: "rustdoc_inline",
        extensions: &[],
        file_names: &[],
        grammar: || tree_sitter_md::INLINE_LANGUAGE.into(),
        highlights: || &[include_str!("../queries/rustdoc-inline-highlights.scm")],
        earlier_patterns_win: false,
        injections: None,
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
    // An extension by default (T427): built in only with the `ruby` feature.
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
    // An extension by default (T427): built in only with the `php` feature.
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
    // An extension by default (T427): built in only with the `csharp` feature.
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
    // An extension by default (T427): built in only with the `scala` feature.
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
    // An extension by default (T427): built in only with the `swift` feature.
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
    // An extension by default (T427): built in only with the `kotlin` feature.
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
    // An extension by default (T427): built in only with the `sql` feature.
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
        highlights: || &[include_str!("../queries/make.scm")],
        earlier_patterns_win: false,
        injections: Some(include_str!("../queries/make-injections.scm")),
        hidden: false,
    },
    #[cfg(feature = "gitignore")]
    Def {
        name: "gitignore",
        extensions: &["gitignore"],
        file_names: &[
            ".gitignore",
            ".dockerignore",
            ".npmignore",
            ".prettierignore",
            ".eslintignore",
            ".stylelintignore",
            ".vscodeignore",
            ".helmignore",
            ".gcloudignore",
            ".ignore",
            ".rgignore",
            ".fdignore",
        ],
        grammar: || tree_sitter_gitignore::LANGUAGE.into(),
        highlights: || &[include_str!("../queries/gitignore.scm")],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "gitcommit")]
    Def {
        name: "gitcommit",
        extensions: &[],
        file_names: &[
            "COMMIT_EDITMSG",
            "MERGE_MSG",
            "TAG_EDITMSG",
            "SQUASH_MSG",
            "NOTES_EDITMSG",
            "EDIT_DESCRIPTION",
        ],
        grammar: || tree_sitter_git_commit::LANGUAGE.into(),
        highlights: || &[include_str!("../queries/gitcommit.scm")],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "gitrebase")]
    Def {
        name: "gitrebase",
        extensions: &[],
        file_names: &["git-rebase-todo"],
        grammar: || tree_sitter_git_rebase::LANGUAGE.into(),
        highlights: || {
            &[
                tree_sitter_git_rebase::HIGHLIGHTS_QUERY,
                include_str!("../queries/gitrebase.scm"),
            ]
        },
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "gitconfig")]
    Def {
        name: "gitconfig",
        extensions: &[],
        file_names: &[".gitconfig", ".gitmodules"],
        grammar: || tree_sitter_git_config::LANGUAGE.into(),
        highlights: || &[tree_sitter_git_config::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "gitattributes")]
    Def {
        name: "gitattributes",
        extensions: &[],
        file_names: &[".gitattributes"],
        grammar: || tree_sitter_gitattributes::LANGUAGE.into(),
        highlights: || &[tree_sitter_gitattributes::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "ini")]
    Def {
        name: "ini",
        extensions: &["ini", "cfg"],
        file_names: &[
            ".editorconfig",
            ".npmrc",
            ".pypirc",
            ".flake8",
            ".coveragerc",
            ".pylintrc",
            "pylintrc",
        ],
        grammar: || tree_sitter_ini::LANGUAGE.into(),
        highlights: || {
            &[
                tree_sitter_ini::HIGHLIGHTS_QUERY,
                include_str!("../queries/ini.scm"),
            ]
        },
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    // Bash's grammar, as a dotenv file is its assignments. `.env.local` and the
    // like come from `FILE_NAME_PREFIXES`.
    #[cfg(feature = "bash")]
    Def {
        name: "dotenv",
        extensions: &["env"],
        file_names: &[".env"],
        grammar: || tree_sitter_bash::LANGUAGE.into(),
        highlights: || {
            &[
                tree_sitter_bash::HIGHLIGHT_QUERY,
                include_str!("../queries/dotenv.scm"),
            ]
        },
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "just")]
    Def {
        name: "just",
        extensions: &["just"],
        file_names: &["justfile", ".justfile"],
        grammar: || tree_sitter_just::LANGUAGE.into(),
        highlights: || &[tree_sitter_just::HIGHLIGHTS_QUERY],
        earlier_patterns_win: false,
        injections: Some(tree_sitter_just::INJECTIONS_QUERY),
        hidden: false,
    },
    #[cfg(feature = "diff")]
    Def {
        name: "diff",
        extensions: &["diff", "patch"],
        file_names: &[],
        grammar: || tree_sitter_diff::LANGUAGE.into(),
        highlights: || {
            &[
                tree_sitter_diff::HIGHLIGHTS_QUERY,
                include_str!("../queries/diff.scm"),
            ]
        },
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "json5")]
    Def {
        name: "json5",
        extensions: &["json5"],
        file_names: &[],
        grammar: || tree_sitter_json5::LANGUAGE.into(),
        highlights: || {
            &[
                tree_sitter_json5::HIGHLIGHTS_QUERY,
                include_str!("../queries/json5.scm"),
            ]
        },
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "csv")]
    Def {
        name: "csv",
        extensions: &["csv"],
        file_names: &[],
        grammar: || fe_grammar_csv::LANGUAGE_CSV.into(),
        highlights: || &[include_str!("../queries/csv.scm")],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
    #[cfg(feature = "csv")]
    Def {
        name: "tsv",
        extensions: &["tsv", "tab"],
        file_names: &[],
        grammar: || fe_grammar_csv::LANGUAGE_TSV.into(),
        highlights: || &[include_str!("../queries/csv.scm")],
        earlier_patterns_win: false,
        injections: None,
        hidden: false,
    },
];
