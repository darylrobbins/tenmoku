// SPDX-License-Identifier: Apache-2.0

//! [`TintedIcons`]: line icons, a glyph per language, well-known project
//! file and folder role, each drawn in the theme's colour for its file
//! family or folder role, the `file_icon` token group.
//!
//! A file's icon is Lucide's, or Tabler's mark where Lucide has none for its
//! language or tool; both are data: [`ICON_NAMES`] and [`ICON_EXTENSIONS`] say where
//! it isn't [`LucideIcons`](crate::LucideIcons)' (a README's is Markdown's),
//! looked up in the same order as Lucide's tables, an exact name before an
//! extension, so a name Lucide's tables know (`Cargo.toml`) keeps its icon.
//! Its tint is data too: [`FILE_NAMES`] and [`EXTENSIONS`] map a name to a
//! token, a file neither names takes `file_icon.default`. [`FOLDER_NAMES`]
//! maps a folder's name to its [`FolderRole`], a Lucide folder glyph and a
//! token; any other folder is the plain folder in `file_icon.folder`. Every `file_icon` token is `list.icon` unless a theme
//! sets it, so under a theme that sets none the set draws in one colour.

use std::{collections::HashMap, sync::LazyLock};

use crate::{
    Icon, IconRef, IconSet, file_icons,
    file_icons::{MAX_KEY, by_name, lookup, lucide_folder},
};

/// Rust sources.
pub(crate) const RUST: &str = "file_icon.rust";
/// The compiled and systems languages: C, Go, Java, Haskell, Elixir, ….
pub(crate) const CODE: &str = "file_icon.code";
/// TOML files.
pub(crate) const TOML: &str = "file_icon.toml";
/// Markdown and READMEs.
pub(crate) const MARKDOWN: &str = "file_icon.markdown";
/// Other configuration.
pub(crate) const CONFIG: &str = "file_icon.config";
/// Data, tables and lock files.
pub(crate) const DATA: &str = "file_icon.data";
/// Shell scripts and scripting languages.
pub(crate) const SCRIPT: &str = "file_icon.script";
/// The web's languages.
pub(crate) const WEB: &str = "file_icon.web";
/// Images, video and sound.
pub(crate) const IMAGE: &str = "file_icon.image";
/// Any other file.
pub(crate) const DEFAULT: &str = "file_icon.default";
/// Source folders.
pub(crate) const FOLDER_SOURCE: &str = "file_icon.folder_source";
/// Tests folders.
pub(crate) const FOLDER_TESTS: &str = "file_icon.folder_tests";
/// Documentation folders.
pub(crate) const FOLDER_DOCS: &str = "file_icon.folder_docs";
/// Any other folder.
pub(crate) const FOLDER: &str = "file_icon.folder";

/// Exact file names, ASCII lowercase, whose icon isn't Lucide's: the ones
/// that select a language. Checked before any extension.
pub(crate) const ICON_NAMES: &[(&str, Icon)] = &[
    ("readme", Icon::Markdown),
    ("readme.txt", Icon::Markdown),
    ("readme.rst", Icon::Markdown),
    ("readme.adoc", Icon::Markdown),
    ("readme.org", Icon::Markdown),
    (".editorconfig", Icon::SlidersHorizontal),
    ("makefile", Icon::Hammer),
    ("gnumakefile", Icon::Hammer),
    ("justfile", Icon::ListChecks),
    (".justfile", Icon::ListChecks),
    ("cmakelists.txt", Icon::Triangle),
    ("nginx.conf", Icon::Server),
    ("dockerfile", Icon::BrandDocker),
    ("containerfile", Icon::BrandDocker),
    (".dockerignore", Icon::BrandDocker),
    (".containerignore", Icon::BrandDocker),
    (".gitignore", Icon::BrandGit),
    (".gitattributes", Icon::BrandGit),
    (".gitmodules", Icon::BrandGit),
    (".gitconfig", Icon::BrandGit),
    (".gitkeep", Icon::BrandGit),
    (".git-blame-ignore-revs", Icon::BrandGit),
    ("commit_editmsg", Icon::BrandGit),
    ("merge_msg", Icon::BrandGit),
    ("tag_editmsg", Icon::BrandGit),
    ("git-rebase-todo", Icon::BrandGit),
    (".bashrc", Icon::FileTerminal),
    (".bash_profile", Icon::FileTerminal),
    (".bash_login", Icon::FileTerminal),
    (".bash_logout", Icon::FileTerminal),
    (".bash_aliases", Icon::FileTerminal),
    (".profile", Icon::FileTerminal),
    (".zshrc", Icon::FileTerminal),
    (".zshenv", Icon::FileTerminal),
    (".zprofile", Icon::FileTerminal),
    (".zlogin", Icon::FileTerminal),
    (".zlogout", Icon::FileTerminal),
    (".envrc", Icon::FileTerminal),
    (".vimrc", Icon::LetterV),
    ("_vimrc", Icon::LetterV),
    (".gvimrc", Icon::LetterV),
    // Project files: manifests, a tool's own files, lock files, licences
    // and changelogs. Lucide's tables already give `Cargo.toml`, `go.mod`,
    // `pom.xml`, `build.gradle(.kts)`, `pyproject.toml` and `Gemfile`
    // `package`, and the `lock` extension `file-lock`.
    (".ecrc", Icon::SlidersHorizontal),
    ("kbuild", Icon::Hammer),
    ("cmakecache.txt", Icon::Triangle),
    ("cmakepresets.json", Icon::Triangle),
    ("docker-compose.yml", Icon::BrandDocker),
    ("docker-compose.yaml", Icon::BrandDocker),
    ("compose.yml", Icon::BrandDocker),
    ("compose.yaml", Icon::BrandDocker),
    ("codeowners", Icon::BrandGithub),
    ("go.work", Icon::Package),
    ("gradle.properties", Icon::Package),
    ("gradlew", Icon::Package),
    ("gradlew.bat", Icon::Package),
    ("requirements.txt", Icon::Package),
    ("pipfile", Icon::Package),
    (".python-version", Icon::BrandPython),
    ("py.typed", Icon::BrandPython),
    ("pylintrc", Icon::SlidersHorizontal),
    (".pylintrc", Icon::SlidersHorizontal),
    (".coveragerc", Icon::SlidersHorizontal),
    ("rakefile", Icon::Gem),
    ("podfile", Icon::Gem),
    ("brewfile", Icon::Gem),
    ("guardfile", Icon::Gem),
    ("capfile", Icon::Gem),
    (".ruby-version", Icon::Gem),
    ("pkgbuild", Icon::FileTerminal),
    ("apkbuild", Icon::FileTerminal),
    ("package.json", Icon::BrandNodejs),
    (".nvmrc", Icon::BrandNodejs),
    (".node-version", Icon::BrandNodejs),
    (".npmrc", Icon::BrandNpm),
    (".npmignore", Icon::BrandNpm),
    ("pnpm-workspace.yaml", Icon::BrandPnpm),
    (".pnpmfile.cjs", Icon::BrandPnpm),
    (".yarnrc", Icon::BrandYarn),
    (".yarnrc.yml", Icon::BrandYarn),
    (".yarnrc.yaml", Icon::BrandYarn),
    ("bunfig.toml", Icon::Croissant),
    (".bun-version", Icon::Croissant),
    ("deno.json", Icon::BrandDeno),
    ("deno.jsonc", Icon::BrandDeno),
    ("bun.lockb", Icon::FileLock),
    ("go.work.sum", Icon::FileLock),
    (".babelrc", Icon::FileBraces),
    (".bowerrc", Icon::FileBraces),
    (".eslintrc", Icon::FileBraces),
    (".hintrc", Icon::FileBraces),
    (".jscsrc", Icon::FileBraces),
    (".jsfmtrc", Icon::FileBraces),
    (".jshintrc", Icon::FileBraces),
    (".jslintrc", Icon::FileBraces),
    (".swcrc", Icon::FileBraces),
    (".vuerc", Icon::FileBraces),
    (".watchmanconfig", Icon::FileBraces),
    ("license.md", Icon::Scale),
    ("license.txt", Icon::Scale),
    ("licence.md", Icon::Scale),
    ("licence.txt", Icon::Scale),
    ("copying.md", Icon::Scale),
    ("copying.txt", Icon::Scale),
    ("copyright", Icon::Scale),
    ("copyright.md", Icon::Scale),
    ("copyright.txt", Icon::Scale),
    ("notice.md", Icon::Scale),
    ("notice.txt", Icon::Scale),
    ("changelog", Icon::FileClock),
    ("changelog.md", Icon::FileClock),
    ("changelog.txt", Icon::FileClock),
    ("changes", Icon::FileClock),
    ("changes.md", Icon::FileClock),
    ("changes.txt", Icon::FileClock),
];

/// Extensions without the dot, ASCII lowercase, whose icon isn't Lucide's:
/// a glyph per language, dialects sharing one, and a kind icon (never the
/// blank file) for each other extension Material Icon Theme's set knows. A
/// two-part key is tried before the last extension alone.
pub(crate) const ICON_EXTENSIONS: &[(&str, Icon)] = &[
    ("rs", Icon::BrandRust),
    ("ron", Icon::FileBraces),
    // The compiled and systems languages.
    ("c", Icon::LetterC),
    ("h", Icon::LetterC),
    ("cc", Icon::BrandCpp),
    ("cpp", Icon::BrandCpp),
    ("cxx", Icon::BrandCpp),
    ("c++", Icon::BrandCpp),
    ("hh", Icon::BrandCpp),
    ("hpp", Icon::BrandCpp),
    ("hxx", Icon::BrandCpp),
    ("h++", Icon::BrandCpp),
    ("inl", Icon::BrandCpp),
    ("cs", Icon::BrandCSharp),
    ("csx", Icon::BrandCSharp),
    ("fs", Icon::LetterF),
    ("fsi", Icon::LetterF),
    ("fsx", Icon::LetterF),
    ("go", Icon::BrandGolang),
    ("java", Icon::Coffee),
    ("kt", Icon::BrandKotlin),
    ("kts", Icon::BrandKotlin),
    ("gradle.kts", Icon::BrandKotlin),
    ("scala", Icon::Layers),
    ("sc", Icon::Layers),
    ("groovy", Icon::LetterG),
    ("gradle", Icon::LetterG),
    ("swift", Icon::BrandSwift),
    ("dart", Icon::Target),
    ("zig", Icon::Zap),
    ("zon", Icon::Zap),
    ("hs", Icon::Lambda),
    ("lhs", Icon::Lambda),
    ("ml", Icon::LetterO),
    ("mli", Icon::LetterO),
    ("elm", Icon::TreeDeciduous),
    ("gleam", Icon::Sparkle),
    ("erl", Icon::LetterE),
    ("hrl", Icon::LetterE),
    ("ex", Icon::Droplet),
    ("exs", Icon::Droplet),
    ("heex", Icon::Droplet),
    ("rkt", Icon::Parentheses),
    ("sol", Icon::Diamond),
    // Scripts.
    ("py", Icon::BrandPython),
    ("pyi", Icon::BrandPython),
    ("pyw", Icon::BrandPython),
    ("pyc", Icon::FileCode),
    ("whl", Icon::FileArchive),
    ("rb", Icon::Gem),
    ("erb", Icon::Gem),
    ("rake", Icon::Gem),
    ("gemspec", Icon::Gem),
    ("ru", Icon::Gem),
    ("podspec", Icon::Gem),
    ("lua", Icon::Moon),
    ("pl", Icon::LetterP),
    ("pm", Icon::LetterP),
    ("php", Icon::BrandPhp),
    ("r", Icon::LetterR),
    ("rmd", Icon::LetterR),
    ("jl", Icon::LetterJ),
    ("ksh", Icon::FileTerminal),
    ("csh", Icon::FileTerminal),
    ("nu", Icon::FileTerminal),
    ("ps1", Icon::BrandPowershell),
    ("psm1", Icon::BrandPowershell),
    ("psd1", Icon::BrandPowershell),
    ("vim", Icon::LetterV),
    // The web.
    ("js", Icon::BrandJavascript),
    ("mjs", Icon::BrandJavascript),
    ("cjs", Icon::BrandJavascript),
    ("ts", Icon::BrandTypescript),
    ("mts", Icon::BrandTypescript),
    ("cts", Icon::BrandTypescript),
    ("jsx", Icon::BrandReact),
    ("tsx", Icon::BrandReact),
    ("html", Icon::BrandHtml5),
    ("htm", Icon::BrandHtml5),
    ("xhtml", Icon::BrandHtml5),
    ("css", Icon::BrandCss3),
    ("less", Icon::BrandCss3),
    ("scss", Icon::BrandSass),
    ("sass", Icon::BrandSass),
    ("vue", Icon::BrandVue),
    ("svelte", Icon::BrandSvelte),
    ("astro", Icon::BrandAstro),
    ("twig", Icon::Sprout),
    // Markdown and LaTeX.
    ("md", Icon::Markdown),
    ("markdown", Icon::Markdown),
    ("mdx", Icon::Markdown),
    ("tex", Icon::Sigma),
    ("latex", Icon::Sigma),
    ("ltx", Icon::Sigma),
    ("sty", Icon::Sigma),
    // Configuration.
    ("yaml", Icon::FileSliders),
    ("yml", Icon::FileSliders),
    ("ini", Icon::SlidersHorizontal),
    ("cfg", Icon::SlidersHorizontal),
    ("conf", Icon::SlidersHorizontal),
    ("config", Icon::SlidersHorizontal),
    ("properties", Icon::SlidersHorizontal),
    ("env", Icon::FileKey),
    ("dockerfile", Icon::BrandDocker),
    ("containerfile", Icon::BrandDocker),
    ("mk", Icon::Hammer),
    ("mak", Icon::Hammer),
    ("just", Icon::ListChecks),
    ("cmake", Icon::Triangle),
    ("nix", Icon::Snowflake),
    ("tf", Icon::BrandTerraform),
    ("tfvars", Icon::BrandTerraform),
    ("hcl", Icon::BrandTerraform),
    ("bzl", Icon::Star),
    ("star", Icon::Star),
    ("bazel", Icon::Star),
    ("kdl", Icon::LetterK),
    ("gitignore", Icon::BrandGit),
    // Data.
    ("jsonl", Icon::FileBraces),
    ("ndjson", Icon::FileBraces),
    ("webmanifest", Icon::FileBraces),
    ("har", Icon::FileBraces),
    ("jsonld", Icon::FileBraces),
    ("geojson", Icon::FileBraces),
    ("ipynb", Icon::FileBraces),
    ("bowerrc", Icon::FileBraces),
    ("jscsrc", Icon::FileBraces),
    ("jslintrc", Icon::FileBraces),
    ("vuerc", Icon::FileBraces),
    ("eslintrc", Icon::FileBraces),
    ("jsfmtrc", Icon::FileBraces),
    ("jshintrc", Icon::FileBraces),
    ("swcrc", Icon::FileBraces),
    ("hintrc", Icon::FileBraces),
    ("babelrc", Icon::FileBraces),
    ("code-workspace", Icon::FileBraces),
    ("code-snippets", Icon::FileBraces),
    ("xml", Icon::CodeXml),
    ("xsd", Icon::CodeXml),
    ("xsl", Icon::CodeXml),
    ("xslt", Icon::CodeXml),
    ("plist", Icon::CodeXml),
    ("tab", Icon::FileSpreadsheet),
    ("psv", Icon::FileSpreadsheet),
    ("sqlite3", Icon::Database),
    ("proto", Icon::FileBox),
    ("rej", Icon::FileDiff),
    // Everything else, by kind.
    ("tif", Icon::FileImage),
    ("heic", Icon::FileImage),
    ("icns", Icon::FileImage),
    ("m4v", Icon::FileVideoCamera),
    ("opus", Icon::FileMusic),
    ("aac", Icon::FileMusic),
    ("woff", Icon::FileType),
    ("woff2", Icon::FileType),
    ("ttf", Icon::FileType),
    ("otf", Icon::FileType),
    ("eot", Icon::FileType),
    ("ttc", Icon::FileType),
    ("deb", Icon::FileArchive),
    ("rpm", Icon::FileArchive),
    ("pub", Icon::FileKey),
    ("asc", Icon::FileKey),
    ("gpg", Icon::FileKey),
    ("cer", Icon::FileKey),
    ("cert", Icon::FileKey),
    ("pfx", Icon::FileKey),
    ("pdf", Icon::FileText),
    ("log", Icon::ScrollText),
    ("exe", Icon::AppWindow),
    ("msi", Icon::AppWindow),
];

/// Exact file names, ASCII lowercase, to tints. Checked before any
/// extension.
pub(crate) const FILE_NAMES: &[(&str, &str)] = &[
    ("readme", MARKDOWN),
    ("readme.txt", MARKDOWN),
    ("readme.rst", MARKDOWN),
    ("readme.adoc", MARKDOWN),
    ("readme.org", MARKDOWN),
    (".editorconfig", CONFIG),
    ("makefile", CONFIG),
    ("gnumakefile", CONFIG),
    ("justfile", CONFIG),
    (".justfile", CONFIG),
    ("cmakelists.txt", CONFIG),
    ("nginx.conf", CONFIG),
    ("dockerfile", CONFIG),
    ("containerfile", CONFIG),
    (".dockerignore", CONFIG),
    (".containerignore", CONFIG),
    (".gitignore", CONFIG),
    (".gitattributes", CONFIG),
    (".gitmodules", CONFIG),
    (".gitconfig", CONFIG),
    (".gitkeep", CONFIG),
    (".git-blame-ignore-revs", CONFIG),
    ("commit_editmsg", CONFIG),
    ("merge_msg", CONFIG),
    ("tag_editmsg", CONFIG),
    ("git-rebase-todo", CONFIG),
    (".bashrc", SCRIPT),
    (".bash_profile", SCRIPT),
    (".bash_login", SCRIPT),
    (".bash_logout", SCRIPT),
    (".bash_aliases", SCRIPT),
    (".profile", SCRIPT),
    (".zshrc", SCRIPT),
    (".zshenv", SCRIPT),
    (".zprofile", SCRIPT),
    (".zlogin", SCRIPT),
    (".zlogout", SCRIPT),
    (".envrc", SCRIPT),
    (".vimrc", SCRIPT),
    ("_vimrc", SCRIPT),
    (".gvimrc", SCRIPT),
    // Project files, as ICON_NAMES draws them; a lock file is `data` by its
    // extension or here.
    (".ecrc", CONFIG),
    ("kbuild", CONFIG),
    ("cmakecache.txt", CONFIG),
    ("cmakepresets.json", CONFIG),
    ("docker-compose.yml", CONFIG),
    ("docker-compose.yaml", CONFIG),
    ("compose.yml", CONFIG),
    ("compose.yaml", CONFIG),
    ("codeowners", CONFIG),
    ("cargo.toml", RUST),
    ("go.mod", CODE),
    ("go.work", CODE),
    ("pom.xml", CODE),
    ("build.gradle", CODE),
    ("build.gradle.kts", CODE),
    ("gradle.properties", CODE),
    ("gradlew", CODE),
    ("gradlew.bat", CODE),
    ("pyproject.toml", SCRIPT),
    ("requirements.txt", SCRIPT),
    ("pipfile", SCRIPT),
    (".python-version", SCRIPT),
    ("py.typed", SCRIPT),
    ("pylintrc", CONFIG),
    (".pylintrc", CONFIG),
    (".coveragerc", CONFIG),
    ("gemfile", SCRIPT),
    ("rakefile", SCRIPT),
    ("podfile", SCRIPT),
    ("brewfile", SCRIPT),
    ("guardfile", SCRIPT),
    ("capfile", SCRIPT),
    (".ruby-version", SCRIPT),
    ("pkgbuild", SCRIPT),
    ("apkbuild", SCRIPT),
    ("package.json", WEB),
    (".nvmrc", WEB),
    (".node-version", WEB),
    (".npmrc", CONFIG),
    (".npmignore", CONFIG),
    ("pnpm-workspace.yaml", CONFIG),
    (".pnpmfile.cjs", CONFIG),
    (".yarnrc", CONFIG),
    (".yarnrc.yml", CONFIG),
    (".yarnrc.yaml", CONFIG),
    ("bunfig.toml", CONFIG),
    (".bun-version", CONFIG),
    ("deno.json", CONFIG),
    ("deno.jsonc", CONFIG),
    ("pnpm-lock.yaml", DATA),
    ("go.sum", DATA),
    ("go.work.sum", DATA),
    ("bun.lockb", DATA),
    (".babelrc", DATA),
    (".bowerrc", DATA),
    (".eslintrc", DATA),
    (".hintrc", DATA),
    (".jscsrc", DATA),
    (".jsfmtrc", DATA),
    (".jshintrc", DATA),
    (".jslintrc", DATA),
    (".swcrc", DATA),
    (".vuerc", DATA),
    (".watchmanconfig", DATA),
    ("license.md", DEFAULT),
    ("licence.md", DEFAULT),
    ("copying.md", DEFAULT),
    ("copyright.md", DEFAULT),
    ("notice.md", DEFAULT),
    ("changelog", MARKDOWN),
    ("changelog.md", MARKDOWN),
    ("changelog.txt", MARKDOWN),
    ("changes", MARKDOWN),
    ("changes.md", MARKDOWN),
    ("changes.txt", MARKDOWN),
];

/// Extensions without the dot, ASCII lowercase, to tints. A two-part key
/// is tried before the last extension alone.
pub(crate) const EXTENSIONS: &[(&str, &str)] = &[
    ("rs", RUST),
    ("c", CODE),
    ("h", CODE),
    ("cc", CODE),
    ("cpp", CODE),
    ("cxx", CODE),
    ("c++", CODE),
    ("hh", CODE),
    ("hpp", CODE),
    ("hxx", CODE),
    ("h++", CODE),
    ("inl", CODE),
    ("cs", CODE),
    ("csx", CODE),
    ("fs", CODE),
    ("fsi", CODE),
    ("fsx", CODE),
    ("go", CODE),
    ("java", CODE),
    ("kt", CODE),
    ("kts", CODE),
    ("gradle.kts", CODE),
    ("scala", CODE),
    ("sc", CODE),
    ("groovy", CODE),
    ("gradle", CODE),
    ("swift", CODE),
    ("dart", CODE),
    ("zig", CODE),
    ("zon", CODE),
    ("hs", CODE),
    ("lhs", CODE),
    ("ml", CODE),
    ("mli", CODE),
    ("elm", CODE),
    ("gleam", CODE),
    ("erl", CODE),
    ("hrl", CODE),
    ("ex", CODE),
    ("exs", CODE),
    ("heex", CODE),
    ("rkt", CODE),
    ("sol", CODE),
    ("toml", TOML),
    ("md", MARKDOWN),
    ("markdown", MARKDOWN),
    ("mdx", MARKDOWN),
    ("tex", MARKDOWN),
    ("latex", MARKDOWN),
    ("ltx", MARKDOWN),
    ("sty", MARKDOWN),
    ("yaml", CONFIG),
    ("yml", CONFIG),
    ("ini", CONFIG),
    ("cfg", CONFIG),
    ("conf", CONFIG),
    ("config", CONFIG),
    ("properties", CONFIG),
    ("env", CONFIG),
    ("dockerfile", CONFIG),
    ("containerfile", CONFIG),
    ("mk", CONFIG),
    ("mak", CONFIG),
    ("just", CONFIG),
    ("cmake", CONFIG),
    ("nix", CONFIG),
    ("tf", CONFIG),
    ("tfvars", CONFIG),
    ("hcl", CONFIG),
    ("bzl", CONFIG),
    ("star", CONFIG),
    ("bazel", CONFIG),
    ("kdl", CONFIG),
    ("gitignore", CONFIG),
    ("json", DATA),
    ("jsonc", DATA),
    ("json5", DATA),
    ("jsonl", DATA),
    ("ndjson", DATA),
    ("webmanifest", DATA),
    ("har", DATA),
    ("jsonld", DATA),
    ("geojson", DATA),
    ("ipynb", DATA),
    ("bowerrc", DATA),
    ("jscsrc", DATA),
    ("jslintrc", DATA),
    ("vuerc", DATA),
    ("eslintrc", DATA),
    ("jsfmtrc", DATA),
    ("jshintrc", DATA),
    ("swcrc", DATA),
    ("hintrc", DATA),
    ("babelrc", DATA),
    ("code-workspace", DATA),
    ("code-snippets", DATA),
    ("ron", DATA),
    ("xml", DATA),
    ("xsd", DATA),
    ("xsl", DATA),
    ("xslt", DATA),
    ("plist", DATA),
    ("csv", DATA),
    ("tsv", DATA),
    ("tab", DATA),
    ("psv", DATA),
    ("xlsx", DATA),
    ("xls", DATA),
    ("ods", DATA),
    ("sql", DATA),
    ("sqlite", DATA),
    ("sqlite3", DATA),
    ("db", DATA),
    ("proto", DATA),
    ("diff", DATA),
    ("patch", DATA),
    ("rej", DATA),
    ("lock", DATA),
    ("sh", SCRIPT),
    ("bash", SCRIPT),
    ("zsh", SCRIPT),
    ("ksh", SCRIPT),
    ("fish", SCRIPT),
    ("csh", SCRIPT),
    ("nu", SCRIPT),
    ("ps1", SCRIPT),
    ("psm1", SCRIPT),
    ("psd1", SCRIPT),
    ("bat", SCRIPT),
    ("cmd", SCRIPT),
    ("py", SCRIPT),
    ("pyi", SCRIPT),
    ("pyw", SCRIPT),
    ("pyc", SCRIPT),
    ("whl", SCRIPT),
    ("rb", SCRIPT),
    ("erb", SCRIPT),
    ("rake", SCRIPT),
    ("gemspec", SCRIPT),
    ("ru", SCRIPT),
    ("podspec", SCRIPT),
    ("lua", SCRIPT),
    ("php", SCRIPT),
    ("pl", SCRIPT),
    ("pm", SCRIPT),
    ("r", SCRIPT),
    ("rmd", SCRIPT),
    ("jl", SCRIPT),
    ("vim", SCRIPT),
    ("js", WEB),
    ("mjs", WEB),
    ("cjs", WEB),
    ("jsx", WEB),
    ("ts", WEB),
    ("mts", WEB),
    ("cts", WEB),
    ("tsx", WEB),
    ("html", WEB),
    ("htm", WEB),
    ("xhtml", WEB),
    ("css", WEB),
    ("less", WEB),
    ("scss", WEB),
    ("sass", WEB),
    ("vue", WEB),
    ("svelte", WEB),
    ("astro", WEB),
    ("twig", WEB),
    ("png", IMAGE),
    ("jpg", IMAGE),
    ("jpeg", IMAGE),
    ("gif", IMAGE),
    ("webp", IMAGE),
    ("bmp", IMAGE),
    ("ico", IMAGE),
    ("svg", IMAGE),
    ("tif", IMAGE),
    ("tiff", IMAGE),
    ("avif", IMAGE),
    ("heic", IMAGE),
    ("icns", IMAGE),
    ("mp4", IMAGE),
    ("m4v", IMAGE),
    ("mov", IMAGE),
    ("webm", IMAGE),
    ("mkv", IMAGE),
    ("avi", IMAGE),
    ("mp3", IMAGE),
    ("wav", IMAGE),
    ("flac", IMAGE),
    ("ogg", IMAGE),
    ("opus", IMAGE),
    ("m4a", IMAGE),
    ("aac", IMAGE),
];

/// A folder's role, its glyph and tint: Lucide folder glyphs only, so
/// every folder keeps one shape, drawn the same open or closed (the tree's
/// chevron shows which).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum FolderRole {
    Source,
    Tests,
    Docs,
    Config,
    /// Git's own folders and patches.
    Git,
    /// A forge's: `.github`, `.gitlab`.
    Forge,
    /// Build output.
    Output,
    /// Dependencies vendored or installed.
    Dependencies,
    Examples,
    /// Scripts and tools.
    Scripts,
    /// Assets, styles, themes and translations.
    Assets,
    /// Temporary files and caches.
    Temporary,
}

impl FolderRole {
    /// Every role.
    #[cfg(test)]
    pub(crate) const ALL: &[FolderRole] = &[
        FolderRole::Source,
        FolderRole::Tests,
        FolderRole::Docs,
        FolderRole::Config,
        FolderRole::Git,
        FolderRole::Forge,
        FolderRole::Output,
        FolderRole::Dependencies,
        FolderRole::Examples,
        FolderRole::Scripts,
        FolderRole::Assets,
        FolderRole::Temporary,
    ];

    /// Its glyph, open or closed.
    pub(crate) fn icon(self) -> Icon {
        match self {
            FolderRole::Source => Icon::FolderCode,
            FolderRole::Tests => Icon::FolderCheck,
            FolderRole::Docs => Icon::FolderPen,
            FolderRole::Config => Icon::FolderCog,
            FolderRole::Git => Icon::FolderGit,
            FolderRole::Forge => Icon::FolderGit2,
            FolderRole::Output => Icon::FolderOutput,
            FolderRole::Dependencies => Icon::FolderArchive,
            FolderRole::Examples => Icon::FolderKanban,
            FolderRole::Scripts => Icon::FolderInput,
            FolderRole::Assets => Icon::FolderBookmark,
            FolderRole::Temporary => Icon::FolderClock,
        }
    }

    /// Its token: source, tests and docs their own, the rest the plain
    /// folder's.
    pub(crate) fn tint(self) -> &'static str {
        match self {
            FolderRole::Source => FOLDER_SOURCE,
            FolderRole::Tests => FOLDER_TESTS,
            FolderRole::Docs => FOLDER_DOCS,
            _ => FOLDER,
        }
    }
}

/// Folder names, ASCII lowercase, to their roles: every name Material Icon
/// Theme's set gives a folder icon, and a few more. A name with a leading
/// `.` or `_`, or a trailing `_` (`.github`, `__tests__`), is looked up
/// without them if it isn't here as it is.
pub(crate) const FOLDER_NAMES: &[(&str, FolderRole)] = &[
    ("src", FolderRole::Source),
    ("srcs", FolderRole::Source),
    ("source", FolderRole::Source),
    ("sources", FolderRole::Source),
    ("code", FolderRole::Source),
    ("lib", FolderRole::Source),
    ("libs", FolderRole::Source),
    ("library", FolderRole::Source),
    ("crates", FolderRole::Source),
    ("package", FolderRole::Source),
    ("packages", FolderRole::Source),
    ("pkg", FolderRole::Source),
    ("pkgs", FolderRole::Source),
    ("app", FolderRole::Source),
    ("include", FolderRole::Source),
    ("includes", FolderRole::Source),
    ("inc", FolderRole::Source),
    ("util", FolderRole::Source),
    ("utils", FolderRole::Source),
    ("utility", FolderRole::Source),
    ("utilities", FolderRole::Source),
    ("rust", FolderRole::Source),
    ("tests", FolderRole::Tests),
    ("test", FolderRole::Tests),
    ("testing", FolderRole::Tests),
    ("spec", FolderRole::Tests),
    ("specs", FolderRole::Tests),
    ("e2e", FolderRole::Tests),
    ("fixture", FolderRole::Tests),
    ("fixtures", FolderRole::Tests),
    ("mock", FolderRole::Tests),
    ("mocks", FolderRole::Tests),
    ("testdata", FolderRole::Tests),
    ("snapshots", FolderRole::Tests),
    ("bench", FolderRole::Tests),
    ("benches", FolderRole::Tests),
    ("benchmark", FolderRole::Tests),
    ("benchmarks", FolderRole::Tests),
    ("perf", FolderRole::Tests),
    ("docs", FolderRole::Docs),
    ("doc", FolderRole::Docs),
    ("documentation", FolderRole::Docs),
    ("wiki", FolderRole::Docs),
    ("config", FolderRole::Config),
    ("configs", FolderRole::Config),
    ("configuration", FolderRole::Config),
    ("cfg", FolderRole::Config),
    ("conf", FolderRole::Config),
    ("settings", FolderRole::Config),
    ("options", FolderRole::Config),
    ("prefs", FolderRole::Config),
    ("meta-inf", FolderRole::Config),
    ("vscode", FolderRole::Config),
    ("idea", FolderRole::Config),
    ("cargo", FolderRole::Config),
    ("git", FolderRole::Git),
    ("githooks", FolderRole::Git),
    ("patches", FolderRole::Git),
    ("submodules", FolderRole::Git),
    ("github", FolderRole::Forge),
    ("gitlab", FolderRole::Forge),
    ("target", FolderRole::Output),
    ("dist", FolderRole::Output),
    ("out", FolderRole::Output),
    ("output", FolderRole::Output),
    ("build", FolderRole::Output),
    ("builds", FolderRole::Output),
    ("built", FolderRole::Output),
    ("compiled", FolderRole::Output),
    ("bin", FolderRole::Output),
    ("release", FolderRole::Output),
    ("node_modules", FolderRole::Dependencies),
    ("node", FolderRole::Dependencies),
    ("nodejs", FolderRole::Dependencies),
    ("vendor", FolderRole::Dependencies),
    ("vendors", FolderRole::Dependencies),
    ("third_party", FolderRole::Dependencies),
    ("third-party", FolderRole::Dependencies),
    ("thirdparty", FolderRole::Dependencies),
    ("external", FolderRole::Dependencies),
    ("examples", FolderRole::Examples),
    ("example", FolderRole::Examples),
    ("demo", FolderRole::Examples),
    ("demos", FolderRole::Examples),
    ("sample", FolderRole::Examples),
    ("samples", FolderRole::Examples),
    ("scripts", FolderRole::Scripts),
    ("script", FolderRole::Scripts),
    ("xtask", FolderRole::Scripts),
    ("tools", FolderRole::Scripts),
    ("tooling", FolderRole::Scripts),
    ("devtools", FolderRole::Scripts),
    ("assets", FolderRole::Assets),
    ("asset", FolderRole::Assets),
    ("resources", FolderRole::Assets),
    ("resource", FolderRole::Assets),
    ("res", FolderRole::Assets),
    ("static", FolderRole::Assets),
    ("public", FolderRole::Assets),
    ("www", FolderRole::Assets),
    ("web", FolderRole::Assets),
    ("images", FolderRole::Assets),
    ("image", FolderRole::Assets),
    ("img", FolderRole::Assets),
    ("imgs", FolderRole::Assets),
    ("icons", FolderRole::Assets),
    ("icon", FolderRole::Assets),
    ("figures", FolderRole::Assets),
    ("screenshots", FolderRole::Assets),
    ("pics", FolderRole::Assets),
    ("pictures", FolderRole::Assets),
    ("photos", FolderRole::Assets),
    ("fonts", FolderRole::Assets),
    ("font", FolderRole::Assets),
    ("themes", FolderRole::Assets),
    ("theme", FolderRole::Assets),
    ("colors", FolderRole::Assets),
    ("colours", FolderRole::Assets),
    ("styles", FolderRole::Assets),
    ("style", FolderRole::Assets),
    ("css", FolderRole::Assets),
    ("stylesheets", FolderRole::Assets),
    ("i18n", FolderRole::Assets),
    ("l10n", FolderRole::Assets),
    ("locales", FolderRole::Assets),
    ("locale", FolderRole::Assets),
    ("lang", FolderRole::Assets),
    ("translations", FolderRole::Assets),
    ("tmp", FolderRole::Temporary),
    ("temp", FolderRole::Temporary),
    ("cache", FolderRole::Temporary),
];

// Lucide's tables, then ours over them.
static ICON_NAME_MAP: LazyLock<HashMap<&'static str, Icon>> = LazyLock::new(|| {
    (file_icons::FILE_NAMES.iter().chain(ICON_NAMES))
        .copied()
        .collect()
});
static ICON_EXTENSION_MAP: LazyLock<HashMap<&'static str, Icon>> = LazyLock::new(|| {
    (file_icons::EXTENSIONS.iter().chain(ICON_EXTENSIONS))
        .copied()
        .collect()
});
static NAME_MAP: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| FILE_NAMES.iter().copied().collect());
static EXTENSION_MAP: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| EXTENSIONS.iter().copied().collect());
static FOLDER_MAP: LazyLock<HashMap<&'static str, FolderRole>> =
    LazyLock::new(|| FOLDER_NAMES.iter().copied().collect());

/// Line icons, a glyph per language and well-known project file (Lucide's,
/// Tabler's marks where Lucide has none), in the theme's `file_icon`
/// colours: Rust, the compiled languages, TOML, Markdown, configuration,
/// data, scripts, the web and media each in their own; folders by role, a
/// glyph each, source, tests and docs in their own colours.
///
/// Not installed by default: install it with
/// [`set_icon_set`](crate::set_icon_set).
#[derive(Clone, Copy, Debug, Default)]
pub struct TintedIcons;

impl TintedIcons {
    /// The icon a file called `name` is drawn with: [`ICON_NAMES`] and
    /// [`ICON_EXTENSIONS`] over Lucide's tables, in Lucide's order.
    pub(crate) fn file_icon(name: &str) -> Icon {
        by_name(
            &ICON_NAME_MAP,
            &ICON_EXTENSION_MAP,
            Icon::FileKey,
            Icon::File,
            name,
        )
    }

    /// The token whose colour a file called `name` is drawn in.
    pub fn file_tint(name: &str) -> &'static str {
        by_name(&NAME_MAP, &EXTENSION_MAP, CONFIG, DEFAULT, name)
    }

    /// The role of a folder called `name`, if it has one: [`FOLDER_NAMES`]'
    /// as it is, ASCII case-insensitively, else without leading `.`s and
    /// `_`s and trailing `_`s.
    pub(crate) fn folder_role(name: &str) -> Option<FolderRole> {
        let mut buf = [0u8; MAX_KEY];
        let trimmed = name.trim_start_matches(['.', '_']).trim_end_matches('_');
        lookup(&FOLDER_MAP, name, &mut buf).or_else(|| lookup(&FOLDER_MAP, trimmed, &mut buf))
    }

    /// The token whose colour a folder called `name` is drawn in.
    pub fn folder_tint(name: &str) -> &'static str {
        Self::folder_role(name).map_or(FOLDER, FolderRole::tint)
    }
}

impl IconSet for TintedIcons {
    fn file(&self, name: &str) -> IconRef {
        IconRef::Tinted(Self::file_icon(name), Self::file_tint(name))
    }

    fn folder(&self, expanded: bool) -> IconRef {
        IconRef::Tinted(lucide_folder(expanded), FOLDER)
    }

    fn folder_named(&self, name: &str, expanded: bool) -> IconRef {
        match Self::folder_role(name) {
            Some(role) => IconRef::Tinted(role.icon(), role.tint()),
            None => self.folder(expanded),
        }
    }
}
